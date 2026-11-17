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

//! The frame's clocks: the real delta, the game's fixed ticks, the fixed
//! agents' sub-clocks, and the `Time` published for the frame.

use super::sim_steps::{compute_sim_steps, compute_sub_steps, MAX_SIM_STEPS};
use super::{simulation_scale, ExecutionScheduler, MAX_FRAME_DELTA_SECONDS};
use khora_core::agent::gorna::AgentId;
use khora_core::Runtime;

impl ExecutionScheduler {
    /// Advances the frame's clocks and publishes the frame's `Time`.
    ///
    /// Returns the most sub-steps any fixed agent is owed this frame, or `None` when no
    /// place holds a fixed step. See the book:
    /// concepts/the-frame.md, "Fixed-timestep simulation and render interpolation".
    pub(super) fn advance_clocks(&mut self, runtime: &Runtime) -> Option<u32> {
        let now = self.frame_start;
        let tick = khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;
        let fixed_steps = self.declared_fixed_steps();
        let dt = match self.last_frame_instant {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            // First frame: advance by exactly one tick (no real history).
            None => tick,
        }
        .min(MAX_FRAME_DELTA_SECONDS);
        self.last_frame_instant = Some(now);

        // Scaled once, here, never per agent.
        // See the book: concepts/the-frame.md, "The simulation clock".
        let dt = dt * simulation_scale(runtime);

        // The game's clock: whole ticks, capped, on the scaled clock — no tick
        // while paused. What a script's `FixedUpdate` is called for, whether or
        // not any agent sub-steps.
        let ticks = compute_sim_steps(self.sim_accumulator, dt, tick, MAX_SIM_STEPS);
        self.sim_accumulator = ticks.new_accumulator;

        // The fixed agents' sub-clocks: exactly the time the ticks advanced,
        // each cut at its agent's own step. A frame with no tick leaves them
        // untouched; an agent whose place holds no step any more loses its
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

        // Publish the per-frame Time resource (read by Flows + game code).
        self.publish_time(runtime, dt, (tick, ticks.steps), ticks.alpha);

        (!fixed_steps.is_empty()).then_some(rounds)
    }

    /// Returns each agent of a family on whose place runs at a fixed step, with the step in
    /// seconds.
    ///
    /// Read from the place each frame, so a changed step takes effect next frame. An agent
    /// whose family is off keeps no clock: it owes nothing for the time it sat out.
    fn declared_fixed_steps(&self) -> Vec<(AgentId, f32)> {
        (self.roster.met().iter())
            .filter_map(|(_, known)| {
                let step = known.fixed_step()?.as_secs_f32();
                (step > 0.0).then_some((known.id, step))
            })
            .collect()
    }

    /// Writes the frame's [`Time`](khora_core::time::Time) into the runtime's `SharedTime`,
    /// when it holds one.
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
