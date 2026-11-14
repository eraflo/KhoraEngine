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

//! The per-frame entry point: the frame start, then the frame's steps in
//! order — clocks, completion map, wave plan, bus, event pass, background
//! starts, the phases' waves, and the deck handed off.

use super::known::MetAgent;
use super::layers::{swap_layers, wake_agents};
use super::{AgentSelection, ExecutionScheduler, COMPONENT_ACCESS_SAMPLE_PERIOD};
use crate::substrate;
use khora_core::agent::completion::AgentCompletionMap;
use khora_core::agent::completion::CompletionOutcome;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Activation, Agent, ExecutionPhase};
use khora_core::lane::{HostViews, LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use khora_data::ecs::World;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
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
        let inactive = inactive_families(runtime);
        // The runs that ended since come back first: a layer staged while its
        // family's run was out swaps in now that it is home.
        for (report, ms) in self.background.finish(runtime) {
            self.publish_report(Some(report), ms);
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
    ///    to [`MAX_FRAME_DELTA_SECONDS`](super::MAX_FRAME_DELTA_SECONDS)
    ///    (spiral-of-death guard), scale it by `Time::scale`, and add it to
    ///    `sim_accumulator`.
    /// 2. **The game's clock.** Consume whole ticks of
    ///    [`DEFAULT_FIXED_DELTA_SECONDS`](khora_core::time::DEFAULT_FIXED_DELTA_SECONDS):
    ///    `ticks = floor(accumulator / tick)`, capped at
    ///    [`MAX_SIM_STEPS`](super::sim_steps::MAX_SIM_STEPS); the
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
        // The frame start, unless the host began the frame itself.
        if !std::mem::take(&mut self.begun) {
            self.begin_frame(&runtime);
            self.begun = false;
        }
        self.frame_start = Instant::now();
        if let Ok(mut found) = self.undeclared_deck_writes.lock() {
            found.clear();
        }

        // Read the families off, once: the state in force the frame
        // boundary settled, which the Flows and DataSystems read too, so
        // the whole frame agrees. A runtime with no activation runs every
        // family.
        let inactive = inactive_families(&runtime);

        // The families, the frame start having lent and swapped.
        let met = self.met_agents(&inactive);

        // Real frame delta + fixed-timestep accumulator bookkeeping, then
        // the per-frame Time resource (read by Flows + game code).
        let fixed_rounds = self.advance_clocks(&runtime, &inactive);

        // Build the per-frame completion map. The scheduler tracks
        // completion internally — agents do not read it through the
        // runtime containers.
        let completion_map = self.completion_map();

        // Publish how agents will be grouped into concurrent waves this
        // frame, so the DCC can budget each wave by its critical path.
        self.emit_wave_plan(&inactive);

        // Build the per-frame substrate: typed input bus and output deck.
        // The deck is moved into the scheduler's `last_deck` slot at the
        // end of the frame so the engine I/O layer can drain it.
        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        // Run the Substrate Pass: every registered Flow projects its View
        // into the bus. Flows are read-only projectors — no budget needed
        // (only agents compete for the frame budget).
        substrate::run_flows(world, &mut bus, &runtime);
        // Then the Views no Flow produces: the ones the host application set.
        if let Some(host_views) = runtime.resources.get::<Arc<HostViews>>() {
            host_views.publish_into(&mut bus);
        }
        before_agents(&bus);

        // The event pass: every agent whose place gathered a wake is woken
        // for it, once the bus is built and before any layer runs. A quiet
        // agent is neither called nor locked.
        wake_agents(&met, &runtime, &bus);

        // Freeze the bus behind an `Arc` for the rest of the frame. It is
        // strictly read-only from here on (the CLAD descent only reads Views),
        // and the worker pool needs an owned `Arc<LaneBus>` to make a concurrent
        // wave's jobs `'static`. A wave of one derefs it to `&LaneBus`.
        let bus = Arc::new(bus);

        // The families at the background with mail start a run off the
        // frame, on this frame's bus; nothing waits for it.
        self.background.start(&met, &runtime, &bus);
        // The layers the event pass asked to prepare start too, beside
        // the installed ones, which keep running until they land.
        self.background.prepare(&met, &runtime, &bus);

        // The waves: the fixed-update sub-loop, then every phase once.
        self.run_phases(
            &runtime,
            &inactive,
            &completion_map,
            &bus,
            &mut deck,
            fixed_rounds,
        );

        // Hand the populated deck off to the engine for the I/O boundary.
        self.last_deck = deck;

        // Low-rate observation tunnel.
        self.sample_component_access(world);
    }

    /// The per-frame completion map over every registered agent. A fixed
    /// agent its clock owes no sub-step this frame has done all it owes:
    /// completed, so an agent depending on it — a finer-stepped one between
    /// two of its steps — still runs, rather than being skipped as if it had
    /// failed.
    fn completion_map(&self) -> Arc<AgentCompletionMap> {
        let agent_ids = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all_ids();
        let completion_map = Arc::new(AgentCompletionMap::new(&agent_ids));
        for (&id, &due) in &self.fixed_due {
            if due == 0 {
                completion_map.mark(id, CompletionOutcome::Completed);
            }
        }
        completion_map
    }

    /// Runs the frame's phases, each family's installed layer at its place.
    /// When a place holds a fixed step (`fixed_rounds` is `Some`), the fixed
    /// agents first advance their sub-steps, round by round, and are then
    /// left out of the once-per-frame loop.
    fn run_phases(
        &mut self,
        runtime: &Arc<Runtime>,
        inactive: &BTreeSet<AgentId>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
        fixed_rounds: Option<u32>,
    ) {
        // Fixed-update sub-loop. When a place holds a fixed step, the
        // fixed-step agents advance their discrete sub-steps before the
        // once-per-frame phase loop, round by round — each round runs every
        // fixed agent still owed one. Each sub-step is a full agent
        // invocation, so the physics provider integrates N times while the
        // render pass below fires once. When there is no fixed agent, every
        // agent runs in its normal phase below — so this loop does nothing.
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();
        if let Some(rounds) = fixed_rounds {
            for round in 0..rounds {
                for &phase in &phases {
                    self.execute_agents_in_phase(
                        phase,
                        runtime,
                        inactive,
                        completion_map,
                        bus,
                        deck,
                        AgentSelection::FixedRound(round),
                    );
                }
            }
        }

        // Once-per-frame agents. When the sim was sub-stepped above, fixed
        // agents are excluded here so they are not run an extra time; with
        // no fixed agent, every agent runs through this `All` path.
        let selection = if fixed_rounds.is_some() {
            AgentSelection::ExcludeFixed
        } else {
            AgentSelection::All
        };

        // Execute each phase: each family's installed layer runs at its
        // place.
        for phase in phases {
            self.execute_agents_in_phase(
                phase,
                runtime,
                inactive,
                completion_map,
                bus,
                deck,
                selection,
            );
        }
    }

    /// Publishes per-component access snapshots so the DCC's layout advisor
    /// can recommend layouts. Sampled every `COMPONENT_ACCESS_SAMPLE_PERIOD`
    /// frames (the counters are cumulative and move slowly), and best-effort
    /// (dropped if the channel is full).
    fn sample_component_access(&mut self, world: &World) {
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
}

/// The agents whose family is off, as the runtime's [`Activation`] holds
/// them; none without one.
fn inactive_families(runtime: &Runtime) -> BTreeSet<AgentId> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .map(|activation| activation.inactive())
        .unwrap_or_default()
}
