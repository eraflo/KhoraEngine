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
use crate::worker_pool::WorkerPool;
use crossbeam_channel::Sender;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentAccess, AgentDependency, EngineMode, ExecutionPhase};
use khora_core::control::gorna::{AgentFrameStatus, AgentFrameStatusMap, AgentId};
use khora_core::graph::topological_sort;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often (in frames) the scheduler samples per-component access stats for
/// the layout advisor. Per-frame would flood the telemetry channel for data
/// that changes slowly; ~once a second at 60 FPS is plenty.
const COMPONENT_ACCESS_SAMPLE_PERIOD: u64 = 60;

/// Upper bound on real frame delta fed into the simulation accumulator, in
/// seconds. A longer real gap (debugger break, asset hitch, window drag) is
/// truncated to this value so the accumulator never demands an unbounded
/// number of catch-up steps — the classic "spiral of death".
const MAX_FRAME_DELTA_SECONDS: f32 = 0.25;

/// Upper bound on fixed simulation sub-steps run in a single frame. When the
/// accumulator would demand more, the excess time is dropped (the sim runs
/// in slow-motion rather than freezing). Bounds worst-case per-frame cost.
const MAX_SIM_STEPS: u32 = 5;

/// Pure step-count arithmetic for the fixed-timestep accumulator.
///
/// Given the carried-over `accumulator`, the (already clamped) real frame
/// `dt`, the simulation `fixed_delta`, and a `max_steps` ceiling, returns:
/// - `steps` — whole fixed sub-steps to run this frame (`0..=max_steps`),
/// - `new_accumulator` — leftover time carried to the next frame,
/// - `alpha` — render-interpolation factor in `[0, 1)`.
///
/// On `max_steps` saturation the excess accumulated time is discarded so the
/// accumulator stays bounded (slow-motion under sustained overload rather than
/// a runaway). A non-positive `fixed_delta` is treated as a single step with
/// no leftover (degenerate guard; callers pass a positive step).
fn compute_sim_steps(accumulator: f32, dt: f32, fixed_delta: f32, max_steps: u32) -> SimSteps {
    if fixed_delta <= 0.0 {
        return SimSteps {
            steps: 1,
            new_accumulator: 0.0,
            alpha: 0.0,
        };
    }

    let mut acc = accumulator + dt;
    let mut steps = (acc / fixed_delta).floor() as i64;
    if steps < 0 {
        steps = 0;
    }
    let mut steps = steps as u32;

    if steps > max_steps {
        // Drop the excess: consume exactly `max_steps` worth of time and
        // discard the remainder so the accumulator cannot grow without bound.
        acc -= max_steps as f32 * fixed_delta;
        let dropped = (acc / fixed_delta).floor().max(0.0);
        acc -= dropped * fixed_delta;
        steps = max_steps;
    } else {
        acc -= steps as f32 * fixed_delta;
    }

    // Guard against tiny negative residue from float subtraction.
    if acc < 0.0 {
        acc = 0.0;
    }
    let alpha = (acc / fixed_delta).clamp(0.0, 1.0);

    SimSteps {
        steps,
        new_accumulator: acc,
        alpha,
    }
}

/// Result of [`compute_sim_steps`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct SimSteps {
    steps: u32,
    new_accumulator: f32,
    alpha: f32,
}

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
    /// the lone `SharedWorld` agent runs inline. See [`WorkerPool`].
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

    /// Executes the complete frame cycle.
    ///
    /// This is called every frame by the engine loop.
    ///
    /// ## Fixed-timestep sequencing
    ///
    /// Rendering runs at the display's variable rate, but the simulation must
    /// advance in fixed increments to stay frame-rate independent and
    /// deterministic. The scheduler reconciles the two with an accumulator:
    ///
    /// 1. Measure the real wall-clock delta since the previous frame, clamped
    ///    to [`MAX_FRAME_DELTA_SECONDS`] (spiral-of-death guard), and add it to
    ///    `sim_accumulator`.
    /// 2. The **fixed step** is the smallest `fixed_timestep` declared by any
    ///    registered agent (a single sim clock — physics owns it via its GORNA
    ///    strategy). If no agent declares one, the frame degrades to the legacy
    ///    "everything once per frame" path with no behaviour change.
    /// 3. Consume whole steps: `steps = floor(accumulator / fixed_delta)`,
    ///    capped at [`MAX_SIM_STEPS`]; the remainder carries over and yields the
    ///    render `interpolation_alpha`.
    /// 4. Run the **fixed-timestep agents** (TRANSFORM-phase physics) `steps`
    ///    times — a fixed-update sub-loop — so the provider advances N discrete
    ///    sub-steps. Then run the regular phase loop **once**, excluding the
    ///    agents already stepped, so OUTPUT-phase render fires a single time.
    /// 5. Publish the fresh `Time` (delta, fixed_delta, alpha) into the runtime
    ///    resource before the render phase reads it.
    ///
    /// Substrate Flows project once per frame; the GORNA completion map,
    /// budget arbitration, and telemetry all observe each individual agent
    /// invocation (a sub-step counts as a real run, with its own cost sample).
    pub fn run_frame(&mut self, world: &mut World, runtime: Arc<Runtime>) {
        // 1. Sync budgets from cold thread
        self.budget_channel.sync();
        self.frame_start = Instant::now();

        // 1b. Real frame delta + fixed-timestep accumulator bookkeeping.
        let now = self.frame_start;
        let fixed_delta = self.smallest_fixed_delta();
        let dt = match self.last_frame_instant {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            // First frame: advance by exactly one fixed step (no real history).
            None => fixed_delta.unwrap_or(khora_core::time::DEFAULT_FIXED_DELTA_SECONDS),
        }
        .min(MAX_FRAME_DELTA_SECONDS);
        self.last_frame_instant = Some(now);

        // With no fixed-timestep agent, the simulation isn't decoupled: run
        // everything exactly once (legacy behaviour) and report alpha 0.
        let (sim_steps, fixed_delta_for_time) = match fixed_delta {
            Some(fd) => {
                let r = compute_sim_steps(self.sim_accumulator, dt, fd, MAX_SIM_STEPS);
                self.sim_accumulator = r.new_accumulator;
                (r, fd)
            }
            None => (
                SimSteps {
                    steps: 1,
                    new_accumulator: 0.0,
                    alpha: 0.0,
                },
                khora_core::time::DEFAULT_FIXED_DELTA_SECONDS,
            ),
        };

        // 1c. Publish the per-frame Time resource (read by Flows + game code).
        self.publish_time(&runtime, dt, fixed_delta_for_time, sim_steps.alpha);

        // 2. Build the per-frame completion map. The scheduler tracks
        //    completion internally — agents do not read it through the
        //    runtime containers.
        let agent_ids = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all_ids();
        let completion_map = Arc::new(AgentCompletionMap::new(&agent_ids));

        // 3. Read current mode
        let mode = {
            let ctx = self.context.read().unwrap_or_else(|e| e.into_inner());
            ctx.mode.clone()
        };

        // 3b. Publish how agents will be grouped into concurrent waves this
        //     frame, so the DCC can budget each wave by its critical path.
        self.emit_wave_plan(&mode);

        // 4. Build the per-frame substrate: typed input bus and output deck.
        //    The deck is moved into the scheduler's `last_deck` slot at the
        //    end of the frame so the engine I/O layer can drain it.
        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        // 5. Run the Substrate Pass: every registered Flow projects its View
        //    into the bus. Flows are read-only projectors — no budget needed
        //    (only agents compete for the frame budget).
        substrate::run_flows(world, &mut bus, &runtime);

        // Freeze the bus behind an `Arc` for the rest of the frame. It is
        // strictly read-only from here on (the CLAD descent only reads Views),
        // and the worker pool needs an owned `Arc<LaneBus>` to make a concurrent
        // wave's `Isolated` jobs `'static`. Serial paths deref it to `&LaneBus`.
        let bus = Arc::new(bus);

        // 6. Fixed-update sub-loop. When the simulation is decoupled, the
        //    fixed-timestep agents advance `steps` discrete sub-steps before
        //    the once-per-frame phase loop. Each sub-step is a full agent
        //    invocation, so the physics provider integrates N times while the
        //    render pass below fires once. When there is no fixed agent
        //    (`fixed_delta == None`), `steps` is 1 and these agents simply run
        //    in their normal phase below — so this loop does nothing.
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();
        if fixed_delta.is_some() {
            for _ in 0..sim_steps.steps {
                for &phase in &phases {
                    self.execute_agents_in_phase(
                        phase,
                        world,
                        &runtime,
                        &mode,
                        &completion_map,
                        &bus,
                        &mut deck,
                        AgentSelection::FixedOnly,
                    );
                }
            }
        }

        // Once-per-frame agents. When the sim was sub-stepped above, fixed
        // agents are excluded here so they are not run an extra time; with no
        // fixed agent, every agent runs through this `All` path as before.
        let selection = if fixed_delta.is_some() {
            AgentSelection::ExcludeFixed
        } else {
            AgentSelection::All
        };

        // 8. Execute each phase: plugins then agents (CLAD descent —
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
                selection,
            );
        }

        // 9. Hand the populated deck off to the engine for the I/O boundary.
        self.last_deck = deck;

        // 10. Low-rate observation tunnel: publish per-component access snapshots
        //    so the DCC's layout advisor can recommend layouts. Sampled every
        //    `COMPONENT_ACCESS_SAMPLE_PERIOD` frames (the counters are cumulative
        //    and move slowly), and best-effort (dropped if the channel is full).
        self.frame_counter = self.frame_counter.wrapping_add(1);
        if let Some(tx) = &self.telemetry {
            if self
                .frame_counter
                .is_multiple_of(COMPONENT_ACCESS_SAMPLE_PERIOD)
            {
                for (type_name, size_bytes, query_count, rows_scanned) in
                    world.component_access_snapshot()
                {
                    let _ = tx.try_send(TelemetryEvent::ComponentAccess {
                        type_name,
                        size_bytes,
                        query_count,
                        rows_scanned,
                    });
                }
            }
        }
    }

    /// Smallest `fixed_timestep` (in seconds) declared by any registered
    /// agent, or `None` if no agent declares one.
    ///
    /// A single sim clock is assumed: when several agents declare different
    /// fixed steps the smallest wins, so every fixed agent is stepped at least
    /// as often as it asked for. In practice physics is the sole owner.
    fn smallest_fixed_delta(&self) -> Option<f32> {
        let registry = self.registry.lock().ok()?;
        registry
            .iter()
            .filter_map(|agent| {
                agent
                    .lock()
                    .ok()
                    .and_then(|a| a.execution_timing().fixed_timestep)
            })
            .map(|d| d.as_secs_f32())
            .filter(|d| *d > 0.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Writes the per-frame [`Time`](khora_core::time::Time) into the runtime
    /// resource so Flows and game `update` read the real delta + alpha.
    /// No-op if no `SharedTime` resource is registered.
    fn publish_time(&self, runtime: &Runtime, dt: f32, fixed_delta: f32, alpha: f32) {
        let Some(shared) = runtime.resources.get::<khora_core::time::SharedTime>() else {
            return;
        };
        if let Ok(mut time) = shared.write() {
            time.delta_seconds = dt;
            time.fixed_delta_seconds = fixed_delta;
            time.interpolation_alpha = alpha;
            time.frame = time.frame.wrapping_add(1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        world: &mut World,
        runtime: &Arc<Runtime>,
        mode: &EngineMode,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
        selection: AgentSelection,
    ) {
        // Collect agents for this phase and mode
        let agents = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.collect_for_phase(phase, mode)
        };

        // Partition by whether the agent declares a fixed timestep, so the
        // fixed-update sub-loop and the once-per-frame loop never double-run
        // the same agent.
        let agents: Vec<AgentSlot> = match selection {
            AgentSelection::All => agents,
            AgentSelection::FixedOnly => agents.into_iter().filter(slot_is_fixed).collect(),
            AgentSelection::ExcludeFixed => agents
                .into_iter()
                .filter(|slot| !slot_is_fixed(slot))
                .collect(),
        };

        if agents.is_empty() {
            return;
        }

        let sorted = sort_agents(agents);
        if self.parallel_execution {
            self.execute_agents_parallel(sorted, world, runtime, completion_map, bus, deck);
        } else {
            self.execute_agents_sequential(sorted, world, runtime, completion_map, bus, deck);
        }
    }

    /// Executes the phase's agents **sequentially, in priority order** (the
    /// [`sort_agents`] output). GORNA budgets are therefore per-agent
    /// *exclusive time slices of the frame*, not concurrent allocations:
    /// measured agent costs add up (T1 + T2 + …), which the arbitrator's budget
    /// fit models as a sum. The parallel path
    /// ([`execute_agents_parallel`](Self::execute_agents_parallel)) instead
    /// groups concurrency-eligible agents into waves and publishes that grouping
    /// so the fit costs each wave by its critical path; with an all-singleton
    /// grouping (this serial path) the two are identical.
    fn execute_agents_sequential(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
    ) {
        // Serial paths only need a shared `&LaneBus`.
        let bus: &LaneBus = bus;

        // Coarse workload size for the cost model — sampled once per phase
        // (per-domain refinement is a later step).
        let workload_n = world.entity_count() as f64;

        // Scheduler-owned per-agent frame metrics: agents read their own slot
        // in `report_status` so they hold no per-frame counters themselves.
        let frame_status = runtime.resources.get::<AgentFrameStatusMap>().cloned();
        let record_frame_time = |id: AgentId, measured_time_ms: f32| {
            if let Some(fs) = &frame_status {
                fs.write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id, AgentFrameStatus { measured_time_ms });
            }
        };

        for (agent, importance, _priority, dependencies) in agents {
            let agent_id = match agent.lock().ok().map(|a| a.id()) {
                Some(id) => id,
                None => continue,
            };

            // Budget escape valve: only *negotiable* (Optional) work is skipped
            // under pressure. Critical/Important agents are non-negotiable.
            if importance.is_negotiable() && self.is_under_budget_pressure() {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                record_frame_time(agent_id, 0.0);
                continue;
            }

            // Skip if hard dependencies were skipped or are unmarked
            if !are_hard_dependencies_completed(&dependencies, completion_map) {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                record_frame_time(agent_id, 0.0);
                continue;
            }

            // Build EngineContext and execute (CLAD descent: the agent
            // chooses and invokes its lane internally).
            let mut engine_ctx = EngineContext {
                world: WorldAccess::Exclusive(world as &mut dyn std::any::Any),
                runtime: Arc::clone(runtime),
                bus,
                deck,
            };

            let started = Instant::now();
            if let Ok(mut a) = agent.lock() {
                a.execute(&mut engine_ctx);
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            record_frame_time(agent_id, elapsed_ms as f32);
            // Observation tunnel: report (n, time) so the DCC can fit this
            // agent's cost model and forecast budget breaches. Best-effort.
            if let Some(tx) = &self.telemetry {
                let _ = tx.try_send(TelemetryEvent::AgentCost {
                    id: agent_id,
                    n: workload_n,
                    time_ms: elapsed_ms,
                });
            }
            completion_map.mark(agent_id, CompletionOutcome::Completed);
        }
    }

    /// Parallel execution path — runs the phase's agents wave by wave.
    ///
    /// Agents declaring [`AgentAccess::Isolated`] (no `World` access, writes
    /// only their own `OutputDeck`) are grouped into concurrent **waves** via
    /// [`partition_waves`]; every other agent is a singleton wave. Waves run in
    /// order, so a wave never contains an agent that hard-depends on another
    /// member — Hard-dependency ordering is preserved exactly as in the
    /// sequential path.
    ///
    /// In a concurrent wave the `Isolated` agents run as `'static` jobs on the
    /// persistent [`WorkerPool`], each with a private `OutputDeck` shard and a
    /// cloned `Arc<LaneBus>` (`WorldAccess::None`); the wave's lone `SharedWorld`
    /// agent, if any, runs **inline on this thread** with a shared `&World` and
    /// writes straight into the shared deck. The `World` never crosses a thread
    /// boundary (no `unsafe`, no lifetime transmute). The pooled shards are
    /// folded back into the shared deck in wave (index) order once collected,
    /// keeping outputs deterministic. Singleton waves run inline with exclusive
    /// `&mut World`, identical to
    /// [`execute_agents_sequential`](Self::execute_agents_sequential).
    ///
    /// GORNA cost fitting still assumes sequential (sum-of-costs) budgets;
    /// switching it to a critical-path model is a follow-up (see the concurrent
    /// wave's per-agent timings recorded here).
    fn execute_agents_parallel(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
    ) {
        // Inline/singleton paths only need a shared `&LaneBus`; the pool jobs
        // clone the `Arc` itself to become `'static`.
        let bus_ref: &LaneBus = bus;

        let workload_n = world.entity_count() as f64;
        let frame_status = runtime.resources.get::<AgentFrameStatusMap>().cloned();
        let record_frame_time = |id: AgentId, measured_time_ms: f32| {
            if let Some(fs) = &frame_status {
                fs.write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id, AgentFrameStatus { measured_time_ms });
            }
        };
        let report_cost = |id: AgentId, time_ms: f64| {
            if let Some(tx) = &self.telemetry {
                let _ = tx.try_send(TelemetryEvent::AgentCost {
                    id,
                    n: workload_n,
                    time_ms,
                });
            }
        };

        // Read each agent's id + access footprint once (a failed lock yields no
        // id → an always-singleton, always-skipped slot, matching sequential).
        let metas = build_wave_metas(&agents);

        // Returns true if the agent should run (not budget-skipped, hard deps
        // satisfied); marks + records the skip otherwise.
        let gate = |id: AgentId, importance: AgentImportance, deps: &[AgentDependency]| -> bool {
            if importance.is_negotiable() && self.is_under_budget_pressure() {
                completion_map.mark(id, CompletionOutcome::Skipped);
                record_frame_time(id, 0.0);
                return false;
            }
            if !are_hard_dependencies_completed(deps, completion_map) {
                completion_map.mark(id, CompletionOutcome::Skipped);
                record_frame_time(id, 0.0);
                return false;
            }
            true
        };

        for wave in partition_waves(&metas) {
            // Singleton wave: run inline with exclusive &mut World (identical to
            // the sequential path — covers every Exclusive agent).
            if wave.len() == 1 {
                let idx = wave[0];
                let Some(id) = metas[idx].id else { continue };
                let (agent, importance, _priority, deps) = &agents[idx];
                if !gate(id, *importance, deps) {
                    continue;
                }
                let mut engine_ctx = EngineContext {
                    world: WorldAccess::Exclusive(world as &mut dyn std::any::Any),
                    runtime: Arc::clone(runtime),
                    bus: bus_ref,
                    deck,
                };
                let started = Instant::now();
                if let Ok(mut a) = agent.lock() {
                    a.execute(&mut engine_ctx);
                }
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                record_frame_time(id, elapsed_ms as f32);
                report_cost(id, elapsed_ms);
                completion_map.mark(id, CompletionOutcome::Completed);
                continue;
            }

            // Verify the wave's members write disjoint deck slots before we run
            // them into private shards (declared via `Agent::deck_writes`). A
            // collision is a parallel-eligibility bug: the shards would clash on
            // merge — surface it here, naming the agents, not later on a raw TypeId.
            check_wave_deck_disjoint(&wave, &metas);

            // Concurrent wave. Gate on this thread, then split the survivors:
            // the `Isolated` agents (world-free) run as `'static` jobs on the
            // persistent pool, reaching their inputs through `Arc<LaneBus>` /
            // `Arc<Runtime>`; the wave's lone `SharedWorld` agent (if any) runs
            // inline here with a shared `&World`. The `World` therefore never
            // crosses a thread boundary — no `unsafe`, no lifetime transmute.
            let runnable: Vec<usize> = wave
                .into_iter()
                .filter(|&idx| match metas[idx].id {
                    Some(id) => gate(id, agents[idx].1, &agents[idx].3),
                    None => false,
                })
                .collect();
            if runnable.is_empty() {
                continue;
            }

            // Dispatch every `Isolated` agent to the pool first, so their work
            // overlaps the inline `SharedWorld` agent below. Each sends back
            // `(idx, id, shard, ms)`; `idx` recovers wave order for a
            // deterministic fold.
            let (tx, rx) = std::sync::mpsc::channel::<(usize, AgentId, OutputDeck, f64)>();
            let mut isolated_count = 0usize;
            for &idx in runnable
                .iter()
                .filter(|&&idx| metas[idx].access != AgentAccess::SharedWorld)
            {
                let agent = Arc::clone(&agents[idx].0);
                let id = metas[idx].id.expect("gated runnable has an id");
                let runtime = Arc::clone(runtime);
                let bus = Arc::clone(bus);
                let tx = tx.clone();
                isolated_count += 1;
                self.pool.submit(move || {
                    let bus_ref: &LaneBus = &bus;
                    let mut shard = OutputDeck::new();
                    let started = Instant::now();
                    {
                        let mut ctx = EngineContext {
                            world: WorldAccess::None,
                            runtime,
                            bus: bus_ref,
                            deck: &mut shard,
                        };
                        if let Ok(mut a) = agent.lock() {
                            a.execute(&mut ctx);
                        }
                    }
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    let _ = tx.send((idx, id, shard, ms));
                });
            }
            // Drop our sender so the collector below terminates once the pool
            // jobs (the only remaining senders) have all reported.
            drop(tx);

            // Run the lone `SharedWorld` agent inline on this thread. It reads
            // `&World` and writes its slot straight into the shared deck (its
            // slot type is disjoint from the pooled shards, checked above).
            for &idx in runnable
                .iter()
                .filter(|&&idx| metas[idx].access == AgentAccess::SharedWorld)
            {
                let id = metas[idx].id.expect("gated runnable has an id");
                let mut ctx = EngineContext {
                    world: WorldAccess::Shared(&*world as &dyn std::any::Any),
                    runtime: Arc::clone(runtime),
                    bus: bus_ref,
                    deck,
                };
                let started = Instant::now();
                if let Ok(mut a) = agents[idx].0.lock() {
                    a.execute(&mut ctx);
                }
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                record_frame_time(id, ms as f32);
                report_cost(id, ms);
                completion_map.mark(id, CompletionOutcome::Completed);
            }

            // Collect the pooled `Isolated` results and fold their shards in
            // wave (idx) order → deterministic deck contents.
            let mut results: Vec<(usize, AgentId, OutputDeck, f64)> =
                rx.iter().take(isolated_count).collect();
            results.sort_by_key(|(idx, _, _, _)| *idx);
            for (_, id, shard, ms) in results {
                deck.merge_from(shard);
                record_frame_time(id, ms as f32);
                report_cost(id, ms);
                completion_map.mark(id, CompletionOutcome::Completed);
            }
        }
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

/// Per-agent metadata the parallel executor needs to group agents into waves.
struct WaveMeta {
    /// The agent's id, or `None` if it could not be locked (→ singleton, skipped).
    id: Option<AgentId>,
    /// Whether the agent is safe to run concurrently.
    access: AgentAccess,
    /// Ids this agent hard-depends on (must run in an earlier wave).
    hard_dep_targets: Vec<AgentId>,
    /// `OutputDeck` slot types this agent writes (from [`Agent::deck_writes`]),
    /// used to verify co-wave agents write disjoint slots before dispatch.
    deck_writes: Vec<std::any::TypeId>,
}

/// Worker-pool size: one thread per available core minus two (reserved for the
/// main/render thread and the DCC cold thread), clamped to `[1, 16]`.
fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2))
        .unwrap_or(1)
        .clamp(1, 16)
}

/// Reads each agent's id, access footprint, and declared deck writes once,
/// building the [`WaveMeta`] the wave partitioner + disjointness check need. A
/// failed lock yields `id: None` → an always-singleton, always-skipped slot.
fn build_wave_metas(agents: &[AgentSlot]) -> Vec<WaveMeta> {
    agents
        .iter()
        .map(|(agent, _, _, deps)| {
            let (id, access, deck_writes) = agent
                .lock()
                .ok()
                .map(|a| (Some(a.id()), a.access(), a.deck_writes()))
                .unwrap_or((None, AgentAccess::Exclusive, Vec::new()));
            WaveMeta {
                id,
                access,
                hard_dep_targets: deps
                    .iter()
                    .filter(|d| matches!(d.kind, DependencyKind::Hard))
                    .map(|d| d.target)
                    .collect(),
                deck_writes,
            }
        })
        .collect()
}

/// Groups agents (already in `sort_agents` order) into execution waves.
///
/// A wave is a maximal run of consecutive concurrency-eligible agents
/// ([`AgentAccess::Isolated`] or [`AgentAccess::SharedWorld`]) in which no
/// member hard-depends on another member and **at most one** is `SharedWorld`
/// (that agent may write shared resources, so two of them could race). Any
/// [`AgentAccess::Exclusive`] agent is its own singleton wave. Because the input
/// is already topologically ordered by hard deps, and a dependent never shares
/// a wave with its target, running the waves in order preserves the exact
/// Hard-dependency ordering the sequential path guarantees.
fn partition_waves(metas: &[WaveMeta]) -> Vec<Vec<usize>> {
    let mut waves: Vec<Vec<usize>> = Vec::new();
    for (i, meta) in metas.iter().enumerate() {
        let joins_current = meta.access != AgentAccess::Exclusive
            && waves.last().is_some_and(|wave| {
                // every current member is concurrency-eligible …
                wave.iter().all(|&j| metas[j].access != AgentAccess::Exclusive)
                    // … no member is this agent's hard-dep target …
                    && !meta.hard_dep_targets.iter().any(|target| {
                        wave.iter().any(|&j| metas[j].id == Some(*target))
                    })
                    // … and the wave holds no other SharedWorld agent.
                    && !(meta.access == AgentAccess::SharedWorld
                        && wave
                            .iter()
                            .any(|&j| metas[j].access == AgentAccess::SharedWorld))
            });
        if joins_current {
            waves.last_mut().expect("checked non-empty").push(i);
        } else {
            waves.push(vec![i]);
        }
    }
    waves
}

/// Logs an error for every `OutputDeck` slot type written by more than one
/// agent in the same concurrent `wave`.
///
/// Co-wave agents run into private deck shards that are folded back together
/// afterwards, so they must write disjoint slots (guaranteed in principle by
/// the [`AgentAccess`] eligibility rules). Declaring writes via
/// [`Agent::deck_writes`](khora_core::agent::Agent::deck_writes) lets the
/// scheduler catch a violation here — naming the offending agents — rather than
/// discovering it defensively during the shard merge on a bare `TypeId`.
///
/// Returns the number of collisions detected (0 = disjoint, the expected case).
fn check_wave_deck_disjoint(wave: &[usize], metas: &[WaveMeta]) -> usize {
    let mut seen: std::collections::HashMap<std::any::TypeId, AgentId> =
        std::collections::HashMap::new();
    let mut collisions = 0;
    for &idx in wave {
        let Some(id) = metas[idx].id else { continue };
        for &slot in &metas[idx].deck_writes {
            if let Some(prev) = seen.insert(slot, id) {
                collisions += 1;
                log::error!(
                    "Scheduler: agents {prev:?} and {id:?} share a concurrent wave but both write \
                     OutputDeck slot {slot:?} — their shards will collide on merge \
                     (parallel-eligibility bug)"
                );
            }
        }
    }
    collisions
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

#[cfg(test)]
mod wave_tests {
    use super::{check_wave_deck_disjoint, partition_waves, WaveMeta};
    use khora_core::agent::AgentAccess;
    use khora_core::control::gorna::AgentId;

    fn meta(id: AgentId, access: AgentAccess, deps: &[AgentId]) -> WaveMeta {
        WaveMeta {
            id: Some(id),
            access,
            hard_dep_targets: deps.to_vec(),
            deck_writes: Vec::new(),
        }
    }

    #[test]
    fn all_exclusive_are_singletons() {
        let metas = vec![
            meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
            meta(AgentId::Renderer, AgentAccess::Exclusive, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
    }

    #[test]
    fn consecutive_isolated_form_one_wave() {
        let metas = vec![
            meta(AgentId::Audio, AgentAccess::Isolated, &[]),
            meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0, 1]]);
    }

    #[test]
    fn exclusive_breaks_the_wave() {
        let metas = vec![
            meta(AgentId::Audio, AgentAccess::Isolated, &[]),
            meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
            meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn hard_dep_on_wave_member_splits_it() {
        // The second Isolated agent hard-depends on the first, so it must run in
        // a later wave — preserving the dependency ordering.
        let metas = vec![
            meta(AgentId::Audio, AgentAccess::Isolated, &[]),
            meta(
                AgentId::ShadowRenderer,
                AgentAccess::Isolated,
                &[AgentId::Audio],
            ),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
    }

    #[test]
    fn shared_world_joins_isolated_agents() {
        // One SharedWorld reader runs alongside any number of Isolated agents.
        let metas = vec![
            meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
            meta(AgentId::Audio, AgentAccess::Isolated, &[]),
            meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn two_shared_world_agents_split_into_separate_waves() {
        // Two SharedWorld agents may each write shared resources, so at most one
        // runs per wave.
        let metas = vec![
            meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
            meta(AgentId::Overlay, AgentAccess::SharedWorld, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
    }

    #[test]
    fn exclusive_still_breaks_a_mixed_wave() {
        let metas = vec![
            meta(AgentId::Audio, AgentAccess::Isolated, &[]),
            meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
            meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
            meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
        ];
        assert_eq!(partition_waves(&metas), vec![vec![0, 1], vec![2], vec![3]]);
    }

    fn meta_writes(id: AgentId, access: AgentAccess, writes: &[std::any::TypeId]) -> WaveMeta {
        WaveMeta {
            id: Some(id),
            access,
            hard_dep_targets: Vec::new(),
            deck_writes: writes.to_vec(),
        }
    }

    #[test]
    fn disjoint_deck_writes_pass_the_check() {
        // Two agents writing distinct slot types share a wave cleanly.
        let metas = vec![
            meta_writes(
                AgentId::Ui,
                AgentAccess::SharedWorld,
                &[std::any::TypeId::of::<u32>()],
            ),
            meta_writes(
                AgentId::Overlay,
                AgentAccess::Isolated,
                &[std::any::TypeId::of::<u64>()],
            ),
        ];
        assert_eq!(check_wave_deck_disjoint(&[0, 1], &metas), 0);
    }

    #[test]
    fn colliding_deck_writes_are_detected() {
        // Two agents in one wave both declaring the same slot type collide.
        let metas = vec![
            meta_writes(
                AgentId::Ui,
                AgentAccess::SharedWorld,
                &[std::any::TypeId::of::<u32>()],
            ),
            meta_writes(
                AgentId::Overlay,
                AgentAccess::Isolated,
                &[std::any::TypeId::of::<u32>()],
            ),
        ];
        assert_eq!(check_wave_deck_disjoint(&[0, 1], &metas), 1);
    }
}

#[cfg(test)]
mod concurrent_exec_tests {
    use super::*;
    use crate::context::Context;
    use crate::registry::AgentRegistry;
    use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
    use khora_core::agent::{Agent, AgentAccess, AgentImportance};
    use khora_core::control::gorna::{
        AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    };
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::{EngineContext, Runtime};
    use khora_data::ecs::World;
    use std::sync::{Arc, Mutex, RwLock};

    /// Deck slot: did the `SharedWorld` agent reach the shared `&World`?
    #[derive(Default)]
    struct SharedSaw(bool);
    /// Deck slot: did the `Isolated` agent run, and was it correctly world-free?
    #[derive(Default)]
    struct IsoRan {
        ran: bool,
        world_was_none: bool,
    }

    enum Role {
        SharedWorldReader,
        IsolatedWriter,
    }

    struct MockAgent {
        id: AgentId,
        access: AgentAccess,
        role: Role,
    }

    impl Agent for MockAgent {
        fn id(&self) -> AgentId {
            self.id
        }
        fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
            NegotiationResponse {
                strategies: vec![],
                timing_adjustment: None,
            }
        }
        fn apply_budget(&mut self, _b: ResourceBudget) {}
        fn report_status(&self) -> AgentStatus {
            AgentStatus {
                agent_id: self.id,
                current_strategy: StrategyId::Balanced,
                health_score: 1.0,
                is_stalled: false,
                message: String::new(),
            }
        }
        fn execute(&mut self, ctx: &mut EngineContext<'_>) {
            match self.role {
                Role::SharedWorldReader => {
                    let saw = ctx
                        .world_ref()
                        .and_then(|w| w.downcast_ref::<World>())
                        .is_some();
                    ctx.deck.slot::<SharedSaw>().0 = saw;
                }
                Role::IsolatedWriter => {
                    let world_was_none = ctx.world_ref().is_none();
                    let slot = ctx.deck.slot::<IsoRan>();
                    slot.ran = true;
                    slot.world_was_none = world_was_none;
                }
            }
        }
        fn access(&self) -> AgentAccess {
            self.access
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// Drives `execute_agents_parallel` directly with a `SharedWorld` reader and
    /// an `Isolated` writer in the same wave, proving the pool path: the reader
    /// runs inline with a shared `&World`, the writer runs pooled with none, both
    /// run, and the pooled shard folds back into the shared deck.
    #[test]
    fn concurrent_wave_runs_shared_reader_and_isolated_writer() {
        let scheduler = ExecutionScheduler::new(
            Arc::new(Mutex::new(AgentRegistry::new())),
            Arc::new(RwLock::new(Context::default())),
            &[AgentId::Renderer, AgentId::Audio],
        );
        let mut world = World::new();
        let runtime = Arc::new(Runtime::new());
        let completion = Arc::new(AgentCompletionMap::new(&[AgentId::Renderer, AgentId::Audio]));
        let bus = Arc::new(LaneBus::new());
        let mut deck = OutputDeck::new();

        let agents: Vec<AgentSlot> = vec![
            (
                Arc::new(Mutex::new(MockAgent {
                    id: AgentId::Renderer,
                    access: AgentAccess::SharedWorld,
                    role: Role::SharedWorldReader,
                })),
                AgentImportance::Critical,
                1.0,
                vec![],
            ),
            (
                Arc::new(Mutex::new(MockAgent {
                    id: AgentId::Audio,
                    access: AgentAccess::Isolated,
                    role: Role::IsolatedWriter,
                })),
                AgentImportance::Critical,
                0.5,
                vec![],
            ),
        ];

        scheduler.execute_agents_parallel(agents, &mut world, &runtime, &completion, &bus, &mut deck);

        assert!(
            deck.take::<SharedSaw>().0,
            "SharedWorld agent must reach the shared &World on its worker thread"
        );
        let iso = deck.take::<IsoRan>();
        assert!(iso.ran, "Isolated agent must run");
        assert!(
            iso.world_was_none,
            "Isolated agent must be granted no world access"
        );
        assert_eq!(
            completion.outcome(AgentId::Renderer),
            Some(CompletionOutcome::Completed)
        );
        assert_eq!(
            completion.outcome(AgentId::Audio),
            Some(CompletionOutcome::Completed)
        );
    }
}

#[cfg(test)]
mod sim_step_tests {
    use super::{compute_sim_steps, MAX_SIM_STEPS};

    const FIXED: f32 = 1.0 / 60.0;

    #[test]
    fn exact_multiple_runs_whole_steps_no_remainder() {
        // Two full steps' worth of time, nothing carried over.
        let r = compute_sim_steps(0.0, 2.0 * FIXED, FIXED, MAX_SIM_STEPS);
        assert_eq!(r.steps, 2);
        assert!(r.new_accumulator.abs() < 1e-6, "no remainder expected");
        assert!(r.alpha.abs() < 1e-6);
    }

    #[test]
    fn fractional_carry_advances_remainder() {
        // 2.5 steps → 2 steps run, half a step carried.
        let r = compute_sim_steps(0.0, 2.5 * FIXED, FIXED, MAX_SIM_STEPS);
        assert_eq!(r.steps, 2);
        assert!((r.new_accumulator - 0.5 * FIXED).abs() < 1e-6);
        assert!((r.alpha - 0.5).abs() < 1e-4);
    }

    #[test]
    fn dt_below_fixed_delta_runs_no_step() {
        let r = compute_sim_steps(0.0, 0.5 * FIXED, FIXED, MAX_SIM_STEPS);
        assert_eq!(r.steps, 0);
        assert!((r.new_accumulator - 0.5 * FIXED).abs() < 1e-6);
        assert!((r.alpha - 0.5).abs() < 1e-4);
    }

    #[test]
    fn accumulator_from_prior_frame_is_included() {
        // 0.7 carried + 0.6 this frame = 1.3 steps → 1 step, 0.3 carry.
        let r = compute_sim_steps(0.7 * FIXED, 0.6 * FIXED, FIXED, MAX_SIM_STEPS);
        assert_eq!(r.steps, 1);
        assert!((r.new_accumulator - 0.3 * FIXED).abs() < 1e-5);
    }

    #[test]
    fn spiral_clamp_caps_steps_and_bounds_accumulator() {
        // A huge dt (100 steps' worth) must clamp to MAX_SIM_STEPS and the
        // accumulator must stay bounded (excess time dropped).
        let r = compute_sim_steps(0.0, 100.0 * FIXED, FIXED, MAX_SIM_STEPS);
        assert_eq!(r.steps, MAX_SIM_STEPS);
        assert!(
            r.new_accumulator < FIXED,
            "accumulator must be bounded below one step, got {}",
            r.new_accumulator
        );
        assert!((0.0..1.0).contains(&r.alpha));
    }

    #[test]
    fn alpha_always_in_unit_interval() {
        for k in 0..400u32 {
            let dt = (k as f32) * 0.001;
            let r = compute_sim_steps(0.0, dt, FIXED, MAX_SIM_STEPS);
            assert!(
                (0.0..1.0).contains(&r.alpha),
                "alpha out of range for dt={dt}: {}",
                r.alpha
            );
        }
    }

    #[test]
    fn determinism_same_total_time_same_step_count() {
        // Cadence A: ten frames of 1.5 fixed-steps each (144 Hz-ish bursts).
        let mut acc_a = 0.0;
        let mut steps_a = 0u32;
        for _ in 0..10 {
            let r = compute_sim_steps(acc_a, 1.5 * FIXED, FIXED, MAX_SIM_STEPS);
            acc_a = r.new_accumulator;
            steps_a += r.steps;
        }
        // Cadence B: five frames of 3.0 fixed-steps each (30 Hz). Same total
        // simulated time (15 fixed steps) split differently.
        let mut acc_b = 0.0;
        let mut steps_b = 0u32;
        for _ in 0..5 {
            let r = compute_sim_steps(acc_b, 3.0 * FIXED, FIXED, MAX_SIM_STEPS);
            acc_b = r.new_accumulator;
            steps_b += r.steps;
        }
        assert_eq!(
            steps_a, steps_b,
            "same total sim time must yield the same total steps regardless of frame cadence"
        );
        assert_eq!(steps_a, 15);
    }

    #[test]
    fn degenerate_fixed_delta_runs_single_step() {
        let r = compute_sim_steps(0.0, 0.016, 0.0, MAX_SIM_STEPS);
        assert_eq!(r.steps, 1);
        assert_eq!(r.alpha, 0.0);
    }
}
