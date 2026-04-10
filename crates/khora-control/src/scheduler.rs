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

//! The ExecutionScheduler — hot-path orchestrator for the SAA frame loop.

use crate::budget_channel::BudgetChannel;
use crate::context::Context;
use crate::plugin::EnginePlugin;
use crate::registry::AgentRegistry;
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{EngineMode, ExecutionPhase};
use khora_core::control::gorna::AgentId;
use khora_core::EngineContext;
use khora_data::ecs::World;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The hot-path execution scheduler.
pub struct ExecutionScheduler {
    registry: Arc<Mutex<AgentRegistry>>,
    budget_channel: BudgetChannel,
    plugins: Vec<EnginePlugin>,
    phase_order: Vec<ExecutionPhase>,
    context: Arc<std::sync::RwLock<Context>>,
    executed_agents_this_frame: HashSet<AgentId>,
    frame_start: Instant,
    frame_budget: Duration,
}

impl ExecutionScheduler {
    /// Creates a new scheduler.
    pub fn new(
        registry: Arc<Mutex<AgentRegistry>>,
        context: Arc<std::sync::RwLock<Context>>,
        agent_ids: &[AgentId],
    ) -> Self {
        Self {
            registry,
            budget_channel: BudgetChannel::new(agent_ids),
            plugins: Vec::new(),
            phase_order: ExecutionPhase::DEFAULT_ORDER.to_vec(),
            context,
            executed_agents_this_frame: HashSet::new(),
            frame_start: Instant::now(),
            frame_budget: Duration::from_millis(16),
        }
    }

    /// Returns a reference to the budget channel for the DCC to send budgets.
    pub fn budget_channel(&self) -> &BudgetChannel {
        &self.budget_channel
    }

    /// Registers an engine plugin.
    pub fn register_plugin(&mut self, plugin: EnginePlugin) {
        log::info!("Scheduler: Registered plugin '{}'", plugin.name());
        self.plugins.push(plugin);
    }

    /// Sets the phase execution order.
    pub fn set_phase_order(&mut self, order: &[ExecutionPhase]) {
        self.phase_order = order.to_vec();
    }

    /// Inserts a phase after an existing phase.
    pub fn insert_after(&mut self, existing: ExecutionPhase, new: ExecutionPhase) {
        if let Some(pos) = self.phase_order.iter().position(|p| *p == existing) {
            self.phase_order.insert(pos + 1, new);
        }
    }

    /// Inserts a phase before an existing phase.
    pub fn insert_before(&mut self, existing: ExecutionPhase, new: ExecutionPhase) {
        if let Some(pos) = self.phase_order.iter().position(|p| *p == existing) {
            self.phase_order.insert(pos, new);
        }
    }

    /// Removes a phase from the order.
    pub fn remove_phase(&mut self, phase: ExecutionPhase) {
        self.phase_order.retain(|p| *p != phase);
    }

    /// Executes the complete frame cycle.
    ///
    /// This is called every frame by the engine loop.
    pub fn run_frame(&mut self, world: &mut World, services: Arc<khora_core::ServiceRegistry>) {
        // 1. Sync budgets from cold thread
        self.budget_channel.sync();
        self.executed_agents_this_frame.clear();
        self.frame_start = Instant::now();

        // 2. Read current mode
        let mode = {
            let ctx = self.context.read().unwrap();
            ctx.mode.clone()
        };

        // 3. Clone phase order to avoid borrow conflicts
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();

        // 4. Execute each phase
        for phase in phases {
            // Plugins
            for plugin in &mut self.plugins {
                if plugin.wants_phase(phase) {
                    plugin.execute(phase, world);
                }
            }

            // Agents
            self.execute_agents_in_phase(phase, world, &services, &mode);
        }
    }

    fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        world: &mut World,
        services: &Arc<khora_core::ServiceRegistry>,
        mode: &EngineMode,
    ) {
        // Collect agents for this phase and mode
        let agents = {
            let registry = self.registry.lock().unwrap();
            registry.collect_for_phase(phase, mode)
        };

        if agents.is_empty() {
            return;
        }

        // Sort by importance (Critical > Important > Optional) then priority (desc)
        let mut sorted: Vec<_> = agents.into_iter().collect();
        sorted.sort_by(|a, b| {
            let imp_ord = a.1.cmp(&b.1);
            if imp_ord != std::cmp::Ordering::Equal {
                return imp_ord;
            }
            b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Execute
        for (agent, importance, _priority, dependencies) in sorted {
            // Skip optional if under budget pressure
            if importance == AgentImportance::Optional && self.is_under_budget_pressure() {
                continue;
            }

            // Skip if hard dependencies not met
            if !self.are_hard_dependencies_satisfied(&dependencies) {
                continue;
            }

            // Build EngineContext and execute
            let mut engine_ctx = EngineContext {
                world: Some(world as &mut dyn std::any::Any),
                services: Arc::clone(services),
            };

            if let Ok(mut a) = agent.lock() {
                let id = a.id();
                a.execute(&mut engine_ctx);
                self.executed_agents_this_frame.insert(id);
            }
        }
    }

    fn is_under_budget_pressure(&self) -> bool {
        self.frame_start.elapsed() > self.frame_budget
    }

    fn are_hard_dependencies_satisfied(
        &self,
        dependencies: &[khora_core::agent::AgentDependency],
    ) -> bool {
        for dep in dependencies {
            if matches!(dep.kind, DependencyKind::Hard) {
                if let Some(condition) = &dep.condition {
                    if !self.is_condition_met(condition) {
                        continue;
                    }
                }
                if !self.executed_agents_this_frame.contains(&dep.target) {
                    return false;
                }
            }
        }
        true
    }

    fn is_condition_met(&self, _condition: &khora_core::agent::DependencyCondition) -> bool {
        true // TODO: implement condition checking
    }
}
