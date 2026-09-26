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

//! GORNA Arbitrator implementation.
//!
//! This module contains the **Goal-Oriented Resource Negotiation & Allocation**
//! logic. The arbitrator is responsible for:
//!
//! 1. Polling agent health via `report_status()`.
//! 2. Sending `NegotiationRequest` to each agent and collecting strategy options.
//! 3. Running a global budget-fitting solver that respects total frame time.
//! 4. Applying thermal/battery multipliers from the `AnalysisReport`.
//! 5. Detecting and handling "death spiral" conditions.
//! 6. Issuing `ResourceBudget` to each agent.

use crate::analysis::AnalysisReport;
use crate::context::Context;
use khora_core::agent::Agent;
use khora_core::control::gorna::{
    AdaptationMode, AgentHints, AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints,
    StrategyId, StrategyOption, TickDecisions,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod fitting;

const MAX_STALLED_AGENTS: usize = 2;

/// Clamp range for the empirical calibration factor applied to agent-quoted
/// strategy costs. Bounds the correction so one pathological measurement
/// (a hitch, a cold cache) can't swing the whole fit by orders of magnitude.
const CALIBRATION_FACTOR_MIN: f64 = 0.25;
const CALIBRATION_FACTOR_MAX: f64 = 4.0;

/// Ordinal rank of a strategy for clamping (`Bounded` mode):
/// `LowPower < Balanced < HighPerformance`, with `Custom` ranked above the
/// standard tiers.
fn strategy_rank(id: StrategyId) -> u8 {
    match id {
        StrategyId::LowPower => 0,
        StrategyId::Balanced => 1,
        StrategyId::HighPerformance => 2,
        StrategyId::Custom(_) => 3,
    }
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

/// Arbitrates resource allocation between multiple ISAs.
///
/// The arbitrator implements a two-pass approach:
/// - **Pass 1 (Negotiation)**: Collects strategy options from all agents.
/// - **Pass 2 (Fitting)**: Selects the optimal strategy combination that fits
///   within the global frame budget, respecting priorities and VRAM constraints.
pub struct GornaArbitrator {
    lock_timeout: Duration,
    /// Per-agent developer-control mode (default `Learning`). Configured by the
    /// host; consulted at issuance so a `Manual` agent is never overridden.
    modes: HashMap<AgentId, AdaptationMode>,
    /// The scheduler's latest wave grouping (agents that run concurrently),
    /// refreshed by the DCC each tick via [`set_wave_plan`](Self::set_wave_plan).
    /// Empty means serial execution: budget fitting then sums per-agent costs.
    wave_plan: Vec<Vec<AgentId>>,
}

/// A collected negotiation from a single agent, used during the fitting pass.
struct AgentNegotiation {
    agent_index: usize,
    agent_id: AgentId,
    priority: f32,
    strategies: Vec<StrategyOption>,
}

/// A resolved allocation for a single agent.
struct AgentAllocation {
    agent_index: usize,
    strategy: StrategyOption,
}

impl GornaArbitrator {
    /// Creates a new arbitrator with the specified lock timeout.
    ///
    /// The lock timeout determines how long to wait when acquiring locks on agents
    /// during negotiation and budget issuance. Agents that cannot be locked within
    /// this timeout are skipped.
    pub fn new(lock_timeout: Duration) -> Self {
        Self {
            lock_timeout,
            modes: HashMap::new(),
            wave_plan: Vec::new(),
        }
    }

    /// Sets the scheduler's latest wave plan (how agents are grouped for
    /// concurrent execution). Budget fitting costs each wave by its critical
    /// path (`max` of its members); an empty plan means serial execution, so
    /// fitting falls back to summing per-agent costs — bit-identical to the
    /// pre-parallel behaviour.
    pub fn set_wave_plan(&mut self, waves: &[Vec<AgentId>]) {
        self.wave_plan = waves.to_vec();
    }

    /// Sets the [`AdaptationMode`] for an agent — the developer-control surface.
    /// `Manual(strategy)` pins the agent; `Learning` (default) lets GORNA negotiate.
    pub fn set_adaptation_mode(&mut self, agent_id: AgentId, mode: AdaptationMode) {
        self.modes.insert(agent_id, mode);
    }

    /// Returns the [`AdaptationMode`] configured for an agent (default `Learning`).
    pub fn adaptation_mode(&self, agent_id: AgentId) -> AdaptationMode {
        self.modes.get(&agent_id).copied().unwrap_or_default()
    }
    /// Performs a full GORNA arbitration round.
    ///
    /// # Arguments
    /// - `context`: The current DCC situational model (phase, hardware, multiplier).
    /// - `report`: The analysis report from the `HeuristicEngine`.
    /// - `agents`: The registered ISA agents.
    /// - `measured_costs`: Per-agent measured execution cost in milliseconds
    ///   (from the DCC's empirical cost models). Used to calibrate the agents'
    ///   self-quoted strategy estimates against reality; agents without a
    ///   measurement keep their quotes as-is (cold start).
    /// - `replay`: When `Some`, issue the recorded strategy per agent instead of
    ///   the negotiated fit (deterministic replay — bypasses budget fitting and
    ///   the per-agent `AdaptationMode`). `None` for normal live arbitration.
    /// - `hints`: Per-agent developer [`AgentHints`] accumulated by the DCC.
    ///   `Prioritize` biases the negotiation priority (which agents the fit
    ///   upgrades first); `Cap` clamps the issued strategy to a time ceiling.
    ///   Advisory only — `replay` and a `Manual` pin both override a hint.
    ///
    /// Returns the [`TickDecisions`] actually issued this tick (agent → strategy),
    /// so the DCC can record them for later replay.
    pub fn arbitrate(
        &self,
        context: &Context,
        report: &AnalysisReport,
        agents: &mut [Arc<Mutex<dyn Agent>>],
        measured_costs: &HashMap<AgentId, f64>,
        replay: Option<&TickDecisions>,
        hints: &HashMap<AgentId, AgentHints>,
    ) -> TickDecisions {
        if agents.is_empty() {
            return TickDecisions::new();
        }

        log::debug!(
            "GORNA: Starting arbitration for {} agents. Phase={:?}, Multiplier={:.2}",
            agents.len(),
            context.mode,
            context.global_budget_multiplier
        );

        // ── 0. Health Check ──────────────────────────────────────────────
        let stalled_count = self.check_agent_health(agents);
        if stalled_count >= MAX_STALLED_AGENTS || report.death_spiral_detected {
            log::error!(
                "GORNA: Death spiral detected ({} stalled agents). \
                Forcing emergency LowPower on all agents.",
                stalled_count
            );
            return self.emergency_stop(agents);
        }

        // ── 1. Compute effective frame budget ────────────────────────────
        // Start from the analysis-suggested latency (accounts for phase, thermal, battery).
        let base_latency_ms = report.suggested_latency_ms;
        // Apply the global budget multiplier from the context.
        let effective_budget_ms = base_latency_ms * context.global_budget_multiplier;

        log::debug!(
            "GORNA: Effective frame budget: {:.2}ms (base={:.2}ms × multiplier={:.2})",
            effective_budget_ms,
            base_latency_ms,
            context.global_budget_multiplier
        );

        // ── 2. Negotiation Pass ──────────────────────────────────────────
        let mut negotiations: Vec<AgentNegotiation> = Vec::with_capacity(agents.len());

        for (i, agent_mutex) in agents.iter().enumerate() {
            let Some(mut agent) = try_lock_agent_with_timeout(agent_mutex, self.lock_timeout)
            else {
                log::warn!(
                    "GORNA: Failed to lock agent {} for negotiation (timeout). Skipping.",
                    i
                );
                continue;
            };
            let agent_id = agent.id();
            // A `Prioritize` hint overrides the default per-agent priority,
            // steering which agents the budget fit upgrades first.
            let priority = hints
                .get(&agent_id)
                .and_then(|h| h.priority)
                .unwrap_or_else(|| self.get_agent_priority(agent_id));
            let timing = agent.execution_timing();
            let current_strategy = agent.report_status().current_strategy;

            let request = NegotiationRequest {
                target_latency: Duration::from_secs_f64(effective_budget_ms as f64 / 1000.0),
                priority_weight: priority,
                constraints: ResourceConstraints {
                    must_run: self.is_critical_agent(agent_id),
                    ..Default::default()
                },
                current_mode: context.mode.clone(),
                agent_timing: timing,
            };

            let response = agent.negotiate(request);

            if response.strategies.is_empty() {
                log::warn!(
                    "GORNA: Agent {:?} returned no strategies. Skipping.",
                    agent_id
                );
                continue;
            }

            // Sort strategies by estimated time (ascending = cheapest first).
            let mut strategies = response.strategies;
            strategies.sort_by_key(|s| s.estimated_time);

            // ── 2b. Empirical calibration ────────────────────────────────
            // Anchor the agent's self-quoted estimates in measured reality:
            // when the DCC has an observed cost for this agent, rescale every
            // quoted option so the one matching the agent's *current* strategy
            // equals the measurement. Relative ordering between options is
            // preserved — only the absolute scale moves, so the fit reasons
            // about real milliseconds instead of static worst-case quotes.
            if let Some(&measured_ms) = measured_costs.get(&agent_id) {
                let quoted_ms = strategies
                    .iter()
                    .find(|s| s.id == current_strategy)
                    .map(|s| s.estimated_time.as_secs_f64() * 1000.0)
                    .filter(|ms| *ms > f64::EPSILON);
                if let Some(quoted_ms) = quoted_ms {
                    let factor = (measured_ms / quoted_ms)
                        .clamp(CALIBRATION_FACTOR_MIN, CALIBRATION_FACTOR_MAX);
                    for s in &mut strategies {
                        s.estimated_time =
                            Duration::from_secs_f64(s.estimated_time.as_secs_f64() * factor);
                    }
                    log::debug!(
                        "GORNA: Calibrated {:?} estimates ×{:.2} \
                         (measured {:.2}ms vs quoted {:.2}ms at {:?})",
                        agent_id,
                        factor,
                        measured_ms,
                        quoted_ms,
                        current_strategy
                    );
                }
            }

            negotiations.push(AgentNegotiation {
                agent_index: i,
                agent_id,
                priority,
                strategies,
            });
        }

        // ── 3. Global Budget Fitting ─────────────────────────────────────
        let max_vram = context
            .hardware
            .available_vram
            .or(context.hardware.total_vram);
        let allocations = self.fit_budgets(&negotiations, effective_budget_ms, max_vram);

        // ── 4. Issuance Pass ─────────────────────────────────────────────
        let mut issued: TickDecisions = Vec::with_capacity(allocations.len());
        for alloc in &allocations {
            let Some(mut agent) =
                try_lock_agent_with_timeout(&agents[alloc.agent_index], self.lock_timeout)
            else {
                log::warn!(
                    "GORNA: Failed to lock agent for budget issuance (index {}). Skipping.",
                    alloc.agent_index
                );
                continue;
            };

            let agent_id = agent.id();

            let strategy = if let Some(recorded) = replay {
                // Replay: issue the recorded strategy for this agent, bypassing
                // the fit and the AdaptationMode (deterministic reproduction).
                // Fall back to the fit if the recorded strategy isn't offered.
                recorded
                    .iter()
                    .find(|(id, _)| *id == agent_id)
                    .and_then(|(_, sid)| self.strategy_for(&negotiations, alloc.agent_index, *sid))
                    .unwrap_or_else(|| alloc.strategy.clone())
            } else {
                // Developer control: a `Manual` agent is pinned to its chosen
                // strategy — GORNA reports but never overrides it. `Learning`
                // (default) issues the negotiated fit.
                match self.adaptation_mode(agent_id) {
                    AdaptationMode::Manual(pinned) => self
                        .strategy_for(&negotiations, alloc.agent_index, pinned)
                        .unwrap_or_else(|| alloc.strategy.clone()),
                    AdaptationMode::Stable => {
                        // No opportunistic upgrade: keep the current strategy unless
                        // the fit is a downgrade (or the current one isn't offered).
                        let current = agent.report_status().current_strategy;
                        match self.strategy_for(&negotiations, alloc.agent_index, current) {
                            Some(cur) if alloc.strategy.estimated_time > cur.estimated_time => cur,
                            _ => alloc.strategy.clone(),
                        }
                    }
                    AdaptationMode::Bounded { min, max } => self.clamp_strategy(
                        &negotiations,
                        alloc.agent_index,
                        &alloc.strategy,
                        min,
                        max,
                    ),
                    AdaptationMode::Learning => alloc.strategy.clone(),
                }
            };

            // Developer `Cap` hint: clamp the issued strategy down to the time
            // ceiling. Skipped under replay (deterministic) and for a `Manual`
            // pin (an explicit strategy choice outranks a budget hint).
            let strategy = if replay.is_none()
                && !matches!(self.adaptation_mode(agent_id), AdaptationMode::Manual(_))
            {
                self.apply_cap(
                    &negotiations,
                    alloc.agent_index,
                    strategy,
                    hints.get(&agent_id),
                )
            } else {
                strategy
            };

            let budget = ResourceBudget {
                strategy_id: strategy.id,
                time_limit: strategy.estimated_time,
                memory_limit: Some(strategy.estimated_vram),
                extra_params: std::collections::HashMap::new(),
            };

            log::info!(
                "GORNA: Issuing budget to {:?} — strategy={:?}, time={:.2}ms, vram={}KB",
                agent_id,
                budget.strategy_id,
                budget.time_limit.as_secs_f64() * 1000.0,
                strategy.estimated_vram / 1024
            );

            agent.apply_budget(budget);
            issued.push((agent_id, strategy.id));
        }

        log::debug!(
            "GORNA: Arbitration complete. {} budgets issued.",
            issued.len()
        );
        issued
    }

    /// Polls all agents for health status and returns the count of stalled agents.
    fn check_agent_health(&self, agents: &[Arc<Mutex<dyn Agent>>]) -> usize {
        let mut stalled = 0;
        for (i, agent_mutex) in agents.iter().enumerate() {
            let Some(agent) = try_lock_agent_with_timeout(agent_mutex, self.lock_timeout) else {
                log::warn!(
                    "GORNA: Failed to lock agent {} for health check (timeout).",
                    i
                );
                continue;
            };
            let status = agent.report_status();
            if status.is_stalled {
                log::warn!(
                    "GORNA: Agent {:?} is STALLED. Health={:.2}, Message: {}",
                    status.agent_id,
                    status.health_score,
                    status.message
                );
                stalled += 1;
            } else if status.health_score < 0.5 {
                log::warn!(
                    "GORNA: Agent {:?} health degraded ({:.2}). Message: {}",
                    status.agent_id,
                    status.health_score,
                    status.message
                );
            }
        }
        stalled
    }

    /// Forces all agents to their lowest-cost strategy as an emergency measure.
    /// Returns the issued decisions (all `LowPower`) for recording.
    fn emergency_stop(&self, agents: &mut [Arc<Mutex<dyn Agent>>]) -> TickDecisions {
        let mut issued = TickDecisions::with_capacity(agents.len());
        for (i, agent_mutex) in agents.iter_mut().enumerate() {
            let Some(mut agent) = try_lock_agent_with_timeout(agent_mutex, self.lock_timeout)
            else {
                log::warn!(
                    "GORNA: Failed to lock agent {} for emergency stop (timeout).",
                    i
                );
                continue;
            };

            let budget = ResourceBudget {
                strategy_id: StrategyId::LowPower,
                time_limit: Duration::from_millis(2),
                memory_limit: None,
                extra_params: std::collections::HashMap::new(),
            };

            log::warn!("GORNA: Emergency LowPower issued to {:?}.", agent.id());
            agent.apply_budget(budget);
            issued.push((agent.id(), StrategyId::LowPower));
        }
        issued
    }

    /// Maps each negotiation to a wave id from `self.wave_plan`.
    ///
    /// Agents named together in a plan wave share its id (they run concurrently);
    /// any agent the plan does not mention — and every agent when the plan is
    /// empty — gets a fresh singleton id, so its cost is summed rather than
    /// folded into a wave `max`.
    fn assign_waves(&self, negotiations: &[AgentNegotiation]) -> Vec<usize> {
        let mut wave_of_id: HashMap<AgentId, usize> = HashMap::new();
        for (w, wave) in self.wave_plan.iter().enumerate() {
            for id in wave {
                wave_of_id.entry(*id).or_insert(w);
            }
        }
        let mut next_singleton = self.wave_plan.len();
        negotiations
            .iter()
            .map(|n| match wave_of_id.get(&n.agent_id) {
                Some(&w) => w,
                None => {
                    let w = next_singleton;
                    next_singleton += 1;
                    w
                }
            })
            .collect()
    }

    /// Returns the priority weight for an agent.
    ///
    /// Higher values indicate greater importance. The DCC uses these weights to
    /// decide which agents get upgraded first when budget is available.
    fn get_agent_priority(&self, id: AgentId) -> f32 {
        match id {
            AgentId::Renderer => 1.0,
            AgentId::ShadowRenderer => 1.0,
            AgentId::Physics => 1.0,
            AgentId::Ecs => 0.8,
            AgentId::Ui => 0.7,
            AgentId::Audio => 0.6,
            AgentId::Asset => 0.5,
            AgentId::Overlay => 0.4,
            AgentId::Skybox => 0.4,
            // Above rendering polish and below simulation: gameplay decides
            // what the frame is about, but a frame that skipped a tick of it
            // is recoverable in a way a missed physics step is not.
            AgentId::Script => 0.9,
        }
    }

    /// Returns `true` if the agent is considered critical
    /// and must always receive at least its minimum strategy.
    fn is_critical_agent(&self, id: AgentId) -> bool {
        matches!(
            id,
            AgentId::Renderer | AgentId::Physics | AgentId::Ecs | AgentId::Ui
        )
    }
}

#[cfg(test)]
mod tests;
