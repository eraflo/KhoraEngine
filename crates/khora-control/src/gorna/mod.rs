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
        }
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
                self.apply_cap(&negotiations, alloc.agent_index, strategy, hints.get(&agent_id))
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

    /// Runs the global budget fitting algorithm.
    ///
    /// Strategy: Priority-weighted greedy allocation.
    /// 1. Sort agents by priority (highest first).
    /// 2. Try to give each agent its most expensive strategy that fits.
    /// 3. If the total exceeds the budget, downgrade lower-priority agents first.
    /// 4. Respect VRAM constraints if specified.
    fn fit_budgets(
        &self,
        negotiations: &[AgentNegotiation],
        total_budget_ms: f32,
        max_vram_bytes: Option<u64>,
    ) -> Vec<AgentAllocation> {
        if negotiations.is_empty() {
            return Vec::new();
        }

        let mut sorted_indices: Vec<usize> = (0..negotiations.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            negotiations[b]
                .priority
                .partial_cmp(&negotiations[a].priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut allocations: Vec<AgentAllocation> = negotiations
            .iter()
            .map(|n| AgentAllocation {
                agent_index: n.agent_index,
                strategy: n.strategies[0].clone(),
            })
            .collect();

        let total_min_ms: f32 = allocations
            .iter()
            .map(|a| a.strategy.estimated_time.as_secs_f32() * 1000.0)
            .sum();

        let total_min_vram: u64 = allocations.iter().map(|a| a.strategy.estimated_vram).sum();

        if total_min_ms > total_budget_ms {
            log::warn!(
                "GORNA: Even minimum strategies ({:.2}ms) exceed budget ({:.2}ms). \
                All agents at LowPower.",
                total_min_ms,
                total_budget_ms
            );
            return allocations;
        }

        if let Some(max_vram) = max_vram_bytes {
            if total_min_vram > max_vram {
                log::warn!(
                    "GORNA: Even minimum strategies VRAM ({:.2}MB) exceeds budget ({:.2}MB).",
                    total_min_vram as f64 / (1024.0 * 1024.0),
                    max_vram as f64 / (1024.0 * 1024.0)
                );
            }
        }

        let mut remaining_ms = total_budget_ms - total_min_ms;
        let mut current_vram = total_min_vram;

        for &idx in &sorted_indices {
            let negotiation = &negotiations[idx];
            let current_cost_ms = allocations[idx].strategy.estimated_time.as_secs_f32() * 1000.0;
            let current_vram_cost = allocations[idx].strategy.estimated_vram;

            let mut best_upgrade: Option<&StrategyOption> = None;
            for strategy in negotiation.strategies.iter().rev() {
                let cost_ms = strategy.estimated_time.as_secs_f32() * 1000.0;
                let delta_ms = cost_ms - current_cost_ms;
                let delta_vram = strategy.estimated_vram.saturating_sub(current_vram_cost);

                let time_fits = delta_ms <= remaining_ms;
                let vram_fits = max_vram_bytes
                    .map(|max| current_vram + delta_vram <= max)
                    .unwrap_or(true);

                if time_fits && vram_fits {
                    best_upgrade = Some(strategy);
                    break;
                }
            }

            if let Some(upgrade) = best_upgrade {
                let old_cost = current_cost_ms;
                let new_cost = upgrade.estimated_time.as_secs_f32() * 1000.0;
                let delta_vram = upgrade.estimated_vram.saturating_sub(current_vram_cost);

                remaining_ms -= new_cost - old_cost;
                current_vram += delta_vram;
                allocations[idx].strategy = upgrade.clone();

                log::trace!(
                    "GORNA: Upgraded {:?} from {:.2}ms to {:.2}ms (remaining={:.2}ms, vram={:.2}MB)",
                    negotiation.agent_id,
                    old_cost,
                    new_cost,
                    remaining_ms,
                    current_vram as f64 / (1024.0 * 1024.0)
                );
            }
        }

        if let Some(max_vram) = max_vram_bytes {
            let total_vram: u64 = allocations.iter().map(|a| a.strategy.estimated_vram).sum();
            log::debug!(
                "GORNA: Total VRAM allocated: {:.2}MB / {:.2}MB",
                total_vram as f64 / (1024.0 * 1024.0),
                max_vram as f64 / (1024.0 * 1024.0)
            );
        }

        allocations
    }

    /// Clamps `fitted` into the `[min, max]` strategy range by picking, from the
    /// agent's offered strategies within range, the one nearest the fit. Honours
    /// `Bounded` mode.
    fn clamp_strategy(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        fitted: &StrategyOption,
        min: StrategyId,
        max: StrategyId,
    ) -> StrategyOption {
        let (lo, hi) = (strategy_rank(min), strategy_rank(max));
        let fr = strategy_rank(fitted.id);
        if fr >= lo && fr <= hi {
            return fitted.clone();
        }
        let Some(n) = negotiations.iter().find(|n| n.agent_index == agent_index) else {
            return fitted.clone();
        };
        let mut best: Option<&StrategyOption> = None;
        for s in &n.strategies {
            let sr = strategy_rank(s.id);
            if sr < lo || sr > hi {
                continue;
            }
            let closer = match best {
                None => true,
                Some(b) => {
                    (sr as i32 - fr as i32).abs() < (strategy_rank(b.id) as i32 - fr as i32).abs()
                }
            };
            if closer {
                best = Some(s);
            }
        }
        best.cloned().unwrap_or_else(|| fitted.clone())
    }

    /// Clamps `fitted` down to a developer `Cap` hint: if the fitted strategy's
    /// estimated cost exceeds `hints.cap_ms`, returns the most expensive offered
    /// strategy still within the ceiling (or the cheapest, if even that
    /// exceeds it). No cap, or already within it → `fitted` unchanged.
    fn apply_cap(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        fitted: StrategyOption,
        hints: Option<&AgentHints>,
    ) -> StrategyOption {
        let Some(max_ms) = hints.and_then(|h| h.cap_ms) else {
            return fitted;
        };
        if fitted.estimated_time.as_secs_f32() * 1000.0 <= max_ms {
            return fitted;
        }
        let Some(n) = negotiations.iter().find(|n| n.agent_index == agent_index) else {
            return fitted;
        };
        // `strategies` is sorted ascending by estimated_time, so the last option
        // within the ceiling is the richest one that honours the cap; if none
        // fit, fall back to the cheapest (index 0) — the closest we can get.
        let mut chosen = &n.strategies[0];
        for s in &n.strategies {
            if s.estimated_time.as_secs_f32() * 1000.0 <= max_ms {
                chosen = s;
            } else {
                break;
            }
        }
        chosen.clone()
    }

    /// Finds the negotiated [`StrategyOption`] with `id` for the agent at
    /// `agent_index`, if that agent offered it. Used to honour `Manual` mode.
    fn strategy_for(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        id: StrategyId,
    ) -> Option<StrategyOption> {
        negotiations
            .iter()
            .find(|n| n.agent_index == agent_index)
            .and_then(|n| n.strategies.iter().find(|s| s.id == id).cloned())
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
mod tests {
    use super::*;
    use crate::analysis::AnalysisReport;
    use crate::context::Context;
    use crate::EngineMode;
    use khora_core::agent::Agent;
    use khora_core::control::gorna::{
        AdaptationMode, AgentHints, AgentId, AgentStatus, EngineHint, NegotiationRequest,
        NegotiationResponse, ResourceBudget, StrategyId, StrategyOption, TickDecisions,
    };
    use khora_core::EngineContext;

    // ── Mock Agent ───────────────────────────────────────────────────

    struct MockAgent {
        id: AgentId,
        applied_budget: Option<ResourceBudget>,
        is_stalled: bool,
        health: f32,
    }

    impl MockAgent {
        fn new(id: AgentId) -> Self {
            Self {
                id,
                applied_budget: None,
                is_stalled: false,
                health: 1.0,
            }
        }

        fn stalled(id: AgentId) -> Self {
            Self {
                id,
                applied_budget: None,
                is_stalled: true,
                health: 0.0,
            }
        }
    }

    impl Agent for MockAgent {
        fn id(&self) -> AgentId {
            self.id
        }

        fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
            NegotiationResponse {
                strategies: vec![
                    StrategyOption {
                        id: StrategyId::LowPower,
                        estimated_time: Duration::from_millis(2),
                        estimated_vram: 1024,
                    },
                    StrategyOption {
                        id: StrategyId::Balanced,
                        estimated_time: Duration::from_millis(8),
                        estimated_vram: 10 * 1024 * 1024,
                    },
                    StrategyOption {
                        id: StrategyId::HighPerformance,
                        estimated_time: Duration::from_millis(14),
                        estimated_vram: 20 * 1024 * 1024,
                    },
                ],
                timing_adjustment: None,
            }
        }

        fn apply_budget(&mut self, budget: ResourceBudget) {
            self.applied_budget = Some(budget);
        }

        fn report_status(&self) -> AgentStatus {
            AgentStatus {
                agent_id: self.id,
                current_strategy: self
                    .applied_budget
                    .as_ref()
                    .map(|b| b.strategy_id)
                    .unwrap_or(StrategyId::Balanced),
                health_score: self.health,
                is_stalled: self.is_stalled,
                message: String::new(),
            }
        }

        fn execute(&mut self, _context: &mut EngineContext<'_>) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn normal_report() -> AnalysisReport {
        AnalysisReport {
            needs_negotiation: true,
            suggested_latency_ms: 16.66,
            death_spiral_detected: false,
            alerts: Vec::new(),
        }
    }

    fn simulation_ctx() -> Context {
        Context {
            mode: EngineMode::Playing,
            global_budget_multiplier: 1.0,
            ..Default::default()
        }
    }

    // ── Tests ────────────────────────────────────────────────────────

    fn create_arbitrator() -> GornaArbitrator {
        GornaArbitrator::new(Duration::from_millis(100))
    }

    #[test]
    fn test_measured_costs_calibrate_fit_downward() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        // The agent quotes Balanced at 8ms but actually measured 24ms (×3).
        // Calibration rescales the options to 6/24/42ms, so within the 16.66ms
        // budget only LowPower fits — the fit must downgrade instead of
        // trusting the optimistic quote (which would have picked HighPerformance).
        let measured: HashMap<AgentId, f64> = [(AgentId::Renderer, 24.0)].into();
        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &measured, None, &HashMap::new());

        assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
    }

    #[test]
    fn test_calibration_factor_is_clamped() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        // A pathological measurement (800ms vs the 8ms quote = ×100) is clamped
        // to ×4: options become 8/32/56ms. Even the cheapest exceeds nothing —
        // LowPower (8ms) still fits the 16.66ms budget, but no upgrade does.
        let measured: HashMap<AgentId, f64> = [(AgentId::Renderer, 800.0)].into();
        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &measured, None, &HashMap::new());

        assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
    }

    #[test]
    fn test_arbitrate_single_agent_gets_best_strategy() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        let budget = mock
            .applied_budget
            .as_ref()
            .expect("Budget should be applied");
        // With 16.66ms total budget and a single agent, it should get HighPerformance (14ms)
        assert_eq!(budget.strategy_id, StrategyId::HighPerformance);
    }

    #[test]
    fn test_manual_mode_pins_strategy_against_budget() {
        let mut arbitrator = create_arbitrator();
        arbitrator.set_adaptation_mode(
            AgentId::Renderer,
            AdaptationMode::Manual(StrategyId::LowPower),
        );

        let ctx = simulation_ctx();
        let report = normal_report();
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        let budget = mock
            .applied_budget
            .as_ref()
            .expect("Budget should be applied");
        // The same 16.66ms budget yields HighPerformance under `Learning` (test
        // above). `Manual` pins the developer's choice instead: LowPower.
        assert_eq!(budget.strategy_id, StrategyId::LowPower);
    }

    #[test]
    fn test_stable_mode_blocks_opportunistic_upgrade() {
        let mut arbitrator = create_arbitrator();
        arbitrator.set_adaptation_mode(AgentId::Renderer, AdaptationMode::Stable);

        let ctx = simulation_ctx();
        let report = normal_report();
        // MockAgent reports `Balanced` as its current strategy until a budget is
        // applied. With a 16.66ms budget the fit would upgrade to HighPerformance,
        // but `Stable` forbids opportunistic upgrades — it stays at Balanced.
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::Balanced
        );
    }

    #[test]
    fn test_bounded_mode_clamps_to_max() {
        let mut arbitrator = create_arbitrator();
        arbitrator.set_adaptation_mode(
            AgentId::Renderer,
            AdaptationMode::Bounded {
                min: StrategyId::LowPower,
                max: StrategyId::Balanced,
            },
        );

        let ctx = simulation_ctx();
        let report = normal_report();
        // Fit would pick HighPerformance; bounds cap it at Balanced.
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::Balanced
        );
    }

    #[test]
    fn test_arbitrate_respects_global_budget() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();

        // Two agents: Renderer (priority 1.0) and Physics (priority 1.0)
        // Total budget: 16.66ms
        // Each agent offers: LowPower(2ms), Balanced(8ms), HighPerformance(14ms)
        // Both can't be HighPerformance (14+14=28ms > 16.66ms)
        // With priority-based allocation, they should get strategies that fit.
        let renderer = MockAgent::new(AgentId::Renderer);
        let physics = MockAgent::new(AgentId::Physics);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
            Arc::new(Mutex::new(renderer)),
            Arc::new(Mutex::new(physics)),
        ];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        // Both should have received budgets
        for agent_mutex in &agents {
            let lock = agent_mutex.lock().unwrap();
            let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
            assert!(mock.applied_budget.is_some());
        }

        // Total cost should not exceed 16.66ms
        let total_cost_ms: f64 = agents
            .iter()
            .map(|a| {
                let lock = a.lock().unwrap();
                let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
                mock.applied_budget
                    .as_ref()
                    .unwrap()
                    .time_limit
                    .as_secs_f64()
                    * 1000.0
            })
            .sum();
        assert!(
            total_cost_ms <= 16.66 + 0.1,
            "Total cost {:.2}ms exceeds budget 16.66ms",
            total_cost_ms
        );
    }

    #[test]
    fn test_arbitrate_thermal_reduces_budget() {
        let arbitrator = create_arbitrator();
        let mut ctx = simulation_ctx();
        ctx.hardware.thermal = khora_core::platform::ThermalStatus::Throttling;
        // The PID owns the multiplier in the live loop; here we pin it directly
        // to exercise the lever `arbitrate` consumes.
        ctx.global_budget_multiplier = 0.6;

        let mut report = normal_report();
        report.suggested_latency_ms = 33.33; // Heuristic suggestion for throttling

        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        let budget = mock
            .applied_budget
            .as_ref()
            .expect("Budget should be applied");
        // Effective budget: 33.33 * 0.6 = ~20ms. Agent can easily get HighPerformance (14ms).
        assert_eq!(budget.strategy_id, StrategyId::HighPerformance);
    }

    #[test]
    fn test_emergency_stop_on_death_spiral() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let mut report = normal_report();
        report.death_spiral_detected = true;

        let renderer = MockAgent::new(AgentId::Renderer);
        let physics = MockAgent::new(AgentId::Physics);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
            Arc::new(Mutex::new(renderer)),
            Arc::new(Mutex::new(physics)),
        ];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        // Both agents should be forced to LowPower
        for agent_mutex in &agents {
            let lock = agent_mutex.lock().unwrap();
            let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
            let budget = mock
                .applied_budget
                .as_ref()
                .expect("Budget should be applied");
            assert_eq!(budget.strategy_id, StrategyId::LowPower);
        }
    }

    #[test]
    fn test_emergency_stop_on_stalled_agents() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();

        // Two stalled agents should trigger emergency stop
        let stalled1 = MockAgent::stalled(AgentId::Renderer);
        let stalled2 = MockAgent::stalled(AgentId::Physics);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
            Arc::new(Mutex::new(stalled1)),
            Arc::new(Mutex::new(stalled2)),
        ];

        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        // Both should be forced to LowPower
        for agent_mutex in &agents {
            let lock = agent_mutex.lock().unwrap();
            let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
            let budget = mock
                .applied_budget
                .as_ref()
                .expect("Budget should be applied");
            assert_eq!(budget.strategy_id, StrategyId::LowPower);
        }
    }

    #[test]
    fn test_arbitrate_empty_agents() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![];

        // Should not panic
        arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());
    }

    #[test]
    fn test_priority_order_renderer_before_asset_in_simulation() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();

        // Tight budget: only 10ms total. Renderer (priority 1.0) should be
        // upgraded before Asset (priority 0.5).
        let mut tight_report = report;
        tight_report.suggested_latency_ms = 10.0;

        let renderer = MockAgent::new(AgentId::Renderer);
        let asset = MockAgent::new(AgentId::Asset);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(renderer)), Arc::new(Mutex::new(asset))];

        arbitrator.arbitrate(&ctx, &tight_report, &mut agents, &HashMap::new(), None, &HashMap::new());

        // With 10ms total: both minimum = 2+2=4ms, remaining=6ms.
        // Renderer (priority 1.0) should be upgraded first: +6ms → Balanced (8ms).
        // Asset (priority 0.5) stays at LowPower (2ms). Total: 8+2=10ms ≤ 10ms.
        let renderer_lock = agents[0].lock().unwrap();
        let renderer_mock =
            unsafe { &*((&*renderer_lock as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            renderer_mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::Balanced
        );
    }

    #[test]
    fn test_hint_cap_clamps_issued_strategy() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

        // Ample budget would fit HighPerformance (14ms), but a 5ms Cap leaves
        // only LowPower (2ms) within the ceiling — Balanced (8ms) exceeds it.
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::Renderer,
            AgentHints {
                cap_ms: Some(5.0),
                priority: None,
            },
        )]
        .into();
        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
        assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
    }

    #[test]
    fn test_hint_cap_allows_richest_within_ceiling() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

        // A 10ms Cap admits Balanced (8ms) but not HighPerformance (14ms).
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::Renderer,
            AgentHints {
                cap_ms: Some(10.0),
                priority: None,
            },
        )]
        .into();
        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
        assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::Balanced)]);
    }

    #[test]
    fn test_hint_prioritize_reorders_budget_fit() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut tight_report = report;
        tight_report.suggested_latency_ms = 10.0;

        // Default priorities upgrade Renderer (1.0) before Asset (0.5). A
        // Prioritize hint lifting Asset above Renderer flips the fit: Asset
        // takes the single available upgrade to Balanced, Renderer stays LowPower.
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
            Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer))),
            Arc::new(Mutex::new(MockAgent::new(AgentId::Asset))),
        ];
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::Asset,
            AgentHints {
                cap_ms: None,
                priority: Some(2.0),
            },
        )]
        .into();

        arbitrator.arbitrate(&ctx, &tight_report, &mut agents, &HashMap::new(), None, &hints);

        let renderer = agents[0].lock().unwrap();
        let renderer_mock = unsafe { &*((&*renderer as *const dyn Agent) as *const MockAgent) };
        let asset = agents[1].lock().unwrap();
        let asset_mock = unsafe { &*((&*asset as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            asset_mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::Balanced,
            "the prioritized agent takes the upgrade"
        );
        assert_eq!(
            renderer_mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::LowPower,
            "the deprioritized agent is downgraded"
        );
    }

    #[test]
    fn test_hint_targets_only_its_agent() {
        // A Cap on a DIFFERENT agent must not touch Renderer: with ample budget
        // and no Renderer hint, it still reaches HighPerformance (regression-0).
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::Audio,
            AgentHints {
                cap_ms: Some(1.0),
                priority: None,
            },
        )]
        .into();
        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
        assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::HighPerformance)]);
    }

    #[test]
    fn test_arbitrate_returns_issued_decisions() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());
        // Single agent, ample budget → HighPerformance, reported back as issued.
        assert_eq!(
            issued,
            vec![(AgentId::Renderer, StrategyId::HighPerformance)]
        );
    }

    #[test]
    fn test_replay_overrides_fit_with_recorded_decision() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
            vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

        // A recorded decision of LowPower must be issued verbatim, even though
        // the live fit (ample budget) would pick HighPerformance.
        let recorded: TickDecisions = vec![(AgentId::Renderer, StrategyId::LowPower)];
        let replayed = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), Some(&recorded), &HashMap::new());
        assert_eq!(replayed, vec![(AgentId::Renderer, StrategyId::LowPower)]);

        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::LowPower
        );
    }

    #[test]
    fn test_vram_budget_caps_upgrade() {
        let arbitrator = create_arbitrator();
        let mut ctx = simulation_ctx();
        let report = normal_report();

        // Ample time budget (16.66ms) would let a lone agent reach
        // HighPerformance (14ms). But HighPerformance costs 20MB of VRAM,
        // Balanced 10MB. Cap available VRAM at 15MB: the fit must stop at
        // Balanced — the time budget is not the binding constraint here.
        ctx.hardware.available_vram = Some(15 * 1024 * 1024);

        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &HashMap::new());

        assert_eq!(
            issued,
            vec![(AgentId::Renderer, StrategyId::Balanced)],
            "VRAM ceiling must block the HighPerformance upgrade"
        );
        let lock = agents[0].lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        assert_eq!(
            mock.applied_budget.as_ref().unwrap().strategy_id,
            StrategyId::Balanced
        );
    }

    #[test]
    fn test_agent_priorities() {
        let arbitrator = create_arbitrator();
        assert!(arbitrator.get_agent_priority(AgentId::Renderer) >= 0.9);
        assert!(arbitrator.get_agent_priority(AgentId::Physics) >= 0.9);
        assert!(arbitrator.get_agent_priority(AgentId::Ui) >= 0.5);
        assert!(arbitrator.get_agent_priority(AgentId::Audio) >= 0.5);
    }

    #[test]
    fn test_critical_agents() {
        let arbitrator = create_arbitrator();
        assert!(arbitrator.is_critical_agent(AgentId::Renderer));
        assert!(arbitrator.is_critical_agent(AgentId::Physics));
        assert!(arbitrator.is_critical_agent(AgentId::Ecs));
        assert!(arbitrator.is_critical_agent(AgentId::Ui));
        assert!(!arbitrator.is_critical_agent(AgentId::Audio));
    }
}
