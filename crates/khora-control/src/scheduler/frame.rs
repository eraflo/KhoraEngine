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

use super::sim_steps::{compute_sim_steps, SimSteps, MAX_SIM_STEPS};
use super::{
    simulation_scale, AgentSelection, ExecutionScheduler, COMPONENT_ACCESS_SAMPLE_PERIOD,
    MAX_FRAME_DELTA_SECONDS,
};
use crate::substrate;
use khora_core::agent::completion::AgentCompletionMap;
use khora_core::agent::ExecutionPhase;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use khora_data::ecs::World;
use std::sync::Arc;
use std::time::Instant;

impl ExecutionScheduler {
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
}
