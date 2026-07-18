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
use crossbeam_channel::Sender;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, EngineMode, ExecutionPhase};
use khora_core::control::gorna::AgentId;
use khora_core::graph::topological_sort;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
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
        }
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

        // 4. Build the per-frame substrate: typed input bus and output deck.
        //    The deck is moved into the scheduler's `last_deck` slot at the
        //    end of the frame so the engine I/O layer can drain it.
        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        // 5. Run the Substrate Pass: every registered Flow projects its View
        //    into the bus. Flows are read-only projectors — no budget needed
        //    (only agents compete for the frame budget).
        substrate::run_flows(world, &mut bus, &runtime);

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
        bus: &LaneBus,
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
        self.execute_agents_sequential(sorted, world, runtime, completion_map, bus, deck);
    }

    /// Executes the phase's agents **sequentially, in priority order** (the
    /// [`sort_agents`] output). GORNA budgets are therefore per-agent
    /// *exclusive time slices of the frame*, not concurrent allocations:
    /// measured agent costs add up (T1 + T2 + …), which is exactly what the
    /// arbitrator's budget fitting assumes when it sums estimated times
    /// against the frame budget. When parallel execution lands
    /// ([`execute_agents_parallel`](Self::execute_agents_parallel), today an
    /// `unimplemented!` stub), the fitting must switch from sum-of-costs to a
    /// critical-path model.
    fn execute_agents_sequential(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &LaneBus,
        deck: &mut OutputDeck,
    ) {
        // Coarse workload size for the cost model — sampled once per phase
        // (per-domain refinement is a later step).
        let workload_n = world.entity_count() as f64;

        for (agent, importance, _priority, dependencies) in agents {
            let agent_id = match agent.lock().ok().map(|a| a.id()) {
                Some(id) => id,
                None => continue,
            };

            // Budget escape valve: only *negotiable* (Optional) work is skipped
            // under pressure. Critical/Important agents are non-negotiable.
            if importance.is_negotiable() && self.is_under_budget_pressure() {
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

            let started = Instant::now();
            if let Ok(mut a) = agent.lock() {
                a.execute(&mut engine_ctx);
            }
            // Observation tunnel: report (n, time) so the DCC can fit this
            // agent's cost model and forecast budget breaches. Best-effort.
            if let Some(tx) = &self.telemetry {
                let _ = tx.try_send(TelemetryEvent::AgentCost {
                    id: agent_id,
                    n: workload_n,
                    time_ms: started.elapsed().as_secs_f64() * 1000.0,
                });
            }
            completion_map.mark(agent_id, CompletionOutcome::Completed);
        }
    }

    /// Parallel execution path — roadmap stub, not yet implemented.
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
