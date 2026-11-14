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

//! The execution scheduler: runs each family's installed layer, phase by phase, every frame.
//! See the book: concepts/the-frame.md, "The six-stage descent".

use crate::agent_registry::AgentRegistry;
use crate::budget_channel::{BudgetChannel, Publication};
use crate::worker_pool::WorkerPool;
use crossbeam_channel::Sender;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, ExecutionPhase};
use khora_core::graph::topological_sort;
use khora_core::lane::OutputDeck;
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use known::{Known, KnownAgents};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use waves::{build_wave_metas, partition_waves};

mod background;
mod clocks;
mod execution;
mod frame;
mod known;
mod layers;
mod lending;
mod panic_guard;
mod pooled_run;
mod sim_steps;
mod sub_steps;
mod waves;

/// How often, in frames, the scheduler samples component access stats for the layout advisor.
///
/// The stats move slowly: once a second at 60 FPS spares the telemetry channel.
const COMPONENT_ACCESS_SAMPLE_PERIOD: u64 = 60;

/// The longest real frame delta, in seconds, fed into the simulation accumulator.
///
/// A longer gap is truncated, so the accumulator never demands unbounded catch-up.
/// See the book: concepts/the-frame.md, "Fixed-timestep simulation and render interpolation".
const MAX_FRAME_DELTA_SECONDS: f32 = 0.25;

/// An agent of a phase: its lock, its importance, its priority, its
/// dependencies, and what the scheduler knows of it.
type AgentSlot = (
    Arc<Mutex<dyn khora_core::agent::Agent>>,
    AgentImportance,
    f32,
    Vec<AgentDependency>,
    Known,
);

/// Which of a phase's agents [`ExecutionScheduler::execute_agents_in_phase`] runs.
///
/// Splits the fixed-update sub-loop from the once-per-frame loop, so no agent runs twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentSelection {
    /// Every agent in the phase: no place holds a fixed step.
    All,
    /// The fixed agents still owed a sub-step at this round of the fixed-update sub-loop:
    /// round `r` runs every fixed agent owed more than `r` sub-steps this frame.
    FixedRound(u32),
    /// Every agent except the fixed-timestep ones (they were sub-stepped).
    ExcludeFixed,
}

/// The hot-path execution scheduler.
///
/// See the book: concepts/agents-and-lanes.md, "Lanes are layers".
pub struct ExecutionScheduler {
    registry: Arc<Mutex<AgentRegistry>>,
    budget_channel: BudgetChannel,
    /// What the frame starts took from the channel, oldest first, each with whether it
    /// was published in turn.
    ///
    /// The last is in force; the others are replays an agent of a family on is still owed.
    taken: Vec<(Publication, bool)>,
    /// Each agent seen at a frame start, by a weak reference to its lock, with the place
    /// of the last publication it was handed: it is owed what was taken after it.
    lent: Vec<(Weak<Mutex<dyn khora_core::agent::Agent>>, u64)>,
    /// The families seen off at a frame start and not on again since.
    resting: BTreeSet<AgentId>,
    /// The families on again, each with the place in force when found on (`None` while
    /// nothing was taken).
    ///
    /// Each is withdrawn at the first frame start its agent is free, and owed only what
    /// was taken after that place.
    resuming: Vec<(AgentId, Option<u64>)>,
    phase_order: Vec<ExecutionPhase>,
    frame_start: Instant,
    frame_budget: Duration,
    /// The last frame's output deck, kept for the engine's I/O boundary to drain.
    last_deck: OutputDeck,
    /// The observation tunnel to the DCC, for run reports and component access
    /// snapshots; `None` when telemetry is off.
    telemetry: Option<Sender<TelemetryEvent>>,
    /// Monotonic frame counter, used to throttle low-rate telemetry sampling.
    frame_counter: u64,
    /// Wall-clock instant of the previous `run_frame`, used to derive the
    /// real per-frame delta. `None` on the very first frame.
    last_frame_instant: Option<Instant>,
    /// Simulated time, in seconds, carried over on the game's clock: what is left once
    /// whole ticks are consumed.
    sim_accumulator: f32,
    /// Per fixed agent, the ticked time, in seconds, it has not stepped through yet.
    ///
    /// Fed only by ticks, so it never holds time the game's clock has not passed. One
    /// clock per agent: each cuts the same time at its own step.
    fixed_clocks: HashMap<AgentId, f32>,
    /// This frame's sub-steps per fixed agent, read by the fixed-update
    /// sub-loop's [`AgentSelection::FixedRound`].
    fixed_due: HashMap<AgentId, u32>,
    /// When `true`, agents whose contentions are disjoint share concurrent
    /// waves; when `false` (default) every agent is a wave of its own.
    parallel_execution: bool,
    /// The persistent worker pool the members of a concurrent wave run on.
    pool: WorkerPool,
    /// What agents wrote into the deck without declaring it, last frame
    /// (debug builds only; see [`undeclared_deck_writes`](Self::undeclared_deck_writes)).
    undeclared_deck_writes: Mutex<Vec<(AgentId, std::any::TypeId)>>,
    /// Every agent met, with its name, its place and its timing: a quiet
    /// frame locks no agent.
    known: KnownAgents,
    /// The runs of the families at the background, off the frame.
    background: background::Background,
    /// Whether the host began the frame (`begin_frame`) since the last
    /// `run_frame`: a frame not begun is begun by `run_frame` itself.
    begun: bool,
}

impl ExecutionScheduler {
    /// Creates a scheduler over the agents of `registry`.
    ///
    /// Every agent the registry holds, now or later, is lent the allocations
    /// the DCC publishes. The families a frame runs are the runtime's
    /// [`Activation`](khora_core::agent::Activation).
    pub fn from_registry(registry: Arc<Mutex<AgentRegistry>>) -> Self {
        Self {
            registry,
            budget_channel: BudgetChannel::new(),
            taken: Vec::new(),
            lent: Vec::new(),
            resting: BTreeSet::new(),
            resuming: Vec::new(),
            phase_order: ExecutionPhase::DEFAULT_ORDER.to_vec(),
            frame_start: Instant::now(),
            frame_budget: Duration::from_millis(16),
            last_deck: OutputDeck::new(),
            telemetry: None,
            frame_counter: 0,
            last_frame_instant: None,
            sim_accumulator: 0.0,
            fixed_clocks: HashMap::new(),
            fixed_due: HashMap::new(),
            parallel_execution: false,
            pool: WorkerPool::new(default_pool_size()),
            undeclared_deck_writes: Mutex::new(Vec::new()),
            known: KnownAgents::default(),
            background: background::Background::new(default_background_size()),
            begun: false,
        }
    }

    /// Returns the deck slots agents wrote last frame without declaring them, each with
    /// its writer.
    ///
    /// Declared means listed in the agent's
    /// [`Agent::contention`](khora_core::agent::Agent::contention) `deck`. Recorded in debug
    /// builds only: a release build returns an empty list. Each is also logged as an error.
    pub fn undeclared_deck_writes(&self) -> Vec<(AgentId, std::any::TypeId)> {
        self.undeclared_deck_writes
            .lock()
            .map(|found| found.clone())
            .unwrap_or_default()
    }

    /// Compares the deck slots `id` wrote with the ones its contention
    /// declared, and records (and logs) every one it did not declare.
    fn audit_deck_writes(
        &self,
        id: AgentId,
        declared: &khora_core::agent::Contention,
        written: Vec<std::any::TypeId>,
    ) {
        if !cfg!(debug_assertions) {
            return;
        }
        for type_id in written {
            if declared.deck.contains(&type_id) {
                continue;
            }
            log::error!(
                "agent {id:?} wrote deck slot {type_id:?} without declaring it in its contention"
            );
            if let Ok(mut found) = self.undeclared_deck_writes.lock() {
                found.push((id, type_id));
            }
        }
    }

    /// Enables or disables concurrent waves; off by default.
    ///
    /// On, agents whose [`Contention`](khora_core::agent::Contention)s are disjoint share a
    /// wave; off, every agent is a wave of its own and runs inline.
    pub fn set_parallel_execution(&mut self, enabled: bool) {
        self.parallel_execution = enabled;
    }

    /// Waits for the work off the frame to end and hands it back to the families' places.
    ///
    /// That work is the background runs and the layer preparations under way. Call it
    /// before shutting the lanes down, so every layer is shut down; dropping the scheduler
    /// does it too.
    pub fn wind_down(&mut self) {
        self.background.wind_down();
    }

    /// Publishes this frame's [`TelemetryEvent::WavePlan`], so the DCC costs a wave by its
    /// critical path.
    ///
    /// A no-op when telemetry is off, or when parallel execution is: every wave
    /// is then a single agent, and the DCC's sum is already exact.
    fn emit_wave_plan(&self, inactive: &BTreeSet<AgentId>) {
        let Some(tx) = &self.telemetry else { return };
        if !self.parallel_execution {
            return;
        }
        // Mirrors the executor's grouping, so every agent lands in exactly one wave.
        // Fixed-step agents run in the fixed rounds, apart from the once-per-frame
        // ones, so the two never share a wave.
        let mut waves: Vec<Vec<AgentId>> = Vec::new();
        for &phase in &self.phase_order {
            let (fixed, once): (Vec<AgentSlot>, Vec<AgentSlot>) = self
                .phase_slots(phase, inactive)
                .into_iter()
                .partition(|slot| self.fixed_slot_id(slot).is_some());
            for agents in [fixed, once] {
                if agents.is_empty() {
                    continue;
                }
                let metas = build_wave_metas(&sort_agents(agents));
                for wave in partition_waves(&metas, true) {
                    let ids: Vec<AgentId> = wave.iter().map(|&i| metas[i].id).collect();
                    if !ids.is_empty() {
                        waves.push(ids);
                    }
                }
            }
        }
        let _ = tx.try_send(TelemetryEvent::WavePlan { waves });
    }

    /// Connects the observation tunnel to the DCC: run reports and component access
    /// snapshots.
    ///
    /// Non-blocking: a sample is dropped when the channel is full.
    pub fn set_telemetry_sender(&mut self, sender: Sender<TelemetryEvent>) {
        self.telemetry = Some(sender);
    }

    /// Returns the last frame's [`OutputDeck`], for the engine to drain at the I/O boundary.
    pub fn deck_mut(&mut self) -> &mut OutputDeck {
        &mut self.last_deck
    }

    /// Returns a reference to the budget channel for the DCC to send budgets.
    pub fn budget_channel(&self) -> &BudgetChannel {
        &self.budget_channel
    }

    /// Sets the phase execution order.
    pub fn set_phase_order(&mut self, order: &[ExecutionPhase]) {
        self.phase_order = order.to_vec();
    }

    /// Returns the agents of the families on in `phase`, with their importance, priority
    /// and dependencies.
    ///
    /// Locks no agent: the timing is the one it declared when the scheduler met it.
    fn phase_slots(&self, phase: ExecutionPhase, inactive: &BTreeSet<AgentId>) -> Vec<AgentSlot> {
        let agents: Vec<Arc<Mutex<dyn khora_core::agent::Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        agents
            .into_iter()
            .filter_map(|agent| {
                let known = self.known.of(&agent)?;
                let timing = known.timing.clone();
                (timing.phase == phase).then_some((
                    agent,
                    timing.importance,
                    timing.priority,
                    timing.dependencies,
                    known,
                ))
            })
            .collect()
    }

    /// Returns the id of the agent in `slot` when its place runs at a fixed step.
    fn fixed_slot_id(&self, slot: &AgentSlot) -> Option<AgentId> {
        slot.4.fixed_step().map(|_| slot.4.id)
    }
}

/// Orders a phase's agents by their Hard dependencies, then by importance and priority.
///
/// On a dependency cycle, logs an error and keeps the importance and priority order.
fn sort_agents(mut agents: Vec<AgentSlot>) -> Vec<AgentSlot> {
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
        .map(|(idx, slot)| (slot.4.id, idx))
        .collect();

    // In the importance + priority order: the topological sort keeps its
    // input order among agents no dependency orders.
    let mut nodes: Vec<(usize, AgentId)> = id_to_index.iter().map(|(id, i)| (*i, *id)).collect();
    nodes.sort_unstable_by_key(|(index, _)| *index);
    let nodes: Vec<AgentId> = nodes.into_iter().map(|(_, id)| id).collect();

    // Build hard-dep edges (parent = dep target, child = dependent agent)
    // — the dep target must run first.
    let mut edges: Vec<(AgentId, AgentId)> = Vec::new();
    for slot in &agents {
        let agent_id = slot.4.id;
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

/// Returns the background pool's size: a quarter of the cores, within `[1, 8]`.
///
/// Apart from the waves' pool, so a long run never queues in front of a wave's jobs.
fn default_background_size() -> usize {
    std::thread::available_parallelism()
        .map(|cores| cores.get() / 4)
        .unwrap_or(1)
        .clamp(1, 8)
}

/// Returns the waves' pool size: the cores minus two (the main thread's and the DCC's), within
/// `[1, 16]`.
fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2))
        .unwrap_or(1)
        .clamp(1, 16)
}

/// Returns how fast the simulated world runs this frame.
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
            // A dependency's condition is not evaluated here: a conditional Hard
            // dependency counts as active.
            match completion_map.outcome(dep.target) {
                Some(CompletionOutcome::Completed) => {}
                Some(CompletionOutcome::Skipped | CompletionOutcome::Failed) | None => {
                    return false
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
