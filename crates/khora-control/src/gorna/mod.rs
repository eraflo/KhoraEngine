// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! GORNA, the DCC's side of the protocol.
//!
//! One round per epoch: the arbitrator asks every agent for an
//! [`Offer`] (one `OfferRequest` each, with a deadline), calibrates the quoted
//! costs by what the DCC measured, and [`decide`]s an envelope for each agent.
//! The DCC publishes the allocation; the scheduler lends it at its next frame
//! start, through [`Agent::apply`] — the DCC's thread never does. The DCC
//! decides *how much*; each agent chooses *how*, within its envelope. A death
//! spiral, too many stalled agents, or floors that do not fit make the round
//! `Trigger::Safety`: every agent is lent its floor.

use crate::analysis::AnalysisReport;
use crate::dcc_context::Context;
use khora_core::agent::gorna::protocol::{
    Allocation, Envelope, Epoch, Offer, OfferRequest, Trigger, Why,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, Planning, StrategyId};
use khora_core::agent::{Agent, AgentImportance};
use khora_core::lane::cost::{PerDevice, PerLink};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

pub(crate) mod arbiter;
mod decide;
pub mod planning;

use arbiter::Arbiter;
pub(crate) use decide::hold_to_quotes;
pub use decide::{decide, Bid, Round};
use planning::{Planner, PlanningConfig};

const MAX_STALLED_AGENTS: usize = 2;

/// Clamp range for the empirical calibration factor applied to agent-quoted
/// costs. Bounds the correction so one pathological measurement (a hitch, a
/// cold cache) can't swing a whole round by orders of magnitude.
const CALIBRATION_FACTOR_MIN: f64 = 0.25;
const CALIBRATION_FACTOR_MAX: f64 = 4.0;

fn try_lock_agent_with_timeout<T: ?Sized>(
    mutex: &Mutex<T>,
    timeout: Duration,
) -> Option<std::sync::MutexGuard<'_, T>> {
    let start = Instant::now();
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Some(guard),
            Err(std::sync::TryLockError::WouldBlock) => {
                if start.elapsed() >= timeout {
                    return None;
                }
                std::thread::yield_now();
            }
            Err(std::sync::TryLockError::Poisoned(err)) => {
                log::error!("Agent mutex poisoned: {}", err);
                return None;
            }
        }
    }
}

/// The DCC's side of GORNA: the rounds, the epochs, and the allocation in
/// force.
pub struct GornaArbitrator {
    /// How long a round waits for an agent's lock, and for its offer.
    lock_timeout: Duration,
    /// Per-agent developer-control mode (default `Learning`).
    modes: HashMap<AgentId, AdaptationMode>,
    /// The scheduler's latest wave grouping (agents that run concurrently),
    /// refreshed by the DCC each tick via [`set_wave_plan`](Self::set_wave_plan).
    /// Empty means serial execution.
    wave_plan: Vec<Vec<AgentId>>,
    /// The agents the last round found at the background, off the frame.
    background: Vec<AgentId>,
    /// The last epoch opened.
    epoch: Epoch,
    /// The allocation in force, until the next round.
    in_force: Option<Allocation>,
    /// What decided the last round, and why.
    why: Why,
    /// The proposers behind the shield.
    arbiter: Arbiter,
    /// What the DCC learns between rounds for its planning.
    planner: Planner,
    /// What the last round planned for beyond its frame.
    planning: Planning,
    /// The last round's offers, as the agents answered them.
    offers: Vec<Offer>,
    /// The last round's target time for the frame.
    target: Duration,
    /// Each agent's importance, as the last round read it, by agent.
    importance: Vec<(AgentId, AgentImportance)>,
    /// The bytes the last round let the agents hold on each device.
    memory: PerDevice<Option<u64>>,
    /// Each agent seen, by a weak reference to its lock: the name of one busy
    /// through a round is known without locking it (the weak reference also
    /// keeps a removed agent's lock address from being reused while held).
    known: Vec<(Weak<Mutex<dyn Agent>>, AgentId)>,
}

impl GornaArbitrator {
    /// Creates an arbitrator that waits at most `lock_timeout` for an agent's
    /// lock and for its offer.
    pub fn new(lock_timeout: Duration) -> Self {
        Self {
            lock_timeout,
            modes: HashMap::new(),
            wave_plan: Vec::new(),
            background: Vec::new(),
            epoch: Epoch::default(),
            in_force: None,
            why: Why::default(),
            arbiter: Arbiter::default(),
            planner: Planner::new(PlanningConfig::default()),
            planning: Planning::default(),
            offers: Vec::new(),
            target: Duration::ZERO,
            importance: Vec::new(),
            memory: PerDevice::default(),
            known: Vec::new(),
        }
    }

    /// Sets the scheduler's latest wave plan: the agents of one wave run side
    /// by side, so a wave costs its slowest member on the critical path.
    pub fn set_wave_plan(&mut self, waves: &[Vec<AgentId>]) {
        self.wave_plan = waves.to_vec();
    }

    /// Sets the [`AdaptationMode`] for an agent — the developer-control surface.
    pub fn set_adaptation_mode(&mut self, agent_id: AgentId, mode: AdaptationMode) {
        self.modes.insert(agent_id, mode);
    }

    /// Returns the [`AdaptationMode`] configured for an agent (default `Learning`).
    pub fn adaptation_mode(&self, agent_id: AgentId) -> AdaptationMode {
        self.modes.get(&agent_id).copied().unwrap_or_default()
    }

    /// The allocation in force, if a round ran.
    pub fn in_force(&self) -> Option<&Allocation> {
        self.in_force.as_ref()
    }

    /// What decided the last round, and why: the proposer, the trigger, the
    /// constraint that stopped the plan growing, the upgrades refused.
    pub fn why(&self) -> &Why {
        &self.why
    }

    /// The last round's offers, as the agents answered them — before the
    /// DCC calibrated them by what it measured.
    pub fn offers(&self) -> &[Offer] {
        &self.offers
    }

    /// The agents the last round found at
    /// [`ExecutionPhase::BACKGROUND`](khora_core::agent::ExecutionPhase::BACKGROUND):
    /// they add nothing to the frame's critical path.
    pub fn background(&self) -> &[AgentId] {
        &self.background
    }

    /// The target time for the frame the last round shared.
    pub fn target(&self) -> Duration {
        self.target
    }

    /// Each agent's importance, as the last round read it, by agent.
    pub fn importance(&self) -> &[(AgentId, AgentImportance)] {
        &self.importance
    }

    /// The bytes the last round let the agents hold on each device; `None`
    /// where it knew no limit.
    pub fn memory(&self) -> PerDevice<Option<u64>> {
        self.memory
    }

    /// What the DCC learns between rounds for its planning: the switches
    /// measured, the frames' spread, each agent's workload trend and class.
    pub fn planner_mut(&mut self) -> &mut Planner {
        &mut self.planner
    }

    /// What the last round planned for beyond its frame.
    pub fn planning(&self) -> &Planning {
        &self.planning
    }

    /// One round of the protocol: asks every agent in `agents` for an offer
    /// (one `OfferRequest` for the round's epoch and the frame's target), calibrates the quoted rows by `measured_costs`, and
    /// [`decide`]s each agent's envelope, pin and allowed rows. Returns the
    /// allocation, which stays in force until the next round; the round
    /// lends nothing itself — the scheduler lends the allocation at its next
    /// frame start.
    ///
    /// `trigger` says why the round opens. The round is `Trigger::Safety`
    /// instead when `report` detects a death spiral, when too many agents are
    /// stalled, or when the floors do not fit.
    pub fn allocate(
        &mut self,
        context: &Context,
        report: &AnalysisReport,
        trigger: Trigger,
        agents: &[Arc<Mutex<dyn Agent>>],
        measured_costs: &HashMap<AgentId, f64>,
        hints: &HashMap<AgentId, AgentHints>,
    ) -> Allocation {
        self.epoch = Epoch(self.epoch.0 + 1);
        let epoch = self.epoch;
        let target_ms = f64::from(report.suggested_latency_ms * context.global_budget_multiplier);
        let target = Duration::from_secs_f64(target_ms.max(0.0) / 1_000.0);
        self.target = target;
        self.offers.clear();
        let stalled = self.stalled(agents);
        let trigger = if report.death_spiral_detected || stalled >= MAX_STALLED_AGENTS {
            log::error!(
                "GORNA: death spiral ({stalled} stalled agents): every agent is lent its floor"
            );
            Trigger::Safety
        } else {
            trigger
        };

        // Agents removed since the last round are forgotten.
        self.known.retain(|(weak, _)| weak.strong_count() > 0);
        let mut bids = Vec::with_capacity(agents.len());
        let mut background: Vec<AgentId> = Vec::new();
        for agent_mutex in agents {
            let weak = Arc::downgrade(agent_mutex);
            let Some(mut agent) = try_lock_agent_with_timeout(agent_mutex, self.lock_timeout)
            else {
                // Busy through the whole deadline: like a late offer, the
                // agent keeps the envelope it holds (it runs on it still).
                let known = self.known.iter().find(|(seen, _)| seen.ptr_eq(&weak));
                if let Some(id) = known.map(|(_, id)| *id) {
                    log::warn!(
                        "GORNA: agent {id} was busy through the round; it keeps its envelope"
                    );
                    if let Some(kept) = self.kept(id) {
                        bids.push(Bid {
                            offer: Offer {
                                agent: id,
                                epoch,
                                rows: Vec::new(),
                                floor: StrategyId::LowPower,
                                current: StrategyId::LowPower,
                                health: 1.0,
                            },
                            importance: AgentImportance::Optional,
                            mode: self.adaptation_mode(id),
                            hints: hints.get(&id).copied().unwrap_or_default(),
                            kept: Some(kept),
                            late: true,
                        });
                    }
                }
                continue;
            };
            match self.known.iter_mut().find(|(seen, _)| seen.ptr_eq(&weak)) {
                Some((_, id)) => *id = agent.id(),
                None => self.known.push((weak, agent.id())),
            }
            let request = OfferRequest {
                epoch,
                target,
                pressure: 0.0,
                reply_by: Instant::now() + self.lock_timeout,
            };
            let mut offer = agent.offer(&request);
            let late = Instant::now() > request.reply_by;
            let timing = agent.execution_timing();
            let importance = timing.importance;
            let id = agent.id();
            drop(agent);
            if timing.default_phase == khora_core::agent::ExecutionPhase::BACKGROUND {
                background.push(id);
            }
            self.offers.push(offer.clone());
            calibrate(&mut offer, measured_costs.get(&id).copied());
            bids.push(Bid {
                kept: self.kept(id),
                late,
                importance,
                mode: self.adaptation_mode(id),
                hints: hints.get(&id).copied().unwrap_or_default(),
                offer,
            });
        }

        let memory = PerDevice {
            gpu: context
                .hardware
                .available_vram
                .or(context.hardware.total_vram),
            ..PerDevice::default()
        };
        // What the round plans for beyond its frame, from the offers as the
        // agents answered them: which row each one runs.
        let planning = self.planner.planning(&self.offers, epoch);
        let mut decision = self.arbiter.decide(&Round {
            epoch,
            trigger,
            target,
            memory,
            link_rate: PerLink::default(),
            waves: &self.wave_plan,
            background: &background,
            bids: &bids,
            planning: planning.clone(),
        });
        // Each agent chooses its row by its own quotes: the envelopes are held
        // to the plan at those too.
        decide::hold_to_quotes(
            &mut decision.allocation,
            &self.offers,
            &bids,
            &decision.rows,
            &decision.kept,
        );
        self.planning = planning;
        self.background = background;
        self.importance = bids
            .iter()
            .map(|bid| (bid.offer.agent, bid.importance))
            .collect();
        self.importance.sort_by_key(|(agent, _)| *agent);
        self.memory = memory;
        self.why = decision.why;
        self.in_force = Some(decision.allocation.clone());
        decision.allocation
    }

    /// Puts a recorded `allocation` in force without asking for offers or
    /// deciding: a replay. The DCC publishes it for the scheduler to lend,
    /// and the next live round keeps from it the envelopes of the agents
    /// that answer late.
    pub fn reissue(&mut self, allocation: &Allocation) {
        self.epoch = self.epoch.max(allocation.epoch);
        self.in_force = Some(allocation.clone());
    }

    /// The envelope `agent` holds from the allocation in force.
    fn kept(&self, agent: AgentId) -> Option<Envelope> {
        self.in_force.as_ref().and_then(|allocation| {
            allocation
                .envelopes
                .iter()
                .find(|(id, _)| *id == agent)
                .map(|(_, envelope)| *envelope)
        })
    }

    /// How many of `agents` report they are stalled.
    fn stalled(&self, agents: &[Arc<Mutex<dyn Agent>>]) -> usize {
        agents
            .iter()
            .filter_map(|agent| try_lock_agent_with_timeout(agent, self.lock_timeout))
            .filter(|agent| {
                let report = agent.report();
                if report.stalled {
                    log::warn!(
                        "GORNA: agent {} is stalled. Health={:.2}: {}",
                        report.agent,
                        report.health,
                        report.message.as_deref().unwrap_or("")
                    );
                }
                report.stalled
            })
            .count()
    }
}

/// Anchors `offer`'s quoted costs in what the DCC measured: every priced
/// row's time is scaled so the row running now costs `measured_ms`. Relative
/// order between rows is kept; the factor is clamped.
pub(crate) fn calibrate(offer: &mut Offer, measured_ms: Option<f64>) {
    let Some(measured_ms) = measured_ms else {
        return;
    };
    let busiest = |row: &khora_core::agent::gorna::protocol::OfferRow| {
        let time = &row.cost.time;
        time.cpu.max(time.gpu).max(time.npu).max(time.other)
    };
    let Some(quoted_ms) = offer
        .rows
        .iter()
        .find(|row| row.id == offer.current)
        .map(|row| busiest(row).as_secs_f64() * 1_000.0)
        .filter(|ms| *ms > f64::EPSILON)
    else {
        return;
    };
    let factor = (measured_ms / quoted_ms).clamp(CALIBRATION_FACTOR_MIN, CALIBRATION_FACTOR_MAX);
    let scale = |time: Duration| Duration::from_secs_f64(time.as_secs_f64() * factor);
    for row in &mut offer.rows {
        let time = &mut row.cost.time;
        time.cpu = scale(time.cpu);
        time.gpu = scale(time.gpu);
        time.npu = scale(time.npu);
        time.other = scale(time.other);
    }
}

#[cfg(test)]
mod tests;
