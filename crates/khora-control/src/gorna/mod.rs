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
//! costs by what the DCC measured, [`decide`]s an envelope for each agent, and
//! lends it through [`Agent::apply`]. The DCC decides *how much*; each agent
//! chooses *how*, within its envelope. A death spiral, too many stalled
//! agents, or floors that do not fit make the round `Trigger::Safety`: every
//! agent is lent its floor.

use crate::analysis::AnalysisReport;
use crate::dcc_context::Context;
use khora_core::agent::gorna::protocol::{
    Allocation, Envelope, Epoch, Offer, OfferRequest, Trigger,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, StrategyId};
use khora_core::agent::{Agent, AgentImportance};
use khora_core::lane::cost::{PerDevice, PerLink};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

mod decide;

pub use decide::{decide, Bid, Round};

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
    /// The last epoch opened.
    epoch: Epoch,
    /// The allocation in force, until the next round.
    in_force: Option<Allocation>,
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
            epoch: Epoch::default(),
            in_force: None,
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

    /// One round of the protocol: asks every agent in `agents` for an offer
    /// (one `OfferRequest` for the round's epoch, the frame's target and the
    /// context's mode), calibrates the quoted rows by `measured_costs`,
    /// [`decide`]s, and lends each agent its envelope, pin and allowed rows
    /// through [`Agent::apply`]. Returns the allocation, which stays in force
    /// until the next round.
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
                mode: context.mode.clone(),
                reply_by: Instant::now() + self.lock_timeout,
            };
            let mut offer = agent.offer(&request);
            let late = Instant::now() > request.reply_by;
            let importance = agent.execution_timing().importance;
            let id = agent.id();
            drop(agent);
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
        let allocation = decide(&Round {
            epoch,
            trigger,
            target,
            memory,
            link_rate: PerLink::default(),
            waves: &self.wave_plan,
            bids: &bids,
        });
        self.lend(&allocation, agents);
        allocation
    }

    /// Lends every agent in `agents` its envelope, pin and allowed rows from a
    /// recorded `allocation`, without asking for offers or deciding: a replay.
    /// The allocation is then the one in force.
    pub fn reissue(&mut self, allocation: &Allocation, agents: &[Arc<Mutex<dyn Agent>>]) {
        self.epoch = self.epoch.max(allocation.epoch);
        self.lend(allocation, agents);
    }

    /// Lends each agent of `agents` what `allocation` names for it, and keeps
    /// the allocation as the one in force.
    fn lend(&mut self, allocation: &Allocation, agents: &[Arc<Mutex<dyn Agent>>]) {
        for agent_mutex in agents {
            let Some(mut agent) = try_lock_agent_with_timeout(agent_mutex, self.lock_timeout)
            else {
                log::warn!("GORNA: an agent could not be locked for its envelope; skipped");
                continue;
            };
            let id = agent.id();
            let Some((_, envelope)) = allocation.envelopes.iter().find(|(agent, _)| *agent == id)
            else {
                continue;
            };
            let pin = allocation
                .pins
                .iter()
                .find(|(agent, _)| *agent == id)
                .map(|(_, row)| *row);
            let allowed = allocation
                .allowed
                .iter()
                .find(|(agent, _)| *agent == id)
                .map(|(_, rows)| rows.as_slice());
            agent.apply(allocation.epoch, envelope, pin, allowed);
        }
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
                let status = agent.report_status();
                if status.is_stalled {
                    log::warn!(
                        "GORNA: agent {} is stalled. Health={:.2}: {}",
                        status.agent_id,
                        status.health_score,
                        status.message
                    );
                }
                status.is_stalled
            })
            .count()
    }
}

/// Anchors `offer`'s quoted costs in what the DCC measured: every priced
/// row's time is scaled so the row running now costs `measured_ms`. Relative
/// order between rows is kept; the factor is clamped.
fn calibrate(offer: &mut Offer, measured_ms: Option<f64>) {
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
