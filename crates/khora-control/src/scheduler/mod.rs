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

use crate::agent_registry::AgentRegistry;
use crate::budget_channel::BudgetChannel;
use crate::dcc_context::Context;
use crate::plugin::EnginePlugin;
use crate::worker_pool::WorkerPool;
use crossbeam_channel::Sender;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, EngineMode, ExecutionPhase};
use khora_core::graph::topological_sort;
use khora_core::lane::OutputDeck;
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use waves::{build_wave_metas, partition_waves};

mod execution;
mod frame;
mod sim_steps;
mod waves;

#[cfg(test)]
mod tests;

/// How often (in frames) the scheduler samples per-component access stats for
/// the layout advisor. Per-frame would flood the telemetry channel for data
/// that changes slowly; ~once a second at 60 FPS is plenty.
const COMPONENT_ACCESS_SAMPLE_PERIOD: u64 = 60;

/// Upper bound on real frame delta fed into the simulation accumulator, in
/// seconds. A longer real gap (debugger break, asset hitch, window drag) is
/// truncated to this value so the accumulator never demands an unbounded
/// number of catch-up steps — the classic "spiral of death".
const MAX_FRAME_DELTA_SECONDS: f32 = 0.25;

type AgentSlot = (
    Arc<Mutex<dyn khora_core::agent::Agent>>,
    AgentImportance,
    f32,
    Vec<AgentDependency>,
);

/// Which subset of a phase's agents [`ExecutionScheduler::execute_agents_in_phase`]
/// should run, used to split the fixed-update sub-loop from the once-per-frame
/// loop without double-running any agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentSelection {
    /// Every agent in the phase (legacy path — no fixed agent present).
    All,
    /// Only agents declaring a `fixed_timestep` (the sub-stepped sim agents).
    FixedOnly,
    /// Every agent except the fixed-timestep ones (they were sub-stepped).
    ExcludeFixed,
}

/// Whether the agent in `slot` declares a positive `fixed_timestep`.
fn slot_is_fixed(slot: &AgentSlot) -> bool {
    slot.0
        .lock()
        .ok()
        .and_then(|a| a.execution_timing().fixed_timestep)
        .map(|d| d > Duration::ZERO)
        .unwrap_or(false)
}

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
    /// Read-only observation tunnel to the DCC: per-agent cost samples and
    /// per-component access snapshots are pushed here (non-blocking) for the
    /// cold-path cost model + layout advisor. `None` if telemetry is disabled.
    telemetry: Option<Sender<TelemetryEvent>>,
    /// Monotonic frame counter, used to throttle low-rate telemetry sampling.
    frame_counter: u64,
    /// Wall-clock instant of the previous `run_frame`, used to derive the
    /// real per-frame delta. `None` on the very first frame.
    last_frame_instant: Option<Instant>,
    /// Carried-over simulation time (seconds) for the fixed-timestep
    /// accumulator. Each frame the real delta is added and whole
    /// `fixed_delta` steps are consumed; the remainder stays here.
    sim_accumulator: f32,
    /// When `true`, each phase runs through the parallel wave executor
    /// ([`execute_agents_parallel`](Self::execute_agents_parallel)); when
    /// `false` (default) agents run sequentially. Off by default: enabling it
    /// is only sound once agents opt into
    /// [`AgentAccess::Isolated`](khora_core::agent::AgentAccess::Isolated) and no
    /// two `Isolated` agents in a phase write the same deck slot.
    parallel_execution: bool,
    /// Persistent worker pool for concurrent waves. Spawned once here and
    /// joined on drop; runs a wave's `Isolated` agents as `'static` jobs while
    /// its `SharedWorld` agents run inline. See [`WorkerPool`].
    pool: WorkerPool,
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
            telemetry: None,
            frame_counter: 0,
            last_frame_instant: None,
            sim_accumulator: 0.0,
            parallel_execution: false,
            pool: WorkerPool::new(default_pool_size()),
        }
    }

    /// Enables or disables the parallel wave executor (default off).
    ///
    /// Sound only when the phase's agents correctly declare their
    /// [`AgentAccess`](khora_core::agent::AgentAccess): eligible agents must
    /// touch no `World` and write disjoint deck slots. With the default
    /// (all-`Exclusive`) declarations every wave is a singleton, so this is
    /// behaviourally identical to sequential execution.
    pub fn set_parallel_execution(&mut self, enabled: bool) {
        self.parallel_execution = enabled;
    }

    /// Publishes the current frame's [`TelemetryEvent::WavePlan`] to the DCC so
    /// the cold-path cost model can budget a concurrent wave by its critical
    /// path (`max`) rather than the sum of its members.
    ///
    /// No-op when telemetry is disabled or parallel execution is off — serial
    /// execution runs every agent as its own singleton, so summing per-agent
    /// costs (the DCC's fallback with no plan) is already exact.
    fn emit_wave_plan(&self, mode: &EngineMode) {
        let Some(tx) = &self.telemetry else { return };
        if !self.parallel_execution {
            return;
        }
        // Mirror the executor's grouping: for each phase, the phase's agents in
        // `sort_agents` order, partitioned into waves. Every agent lands in
        // exactly one wave (Exclusive agents are singletons), so cross-phase and
        // serial work is naturally costed as a sum while concurrent members
        // share a wave.
        let mut waves: Vec<Vec<AgentId>> = Vec::new();
        for &phase in &self.phase_order {
            let agents = {
                let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
                registry.collect_for_phase(phase, mode)
            };
            if agents.is_empty() {
                continue;
            }
            let metas = build_wave_metas(&sort_agents(agents));
            for wave in partition_waves(&metas) {
                let ids: Vec<AgentId> = wave.iter().filter_map(|&i| metas[i].id).collect();
                if !ids.is_empty() {
                    waves.push(ids);
                }
            }
        }
        let _ = tx.try_send(TelemetryEvent::WavePlan { waves });
    }

    /// Connects the read-only observation tunnel to the DCC. The scheduler then
    /// publishes per-agent cost samples and per-component access snapshots so
    /// the cold path can fit cost models and recommend layouts. Non-blocking:
    /// if the channel is full the sample is dropped (telemetry is best-effort).
    pub fn set_telemetry_sender(&mut self, sender: Sender<TelemetryEvent>) {
        self.telemetry = Some(sender);
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

/// Worker-pool size: one thread per available core minus two (reserved for the
/// main/render thread and the DCC cold thread), clamped to `[1, 16]`.
fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2))
        .unwrap_or(1)
        .clamp(1, 16)
}

/// How fast the simulated world should run this frame.
///
/// Defaults to real time when nothing holds a [`Time`](khora_core::time::Time)
/// — a headless host that never installed one is not a paused one.
fn simulation_scale(runtime: &Runtime) -> f32 {
    runtime
        .resources
        .get::<khora_core::time::SharedTime>()
        .and_then(|shared| shared.read().ok().map(|time| time.scale()))
        .unwrap_or(1.0)
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
