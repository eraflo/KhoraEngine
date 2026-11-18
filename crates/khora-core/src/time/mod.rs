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

//! The per-frame [`Time`] resource, the engine's single clock: the frame's
//! simulated delta, the fixed tick and the render-interpolation factor.
//!
//! See the book: concepts/the-frame.md, "Fixed-timestep simulation and render interpolation".

use std::sync::{Arc, RwLock};

/// The game's fixed tick — 60 Hz. The clock a script's `FixedUpdate` runs on
/// and the simulated time a frame advances in, whatever step physics takes.
pub const DEFAULT_FIXED_DELTA_SECONDS: f32 = 1.0 / 60.0;

/// Upper bound on the fixed ticks one frame runs, and a script's `FixedUpdate` may be owed.
///
/// 83 ms of simulated time, the same under every physics precision.
pub const MAX_SIM_STEPS: u32 = 5;

/// Per-frame timing state shared between the simulation and the render path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Time {
    /// Simulated seconds since the previous frame: the delta for variable-rate logic.
    ///
    /// The wall-clock time between the two frame starts, clamped after a long
    /// stall (a debugger break, an asset hitch) to avoid a spiral of death, then
    /// multiplied by the [`scale`](Self::scale): `0.0` while the simulation is
    /// stopped. The first frame advances one fixed tick.
    pub delta_seconds: f32,
    /// The game's fixed tick in seconds, what a `FixedUpdate` is given; 1/60 by default.
    ///
    /// Not the physics step: physics sub-steps the ticks' time at its own precision.
    pub fixed_delta_seconds: f32,
    /// Render-interpolation factor in `[0, 1)`: the fraction of a tick past the last whole one.
    ///
    /// Render-only; it never affects the simulation.
    pub interpolation_alpha: f32,
    /// Monotonic frame counter, incremented once per rendered frame.
    pub frame: u64,
    /// The fixed ticks this frame took, written by the scheduler.
    pub fixed_steps: u32,
    /// How fast the simulated world runs, against the wall clock.
    ///
    /// `1.0` is real time. `0.0` stops the simulation, not the engine: the
    /// renderer, the UI and input read the wall clock. A game's pause menu,
    /// slow motion or fast-forward, not an editor concept. Private so
    /// [`set_scale`](Self::set_scale), which clamps negatives, is the only way in.
    scale: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            fixed_delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            interpolation_alpha: 0.0,
            frame: 0,
            fixed_steps: 0,
            scale: 1.0,
        }
    }
}

impl Time {
    /// Creates a `Time` with the given fixed step and otherwise default values.
    #[must_use]
    pub fn with_fixed_delta(fixed_delta_seconds: f32) -> Self {
        Self {
            fixed_delta_seconds,
            ..Self::default()
        }
    }

    /// Returns how fast the simulated world runs. `1.0` is real time, `0.0` is stopped.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Sets the scale, clamping a negative one, or NaN, to `0.0`.
    ///
    /// Every integrator assumes time moves one way. Clamped rather than
    /// rejected, so a scale computed from a curve needs no guard.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.0);
    }

    /// Returns whether the simulated world is advancing at all.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.scale > 0.0
    }
}

/// Shared, interior-mutable handle to the engine's [`Time`] resource.
///
/// Registered in [`Runtime::resources`](crate::Runtime) under this type: the
/// scheduler writes it once a frame, Flows and game code read it.
pub type SharedTime = Arc<RwLock<Time>>;

#[cfg(test)]
mod tests;
