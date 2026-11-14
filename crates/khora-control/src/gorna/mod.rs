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

//! GORNA, the DCC's side of the protocol: one round per epoch, from each
//! agent's [`Offer`] to the envelopes it [`decide`](decide())s.
//!
//! See the book: concepts/gorna.md, "How the DCC shares the time".

use crate::analysis::AnalysisReport;
use crate::dcc_context::DccContext;
use khora_core::agent::gorna::protocol::{
    Allocation, Envelope, Epoch, Offer, OfferRequest, Trigger, Why,
};
use khora_core::agent::gorna::{
    AdaptationMode, AgentHints, AgentId, Correction, DecisionView, EpochRecord, Planning,
    StrategyId,
};
use khora_core::agent::{Agent, AgentImportance};
use khora_core::lane::cost::{PerDevice, PerLink};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

mod admission;
pub(crate) mod arbiter;
pub mod cost_book;
mod decide;
mod plan;
pub mod planning;
mod round;
mod share;
mod units;

use arbiter::Arbiter;
use cost_book::CostBook;
pub use decide::{decide, Bid, Round};
use planning::{Planner, PlanningConfig};

const MAX_STALLED_AGENTS: usize = 2;

/// Returns the row `decision` planned for each agent it lends an envelope for.
///
/// A `Safety` round plans every offering agent's floor. An agent that keeps
/// the envelope it holds (late, or `Stable`) has no row planned.
fn planned_rows(decision: &arbiter::Decision, bids: &[Bid]) -> Vec<(AgentId, StrategyId)> {
    let floor_of = |agent: AgentId| {
        (bids.iter())
            .find(|bid| bid.offer.agent == agent && !bid.offer.rows.is_empty())
            .map(|bid| bid.offer.floor)
    };
    if decision.allocation.trigger == Trigger::Safety {
        return (decision.rows.iter())
            .filter_map(|(agent, _)| floor_of(*agent).map(|floor| (*agent, floor)))
            .collect();
    }
    (decision.rows.iter())
        .filter(|(agent, _)| !decision.kept.contains(agent))
        .copied()
        .collect()
}

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
    /// The scheduler's latest waves of agents run side by side; empty means
    /// serial execution.
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
    /// What the DCC learnt of each row's measured time against its quote.
    book: CostBook,
    /// The last round's correction of each offered row.
    corrections: Vec<(AgentId, StrategyId, Correction)>,
    /// The row the last round planned for each agent.
    planned: Vec<(AgentId, StrategyId)>,
    /// The row each agent last said it runs: what a busy agent's kept
    /// envelope is corrected by.
    runs: Vec<(AgentId, StrategyId)>,
    /// The last round's offers, as the agents answered them.
    offers: Vec<Offer>,
    /// The last round's target time for the frame.
    target: Duration,
    /// Each agent's importance, as the last round read it, by agent.
    importance: Vec<(AgentId, AgentImportance)>,
    /// The bytes the last round let the agents hold on each device.
    memory: PerDevice<Option<u64>>,
    /// Each agent seen, by a weak reference to its lock.
    ///
    /// Names an agent busy through a round without locking it; the weak
    /// reference keeps a removed agent's lock address from being reused.
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
            book: CostBook::new(cost_book::CostBookConfig::default()),
            corrections: Vec::new(),
            planned: Vec::new(),
            runs: Vec::new(),
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

    /// Returns the allocation in force, if a round ran.
    pub fn in_force(&self) -> Option<&Allocation> {
        self.in_force.as_ref()
    }

    /// Returns what decided the last round, and why: the proposer, the trigger,
    /// the constraint that stopped the plan growing, the upgrades refused.
    pub fn why(&self) -> &Why {
        &self.why
    }

    /// Returns the last round's offers as the agents answered them, before
    /// the cost book corrected them.
    pub fn offers(&self) -> &[Offer] {
        &self.offers
    }

    /// Returns the agents the last round found at the background phase.
    ///
    /// They run off the frame, so they add nothing to its critical path.
    pub fn background(&self) -> &[AgentId] {
        &self.background
    }

    /// Returns the target time for the frame the last round shared.
    pub fn target(&self) -> Duration {
        self.target
    }

    /// Returns each agent's importance as the last round read it, by agent.
    pub fn importance(&self) -> &[(AgentId, AgentImportance)] {
        &self.importance
    }

    /// Returns the bytes the last round let the agents hold on each device;
    /// `None` where it knew no limit.
    pub fn memory(&self) -> PerDevice<Option<u64>> {
        self.memory
    }

    /// Returns what the DCC learns between rounds for its planning.
    ///
    /// The switches measured, the frames' spread, each agent's workload trend
    /// and class.
    pub fn planner_mut(&mut self) -> &mut Planner {
        &mut self.planner
    }

    /// What the last round planned for beyond its frame.
    pub fn planning(&self) -> &Planning {
        &self.planning
    }

    /// Returns what the DCC learnt of each (agent, row).
    pub fn cost_book(&self) -> &CostBook {
        &self.book
    }

    /// Returns, to update, what the DCC learns of each (agent, row): the
    /// correction from its quoted host time to its measured one.
    pub fn cost_book_mut(&mut self) -> &mut CostBook {
        &mut self.book
    }

    /// Returns the correction of each row the last round was offered.
    pub fn corrections(&self) -> &[(AgentId, StrategyId, Correction)] {
        &self.corrections
    }

    /// The row the last round planned for each agent.
    pub fn planned(&self) -> &[(AgentId, StrategyId)] {
        &self.planned
    }

    /// Returns the decision in force as a tool reads it, `replayed` when it
    /// came from a journal; `None` before any round.
    pub fn view(&self, replayed: bool) -> Option<DecisionView> {
        let allocation = self.in_force.as_ref()?;
        Some(DecisionView {
            epoch: allocation.epoch,
            trigger: allocation.trigger,
            target: self.target,
            why: self.why.clone(),
            envelopes: allocation.envelopes.clone(),
            planned: self.planned.clone(),
            corrections: self.corrections.clone(),
            planning: self.planning.clone(),
            replayed,
        })
    }

    /// Runs one round of the protocol and returns the allocation it decides.
    ///
    /// The allocation stays in force until the next round; the scheduler, not
    /// the round, lends it at a frame start. The round is `Trigger::Safety`
    /// instead of `trigger` when `report` detects a death spiral, too many
    /// agents are stalled, or the floors do not fit.
    ///
    /// See the book: concepts/gorna.md, "How the DCC shares the time".
    pub fn allocate(
        &mut self,
        context: &DccContext,
        report: &AnalysisReport,
        trigger: Trigger,
        agents: &[Arc<Mutex<dyn Agent>>],
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
                        let current = (self.runs.iter())
                            .find(|(agent, _)| *agent == id)
                            .map_or(StrategyId::LowPower, |(_, row)| *row);
                        bids.push(Bid {
                            offer: Offer {
                                agent: id,
                                epoch,
                                rows: Vec::new(),
                                floor: StrategyId::LowPower,
                                current,
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
            let offer = agent.offer(&request);
            let late = Instant::now() > request.reply_by;
            let timing = agent.execution_timing();
            let importance = timing.importance;
            let id = agent.id();
            drop(agent);
            if timing.phase == khora_core::agent::ExecutionPhase::BACKGROUND {
                background.push(id);
            }
            self.offers.push(offer.clone());
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
        // Each row is planned at the time it is measured to take, not the
        // time it quotes: the round decides in measured time.
        let mut corrections = self.book.corrections(&self.offers);
        // A busy agent offered nothing: its kept envelope is corrected by the
        // row it last said it runs.
        for bid in bids.iter().filter(|bid| bid.offer.rows.is_empty()) {
            let (agent, row) = (bid.offer.agent, bid.offer.current);
            corrections.push((agent, row, self.book.correction(agent, row)));
        }
        for offer in &self.offers {
            match self
                .runs
                .iter_mut()
                .find(|(agent, _)| *agent == offer.agent)
            {
                Some((_, row)) => *row = offer.current,
                None => self.runs.push((offer.agent, offer.current)),
            }
        }
        // The envelopes kept, as the agents hold them, in their quoted units.
        let held: Vec<(AgentId, Envelope)> = (bids.iter())
            .filter_map(|bid| bid.kept.map(|kept| (bid.offer.agent, kept)))
            .collect();
        for bid in &mut bids {
            cost_book::correct_bid(bid, &corrections, planning.caution);
        }
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
        // Each agent chooses its row by its own quotes: the envelopes are lent
        // in those units, and held to the plan at them.
        cost_book::lend_in_quotes(
            &mut decision.allocation,
            &self.offers,
            &bids,
            &decision.rows,
            &decision.kept,
            &held,
            &corrections,
            planning.caution,
        );
        self.planned = planned_rows(&decision, &bids);
        self.corrections = corrections;
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

    /// Puts a recorded epoch in force, with its reasons, without asking for
    /// offers: a replay.
    ///
    /// The next live round keeps from it the envelopes of the agents that
    /// answer late.
    pub fn reissue(&mut self, record: &EpochRecord) {
        self.epoch = self.epoch.max(record.allocation.epoch);
        self.in_force = Some(record.allocation.clone());
        self.why = record.why.clone();
        self.planning = record.planning.clone();
        self.corrections = record.corrections.clone();
        self.planned = record.planned.clone();
        self.target = record.target;
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

#[cfg(test)]
mod tests;
