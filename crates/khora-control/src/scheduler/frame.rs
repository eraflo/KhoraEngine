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

//! The per-frame entry point: fixed-timestep accounting, the phase loop,
//! and publishing the frame's `Time`.

use super::execution::run_guarded;
use super::known::{locked, MetAgent};
use super::sim_steps::{compute_sim_steps, compute_sub_steps, MAX_SIM_STEPS};
use super::{
    simulation_scale, AgentSelection, ExecutionScheduler, COMPONENT_ACCESS_SAMPLE_PERIOD,
    MAX_FRAME_DELTA_SECONDS,
};
use crate::substrate;
use khora_core::agent::completion::AgentCompletionMap;
use khora_core::agent::completion::CompletionOutcome;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Activation, Agent, ExecutionPhase};
use khora_core::gpu::GpuSetup;
use khora_core::lane::{HostViews, LaneBus, OutputDeck};
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Instant;

impl ExecutionScheduler {
    /// The frame start, before any DataSystem of the frame runs: the budget
    /// the DCC published is taken, the allocation in force is lent, every
    /// family's staged layer is swapped in, and the background runs that
    /// ended since are returned — their layers to their places, what they
    /// staged to the runtime's [`Reserve`](khora_core::lane::Reserve). The
    /// host calls it once the families on and off are settled, before the
    /// frame's `Boundary` systems; [`run_frame`](Self::run_frame) then runs
    /// the frame on what it fixed. A frame [`run_frame`](Self::run_frame)
    /// finds not begun, it begins itself.
    pub fn begin_frame(&mut self, runtime: &Runtime) {
        // The budget the DCC published since the last frame.
        if let Some(next) = self.budget_channel.take() {
            self.taken.push(next);
        }
        let inactive = runtime
            .resources
            .get::<Arc<Activation>>()
            .map(|activation| activation.inactive())
            .unwrap_or_default();
        // The runs that ended since come back first: a layer staged while its
        // family's run was out swaps in now that it is home.
        for (report, ms) in self.background.finish(runtime) {
            self.publish_report(Some(report), ms, 0.0);
        }
        // And the layers prepared off the frame, in time for this frame
        // start's swap; a failed one wakes its agent at the event pass.
        self.background.take_prepared();
        // Then what the host staged since: later than any run that ended.
        if let Some(reserve) = runtime.look_up::<khora_core::lane::Reserve>() {
            reserve.land_from_host();
        }
        // The allocation in force is lent before anything locks an agent: the
        // whole frame runs on one epoch. Then every family has the layer
        // staged at its place swapped in, at the same place.
        self.lend_allocation(&inactive);
        let met = self.met_agents(&inactive);
        swap_layers(&met, runtime);
        self.begun = true;
    }

    /// Executes the complete frame cycle.
    ///
    /// This is called every frame by the engine loop.
    ///
    /// ## Fixed-timestep sequencing
    ///
    /// Rendering runs at the display's variable rate, but the simulation must
    /// advance in fixed increments to stay frame-rate independent and
    /// deterministic. The scheduler reconciles the two with two clocks:
    ///
    /// 1. Measure the real wall-clock delta since the previous frame, clamped
    ///    to [`MAX_FRAME_DELTA_SECONDS`] (spiral-of-death guard), scale it by
    ///    `Time::scale`, and add it to `sim_accumulator`.
    /// 2. **The game's clock.** Consume whole ticks of
    ///    [`DEFAULT_FIXED_DELTA_SECONDS`](khora_core::time::DEFAULT_FIXED_DELTA_SECONDS):
    ///    `ticks = floor(accumulator / tick)`, capped at [`MAX_SIM_STEPS`]; the
    ///    remainder carries over and yields the render `interpolation_alpha`.
    ///    This is the only clock game code sees, and nothing below changes it.
    /// 3. **The fixed agents' sub-clocks.** Each agent whose place runs at
    ///    a fixed step — in practice the physics family, at the game's
    ///    `physics.step` — has its own clock: the time the ticks
    ///    advanced, `ticks × tick`, goes into it, and the agent runs once per
    ///    whole step of its own that fits — a fixed-update sub-loop — the
    ///    remainder carried (also across a change of step). Two agents with
    ///    different steps each see the game's time, cut differently. A frame
    ///    that took no tick runs no sub-step, so a paused world stays still
    ///    even when a step changes under it. Then the regular phase loop runs
    ///    **once**, excluding the agents already stepped, so OUTPUT-phase render
    ///    fires a single time. If no place holds a step, every agent runs
    ///    once a frame.
    /// 4. Publish the fresh `Time` (delta, tick, ticks, alpha) into the runtime
    ///    resource before the render phase reads it.
    ///
    /// Keeping the step off the game's clock is what makes it a precision: a
    /// coarser or finer step changes how a second is cut, never how many
    /// seconds pass, how often a script's `FixedUpdate` runs, or how much
    /// catch-up the cap allows.
    ///
    /// Substrate Flows project once per frame; the GORNA completion map,
    /// budget arbitration, and telemetry all observe each individual agent
    /// invocation (a sub-step counts as a real run, with its own cost sample).
    pub fn run_frame(&mut self, world: &mut World, runtime: Arc<Runtime>) {
        self.run_frame_with(world, runtime, |_| {});
    }

    /// [`run_frame`](Self::run_frame), handing `before_agents` the frame's
    /// bus once the Flows and the host's Views are on it, before any agent
    /// runs: the host's own work from the frame's Views. The SDK prepares the
    /// renderer from the camera View there, whatever agents run.
    pub fn run_frame_with(
        &mut self,
        world: &mut World,
        runtime: Arc<Runtime>,
        before_agents: impl FnOnce(&LaneBus),
    ) {
        // 1. The frame start, unless the host began the frame itself.
        if !std::mem::take(&mut self.begun) {
            self.begin_frame(&runtime);
            self.begun = false;
        }
        self.frame_start = Instant::now();
        if let Ok(mut found) = self.undeclared_deck_writes.lock() {
            found.clear();
        }

        // 1a. Read the families off, once: the state in force the frame
        //     boundary settled, which the Flows and DataSystems read too, so
        //     the whole frame agrees. A runtime with no activation runs every
        //     family.
        let inactive = runtime
            .resources
            .get::<Arc<Activation>>()
            .map(|activation| activation.inactive())
            .unwrap_or_default();

        // 1a-bis. The families, the frame start having lent and swapped.
        let met = self.met_agents(&inactive);

        // 1b. Real frame delta + fixed-timestep accumulator bookkeeping.
        let now = self.frame_start;
        let tick = khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;
        let fixed_steps = self.declared_fixed_steps(&inactive);
        let dt = match self.last_frame_instant {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            // First frame: advance by exactly one tick (no real history).
            None => tick,
        }
        .min(MAX_FRAME_DELTA_SECONDS);
        self.last_frame_instant = Some(now);

        // The simulated world's clock, which is the wall clock times whatever
        // the game asked for. At `0.0` the accumulator below never fills, so
        // `sim_steps` is zero: no body integrates, no script timer counts down,
        // and the renderer carries on at real time because it does not read
        // this. That is what a pause menu is, what a slow-motion hit is — and
        // what the editor sets while nobody has pressed Play.
        //
        // Applied here rather than per agent: an agent deciding for itself
        // whether time passes would be a dozen places to disagree about what
        // "paused" means.
        let dt = dt * simulation_scale(&runtime);

        // The game's clock: whole ticks, capped, on the scaled clock — no tick
        // while paused. What a script's `FixedUpdate` is called for, whether or
        // not any agent sub-steps.
        let ticks = compute_sim_steps(self.sim_accumulator, dt, tick, MAX_SIM_STEPS);
        self.sim_accumulator = ticks.new_accumulator;

        // The fixed agents' sub-clocks: exactly the time the ticks advanced,
        // each cut at its agent's own step. A frame with no tick leaves them
        // untouched; an agent whose place no longer holds a step loses its
        // clock.
        self.fixed_clocks
            .retain(|id, _| fixed_steps.iter().any(|(fixed, _)| fixed == id));
        self.fixed_due.clear();
        let advanced = ticks.steps as f32 * tick;
        for &(id, step) in &fixed_steps {
            let clock = self.fixed_clocks.entry(id).or_insert(0.0);
            let due = if ticks.steps > 0 {
                let r = compute_sub_steps(*clock, advanced, step);
                *clock = r.new_accumulator;
                r.steps
            } else {
                0
            };
            self.fixed_due.insert(id, due);
        }
        let rounds = self.fixed_due.values().copied().max().unwrap_or(0);

        // 1c. Publish the per-frame Time resource (read by Flows + game code).
        self.publish_time(&runtime, dt, (tick, ticks.steps), ticks.alpha);

        // 2. Build the per-frame completion map. The scheduler tracks
        //    completion internally — agents do not read it through the
        //    runtime containers.
        let agent_ids = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all_ids();
        let completion_map = Arc::new(AgentCompletionMap::new(&agent_ids));

        // A fixed agent its clock owes no sub-step this frame has done all it
        // owes: completed, so an agent depending on it — a finer-stepped one
        // between two of its steps — still runs, rather than being skipped as
        // if it had failed.
        for (&id, &due) in &self.fixed_due {
            if due == 0 {
                completion_map.mark(id, CompletionOutcome::Completed);
            }
        }

        // 3. Publish how agents will be grouped into concurrent waves this
        //    frame, so the DCC can budget each wave by its critical path.
        self.emit_wave_plan(&inactive);

        // 4. Build the per-frame substrate: typed input bus and output deck.
        //    The deck is moved into the scheduler's `last_deck` slot at the
        //    end of the frame so the engine I/O layer can drain it.
        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        // 5. Run the Substrate Pass: every registered Flow projects its View
        //    into the bus. Flows are read-only projectors — no budget needed
        //    (only agents compete for the frame budget).
        substrate::run_flows(world, &mut bus, &runtime);
        // Then the Views no Flow produces: the ones the host application set.
        if let Some(host_views) = runtime.resources.get::<Arc<HostViews>>() {
            host_views.publish_into(&mut bus);
        }
        before_agents(&bus);

        // 5b. The event pass: every agent whose place gathered a wake is woken
        //     for it, once the bus is built and before any layer runs. A quiet
        //     agent is neither called nor locked.
        wake_agents(&met, &runtime, &bus);

        // Freeze the bus behind an `Arc` for the rest of the frame. It is
        // strictly read-only from here on (the CLAD descent only reads Views),
        // and the worker pool needs an owned `Arc<LaneBus>` to make a concurrent
        // wave's jobs `'static`. A wave of one derefs it to `&LaneBus`.
        let bus = Arc::new(bus);

        // 5c. The families at the background with mail start a run off the
        //     frame, on this frame's bus; nothing waits for it.
        self.background.start(&met, &runtime, &bus);
        // 5d. The layers the event pass asked to prepare start too, beside
        //     the installed ones, which keep running until they land.
        self.background.prepare(&met, &runtime, &bus);

        // 6. Fixed-update sub-loop. When a place holds a fixed step, the
        //    fixed-step agents advance their discrete sub-steps before the
        //    once-per-frame phase loop, round by round — each round runs every
        //    fixed agent still owed one. Each sub-step is a full agent
        //    invocation, so the physics provider integrates N times while the
        //    render pass below fires once. When there is no fixed agent, every
        //    agent runs in its normal phase below — so this loop does nothing.
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();
        let has_fixed = !fixed_steps.is_empty();
        if has_fixed {
            for round in 0..rounds {
                for &phase in &phases {
                    self.execute_agents_in_phase(
                        phase,
                        world.entity_count() as f64,
                        &runtime,
                        &inactive,
                        &completion_map,
                        &bus,
                        &mut deck,
                        AgentSelection::FixedRound(round),
                    );
                }
            }
        }

        // Once-per-frame agents. When the sim was sub-stepped above, fixed
        // agents are excluded here so they are not run an extra time; with no
        // fixed agent, every agent runs through this `All` path as before.
        let selection = if has_fixed {
            AgentSelection::ExcludeFixed
        } else {
            AgentSelection::All
        };

        // 8. Execute each phase: plugins then agents (CLAD descent — each
        //    family's installed layer runs at its place).
        for phase in phases {
            for plugin in &mut self.plugins {
                if plugin.wants_phase(phase) {
                    plugin.execute(phase, world);
                }
            }

            self.execute_agents_in_phase(
                phase,
                world.entity_count() as f64,
                &runtime,
                &inactive,
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

    /// Lends every agent of a family on what it is owed, under its lock, at the
    /// frame start: each replayed allocation taken since it was last handed
    /// one, in order, then the one in force — which the whole frame runs on.
    /// The DCC never lends. An agent busy now (asked for an offer) is not
    /// waited for: it runs this frame on what it holds, and is handed what it
    /// missed at the next frame start. An agent whose family is off is lent
    /// once it runs. An allocation that does not name an agent leaves it with
    /// what it holds.
    fn lend_allocation(&mut self, inactive: &BTreeSet<AgentId>) {
        let Some(current) = self.taken.last().map(|((place, _), _)| *place) else {
            return;
        };
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        // Agents removed since are forgotten.
        self.lent.retain(|(agent, _)| agent.strong_count() > 0);
        // The oldest place an agent of a family on was handed: what was
        // taken before it is owed to none of them any more.
        let mut oldest_handed = Some(current);
        for agent_mutex in &agents {
            let weak = Arc::downgrade(agent_mutex);
            let entry = self.lent.iter().position(|(seen, _)| seen.ptr_eq(&weak));
            let handed = entry.map(|at| self.lent[at].1);
            if handed == Some(current) {
                continue;
            }
            let mut agent = match agent_mutex.try_lock() {
                Ok(agent) => agent,
                Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    oldest_handed = oldest_handed.min(handed);
                    continue;
                }
            };
            let owed = self
                .taken
                .iter()
                .filter(|((place, _), _)| handed.is_none_or(|handed| *place > handed));
            for ((place, allocation), in_turn) in owed {
                if *in_turn || *place == current {
                    allocation.lend_to(&mut *agent);
                }
            }
            match entry {
                Some(at) => self.lent[at].1 = current,
                None => self.lent.push((weak, current)),
            }
        }
        // `None` is an agent never handed anything: it is owed all that is
        // kept.
        if let Some(oldest) = oldest_handed {
            self.taken
                .retain(|((place, _), _)| *place > oldest || *place == current);
        }
    }

    /// Every agent of a family on whose place runs at a fixed step, with that
    /// step in seconds — read from the place each frame, so a step the game's
    /// setting changed takes effect on the next frame. An agent whose family is off
    /// keeps no clock: it owes nothing for the time it sat out.
    fn declared_fixed_steps(&self, inactive: &BTreeSet<AgentId>) -> Vec<(AgentId, f32)> {
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        agents
            .iter()
            .filter_map(|agent| {
                let known = self.known.of(agent)?;
                let step = known.fixed_step()?.as_secs_f32();
                (step > 0.0).then_some((known.id, step))
            })
            .collect()
    }

    /// Every agent of a family on, with its lock and what the scheduler knows
    /// of it.
    fn met_agents(&self, inactive: &BTreeSet<AgentId>) -> Vec<MetAgent> {
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        agents
            .into_iter()
            .filter_map(|agent| {
                let known = self.known.of(&agent)?;
                Some((agent, known))
            })
            .collect()
    }

    /// Writes the per-frame [`Time`](khora_core::time::Time) into the runtime
    /// resource so Flows and game `update` read the real delta + alpha.
    /// No-op if no `SharedTime` resource is registered.
    fn publish_time(
        &self,
        runtime: &Runtime,
        dt: f32,
        (fixed_delta, fixed_steps): (f32, u32),
        alpha: f32,
    ) {
        let Some(shared) = runtime.resources.get::<khora_core::time::SharedTime>() else {
            return;
        };
        if let Ok(mut time) = shared.write() {
            time.delta_seconds = dt;
            time.fixed_delta_seconds = fixed_delta;
            time.fixed_steps = fixed_steps;
            time.interpolation_alpha = alpha;
            time.frame = time.frame.wrapping_add(1);
        }
    }
}

/// The device and pipeline system a layer is released with, as the runtime
/// holds them; headless without a device.
fn gpu_of(runtime: &Runtime) -> GpuSetup<'_> {
    let mut gpu = match runtime.look_up::<Arc<dyn GraphicsDevice>>() {
        Some(device) => GpuSetup::new(device.as_ref()),
        None => GpuSetup::headless(),
    };
    if let Some(pipelines) = runtime.look_up::<Arc<dyn PipelineSystem>>() {
        gpu = gpu.with_pipelines(pipelines.as_ref());
    }
    gpu
}

/// The frame start of each family's place: its staged layer is swapped in,
/// and the kept layers it can no longer keep are released. A layer that
/// panics while released fails its own family's frame start, not the frame.
fn swap_layers(met: &[MetAgent], runtime: &Runtime) {
    let mut gpu = gpu_of(runtime);
    for (_, known) in met {
        let swapped = run_guarded(known.id, || locked(&known.place).start_frame(&mut gpu));
        if !swapped {
            known.place.clear_poison();
        }
    }
}

/// Wakes every agent whose place gathered a wake, under its lock and then
/// its place's — the one order both are ever taken in — with the permit of
/// its own contention: what it quotes and prepares with. It touches no
/// world, and writes nothing a later agent reads.
fn wake_agents(met: &[MetAgent], runtime: &Arc<Runtime>, bus: &LaneBus) {
    for (agent, known) in met {
        let id = &known.id;
        let why = locked(&known.place).take_wakes();
        if why.is_empty() {
            continue;
        }
        // Waking runs lane code — quotes, the preparation of a new layer —
        // so a panic there fails this family's wake, not the frame: its
        // installed layer runs on it as on any frame.
        let woken = run_guarded(*id, || {
            let mut agent = agent.lock().unwrap_or_else(|e| e.into_inner());
            let permit = agent.contention();
            let mut scratch = OutputDeck::new();
            let mut context = EngineContext::for_agent(
                Arc::clone(runtime),
                bus,
                &mut scratch,
                &permit,
                Some(*id),
            );
            let mut place = locked(&known.place);
            agent.wake(&why, &mut place, &mut context);
        });
        if !woken {
            agent.clear_poison();
            known.place.clear_poison();
            // Handed back, so the next event pass wakes the agent for them
            // again: a panic once is not a silent death, nor a lost
            // allocation.
            let mut place = locked(&known.place);
            for wake in why {
                place.raise(wake);
            }
        }
    }
}
