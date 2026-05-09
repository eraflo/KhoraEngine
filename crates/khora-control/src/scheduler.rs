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
use crate::substrate;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, EngineMode, ExecutionPhase};
use khora_core::control::gorna::{AgentId, ResourceBudget};
use khora_core::graph::topological_sort;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type AgentSlot = (
    Arc<Mutex<dyn khora_core::agent::Agent>>,
    AgentImportance,
    f32,
    Vec<AgentDependency>,
);

/// The hot-path execution scheduler.
pub struct ExecutionScheduler {
    registry: Arc<Mutex<AgentRegistry>>,
    budget_channel: BudgetChannel,
    plugins: Vec<EnginePlugin>,
    phase_order: Vec<ExecutionPhase>,
    context: Arc<std::sync::RwLock<Context>>,
    frame_start: Instant,
    frame_budget: Duration,
    /// Output deck retained across the tick boundary so the engine I/O
    /// layer can drain typed lane outputs (recorded GPU commands, draw
    /// lists, etc.) after the scheduler has finished.
    last_deck: OutputDeck,
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
            frame_start: Instant::now(),
            frame_budget: Duration::from_millis(16),
            last_deck: OutputDeck::new(),
        }
    }

    /// Mutable access to the last frame's [`OutputDeck`] — drained by the
    /// engine at the I/O boundary (e.g. GPU submit / present).
    pub fn deck_mut(&mut self) -> &mut OutputDeck {
        &mut self.last_deck
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
    pub fn run_frame(&mut self, world: &mut World, runtime: Arc<Runtime>) {
        // 1. Sync budgets from cold thread
        self.budget_channel.sync();
        self.frame_start = Instant::now();

        // 2. Build the per-frame completion map. The scheduler tracks
        //    completion internally — agents do not read it through the
        //    runtime containers.
        let agent_ids = self.registry.lock().unwrap().all_ids();
        let completion_map = Arc::new(AgentCompletionMap::new(&agent_ids));

        // 3. Read current mode
        let mode = {
            let ctx = self.context.read().unwrap();
            ctx.mode.clone()
        };

        // 4. Build the per-frame substrate: typed input bus and output deck.
        //    The deck is moved into the scheduler's `last_deck` slot at the
        //    end of the frame so the engine I/O layer can drain it.
        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        // 5. Snapshot agent budgets for this frame and run the Substrate
        //    Pass: every registered Flow projects its View into the bus,
        //    receiving the budget of its corresponding agent and access to
        //    the runtime containers.
        let budgets = self.snapshot_budgets(&agent_ids);
        substrate::run_flows(world, &mut bus, &budgets, &runtime);

        // 6. Clone phase order to avoid borrow conflicts
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();

        // 7. Execute each phase: plugins then agents (CLAD descent —
        //    agents invoke their lanes themselves through `Agent::execute`).
        for phase in phases {
            for plugin in &mut self.plugins {
                if plugin.wants_phase(phase) {
                    plugin.execute(phase, world);
                }
            }

            self.execute_agents_in_phase(
                phase,
                world,
                &runtime,
                &mode,
                &completion_map,
                &bus,
                &mut deck,
            );
        }

        // 8. Hand the populated deck off to the engine for the I/O boundary.
        self.last_deck = deck;
    }

    /// Drains all currently buffered budgets into a per-agent snapshot.
    fn snapshot_budgets(&self, agent_ids: &[AgentId]) -> HashMap<AgentId, ResourceBudget> {
        agent_ids
            .iter()
            .filter_map(|id| self.budget_channel.get(*id).map(|b| (*id, b)))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        world: &mut World,
        runtime: &Arc<Runtime>,
        mode: &EngineMode,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &LaneBus,
        deck: &mut OutputDeck,
    ) {
        // Collect agents for this phase and mode
        let agents = {
            let registry = self.registry.lock().unwrap();
            registry.collect_for_phase(phase, mode)
        };

        if agents.is_empty() {
            return;
        }

        let sorted = sort_agents(agents);
        self.execute_agents_sequential(sorted, world, runtime, completion_map, bus, deck);
    }

    fn execute_agents_sequential(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &LaneBus,
        deck: &mut OutputDeck,
    ) {
        for (agent, importance, _priority, dependencies) in agents {
            let agent_id = match agent.lock().ok().map(|a| a.id()) {
                Some(id) => id,
                None => continue,
            };

            // Skip optional if under budget pressure
            if importance == AgentImportance::Optional && self.is_under_budget_pressure() {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                continue;
            }

            // Skip if hard dependencies were skipped or are unmarked
            if !are_hard_dependencies_completed(&dependencies, completion_map) {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                continue;
            }

            // Build EngineContext and execute (CLAD descent: the agent
            // chooses and invokes its lane internally).
            let mut engine_ctx = EngineContext {
                world: Some(world as &mut dyn std::any::Any),
                runtime: Arc::clone(runtime),
                bus,
                deck,
            };

            if let Ok(mut a) = agent.lock() {
                a.execute(&mut engine_ctx);
            }
            completion_map.mark(agent_id, CompletionOutcome::Completed);
        }
    }

    /// Parallel execution path — Phase 4 stub.
    ///
    /// The intended structure (once enabled):
    /// ```ignore
    /// for (agent, _, _, deps) in agents {
    ///     let map = Arc::clone(completion_map);
    ///     tokio::spawn(async move {
    ///         for dep in deps.iter().filter(|d| d.kind == DependencyKind::Hard) {
    ///             match map.wait(dep.target).await {
    ///                 Some(CompletionOutcome::Completed) => {}
    ///                 _ => { map.mark(agent.id(), Skipped); return; }
    ///             }
    ///         }
    ///         agent.lock().execute(&mut ctx);
    ///         map.mark(agent.id(), Completed);
    ///     });
    /// }
    /// // join_all spawned handles before returning.
    /// ```
    #[allow(dead_code)]
    fn execute_agents_parallel(
        &self,
        _agents: Vec<AgentSlot>,
        _world: &mut World,
        _runtime: &Arc<Runtime>,
        _completion_map: &Arc<AgentCompletionMap>,
    ) {
        unimplemented!("parallel agent execution — not yet implemented");
    }

    fn is_under_budget_pressure(&self) -> bool {
        self.frame_start.elapsed() > self.frame_budget
    }
}

/// Orders the agents within a phase: Hard-dependency edges first (topological),
/// then importance + priority as a tiebreaker among ready-equal nodes.
///
/// On a cycle the topo sort fails; we log and fall back to the
/// importance/priority ordering — execution still happens, just not in DAG
/// order. Validating cycles at registration time is a future improvement.
fn sort_agents(mut agents: Vec<AgentSlot>) -> Vec<AgentSlot> {
    // Importance + priority tiebreaker.
    agents.sort_by(|a, b| {
        let imp_ord = a.1.cmp(&b.1);
        if imp_ord != std::cmp::Ordering::Equal {
            return imp_ord;
        }
        b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build (id -> index) map after the importance sort so the topo result
    // can be reassembled cheaply.
    let id_to_index: std::collections::HashMap<AgentId, usize> = agents
        .iter()
        .enumerate()
        .filter_map(|(idx, slot)| slot.0.lock().ok().map(|a| (a.id(), idx)))
        .collect();

    let nodes: Vec<AgentId> = id_to_index.keys().copied().collect();

    // Build hard-dep edges (parent = dep target, child = dependent agent)
    // — the dep target must run first.
    let mut edges: Vec<(AgentId, AgentId)> = Vec::new();
    for slot in &agents {
        let agent_id = match slot.0.lock().ok().map(|a| a.id()) {
            Some(id) => id,
            None => continue,
        };
        for dep in &slot.3 {
            if matches!(dep.kind, DependencyKind::Hard) && id_to_index.contains_key(&dep.target) {
                edges.push((dep.target, agent_id));
            }
        }
    }

    match topological_sort(nodes, edges) {
        Ok(order) => {
            // Reassemble agents in topo order. Equal-priority nodes inherit
            // the importance+priority ordering already applied above.
            let mut taken: Vec<Option<AgentSlot>> = agents.into_iter().map(Some).collect();
            order
                .into_iter()
                .filter_map(|id| {
                    id_to_index
                        .get(&id)
                        .copied()
                        .and_then(|idx| taken[idx].take())
                })
                .collect()
        }
        Err(_) => {
            log::error!(
                "Scheduler: cycle detected in Hard agent dependencies — falling back to importance/priority order"
            );
            agents
        }
    }
}

fn are_hard_dependencies_completed(
    dependencies: &[AgentDependency],
    completion_map: &AgentCompletionMap,
) -> bool {
    for dep in dependencies {
        if matches!(dep.kind, DependencyKind::Hard) {
            // Conditions are not yet evaluated in the scheduler — preserved
            // from the previous behaviour. Treat conditional deps as active.
            match completion_map.outcome(dep.target) {
                Some(CompletionOutcome::Completed) => {}
                Some(CompletionOutcome::Skipped) | None => return false,
            }
        }
    }
    true
}
