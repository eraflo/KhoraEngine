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
use crate::context::{Context, ExecutionPhase};
use khora_core::agent::Agent;
use khora_core::control::gorna::{
    AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId, StrategyOption,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_STALLED_AGENTS: usize = 2;

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
        Self { lock_timeout }
    }
    /// Performs a full GORNA arbitration round.
    ///
    /// # Arguments
    /// - `context`: The current DCC situational model (phase, hardware, multiplier).
    /// - `report`: The analysis report from the `HeuristicEngine`.
    /// - `agents`: The registered ISA agents.
    pub fn arbitrate(
        &self,
        context: &Context,
        report: &AnalysisReport,
        agents: &mut [Arc<Mutex<dyn Agent>>],
    ) {
        if agents.is_empty() {
            return;
        }

        log::debug!(
            "GORNA: Starting arbitration for {} agents. Phase={:?}, Multiplier={:.2}",
            agents.len(),
            context.phase,
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
            self.emergency_stop(agents);
            return;
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
            let priority = self.get_agent_priority(agent_id, context.phase);

            let request = NegotiationRequest {
                target_latency: Duration::from_secs_f64(effective_budget_ms as f64 / 1000.0),
                priority_weight: priority,
                constraints: ResourceConstraints {
                    must_run: self.is_critical_agent(agent_id, context.phase),
                    ..Default::default()
                },
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
            strategies.sort_by(|a, b| a.estimated_time.cmp(&b.estimated_time));

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

            let budget = ResourceBudget {
                strategy_id: alloc.strategy.id,
                time_limit: alloc.strategy.estimated_time,
                memory_limit: Some(alloc.strategy.estimated_vram),
                extra_params: std::collections::HashMap::new(),
            };

            log::info!(
                "GORNA: Issuing budget to {:?} — strategy={:?}, time={:.2}ms, vram={}KB",
                agent.id(),
                budget.strategy_id,
                budget.time_limit.as_secs_f64() * 1000.0,
                alloc.strategy.estimated_vram / 1024
            );

            agent.apply_budget(budget);
        }

        log::debug!(
            "GORNA: Arbitration complete. {} budgets issued.",
            allocations.len()
        );
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
    fn emergency_stop(&self, agents: &mut [Arc<Mutex<dyn Agent>>]) {
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
        }
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

    /// Returns the priority weight for an agent given the current execution phase.
    ///
    /// Higher values indicate greater importance. The DCC uses these weights to
    /// decide which agents get upgraded first when budget is available.
    fn get_agent_priority(&self, id: AgentId, phase: ExecutionPhase) -> f32 {
        match phase {
            ExecutionPhase::Boot => match id {
                AgentId::Asset => 1.0,
                _ => 0.3,
            },
            ExecutionPhase::Menu => match id {
                AgentId::Renderer => 0.6,
                AgentId::Asset => 1.0,
                AgentId::Audio => 0.8,
                _ => 0.3,
            },
            ExecutionPhase::Simulation => match id {
                AgentId::Renderer => 1.0,
                AgentId::Physics => 1.0,
                AgentId::Ecs => 0.8,
                AgentId::Audio => 0.6,
                AgentId::Asset => 0.5,
            },
            ExecutionPhase::Background => 0.1, // Everything minimal
        }
    }

    /// Returns `true` if the agent is considered critical for the current phase
    /// and must always receive at least its minimum strategy.
    fn is_critical_agent(&self, id: AgentId, phase: ExecutionPhase) -> bool {
        match phase {
            ExecutionPhase::Boot => matches!(id, AgentId::Asset),
            ExecutionPhase::Menu => matches!(id, AgentId::Renderer),
            ExecutionPhase::Simulation => {
                matches!(id, AgentId::Renderer | AgentId::Physics | AgentId::Ecs)
            }
            ExecutionPhase::Background => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AnalysisReport;
    use crate::context::Context;
    use khora_core::agent::Agent;
    use khora_core::control::gorna::{
        AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
        StrategyOption,
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
            }
        }

        fn apply_budget(&mut self, budget: ResourceBudget) {
            self.applied_budget = Some(budget);
        }

        fn update(&mut self, _context: &mut EngineContext<'_>) {}

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

        fn execute(&mut self) {}

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
            phase: ExecutionPhase::Simulation,
            global_budget_multiplier: 1.0,
            ..Default::default()
        }
    }

    // ── Tests ────────────────────────────────────────────────────────

    fn create_arbitrator() -> GornaArbitrator {
        GornaArbitrator::new(Duration::from_millis(100))
    }

    #[test]
    fn test_arbitrate_single_agent_gets_best_strategy() {
        let arbitrator = create_arbitrator();
        let ctx = simulation_ctx();
        let report = normal_report();
        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents);

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

        arbitrator.arbitrate(&ctx, &report, &mut agents);

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
        ctx.refresh_budget_multiplier(); // 0.6

        let mut report = normal_report();
        report.suggested_latency_ms = 33.33; // Heuristic suggestion for throttling

        let agent = MockAgent::new(AgentId::Renderer);
        let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

        arbitrator.arbitrate(&ctx, &report, &mut agents);

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

        arbitrator.arbitrate(&ctx, &report, &mut agents);

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

        arbitrator.arbitrate(&ctx, &report, &mut agents);

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
        arbitrator.arbitrate(&ctx, &report, &mut agents);
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

        arbitrator.arbitrate(&ctx, &tight_report, &mut agents);

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
    fn test_background_phase_minimal_priority() {
        let arbitrator = create_arbitrator();
        assert!(arbitrator.get_agent_priority(AgentId::Renderer, ExecutionPhase::Background) < 0.2);
        assert!(arbitrator.get_agent_priority(AgentId::Physics, ExecutionPhase::Background) < 0.2);
    }

    #[test]
    fn test_simulation_critical_agents() {
        let arbitrator = create_arbitrator();
        assert!(arbitrator.is_critical_agent(AgentId::Renderer, ExecutionPhase::Simulation));
        assert!(arbitrator.is_critical_agent(AgentId::Physics, ExecutionPhase::Simulation));
        assert!(arbitrator.is_critical_agent(AgentId::Ecs, ExecutionPhase::Simulation));
        assert!(!arbitrator.is_critical_agent(AgentId::Audio, ExecutionPhase::Simulation));
    }
}
