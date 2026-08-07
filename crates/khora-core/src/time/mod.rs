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

//! The per-frame [`Time`] resource — the engine's single clock.
//!
//! `Time` carries the real wall-clock frame delta, the fixed simulation
//! step, and the render-interpolation factor. It is the bridge between the
//! decoupled fixed-timestep simulation and the variable-rate render:
//!
//! - The simulation advances in whole [`fixed_delta_seconds`](Time::fixed_delta_seconds)
//!   steps via an accumulator in the scheduler, so it is frame-rate
//!   independent (deterministic).
//! - Rendering happens once per frame; to stay smooth between sim steps the
//!   render path blends the previous and current transforms by
//!   [`interpolation_alpha`](Time::interpolation_alpha).
//!
//! The scheduler publishes a fresh `Time` each frame; game code and Flows
//! read it. It lives in [`Runtime::resources`](crate::Runtime) behind an
//! `Arc<RwLock<Time>>` so the scheduler (which holds `Arc<Runtime>`) can
//! write it while Flows and game `update` (which borrow `&Runtime`) read it.

use std::sync::{Arc, RwLock};

/// Default fixed simulation step — 60 Hz.
pub const DEFAULT_FIXED_DELTA_SECONDS: f32 = 1.0 / 60.0;

/// Per-frame timing state shared between the simulation and the render path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Time {
    /// Real wall-clock time elapsed since the previous frame, in seconds,
    /// clamped to a sane maximum to avoid a "spiral of death" after a long
    /// stall (debugger break, asset hitch). This is the delta game code
    /// should use for variable-rate logic.
    pub delta_seconds: f32,
    /// The fixed simulation step in seconds — the cadence the deterministic
    /// fixed-timestep agents (physics) advance at. Defaults to 1/60.
    pub fixed_delta_seconds: f32,
    /// Render-interpolation factor in `[0, 1)`: the fraction of a fixed step
    /// the accumulator carries past the last whole sim step. Render-only
    /// transform blending uses it; it never affects simulation semantics.
    pub interpolation_alpha: f32,
    /// Monotonic frame counter, incremented once per rendered frame.
    pub frame: u64,
    /// How fast the simulated world runs, against the wall clock.
    ///
    /// `1.0` is real time. `0.0` stops the simulation without stopping the
    /// engine: the fixed-step accumulator never fills, so no sub-step runs, no
    /// body integrates and no script timer counts down — while the renderer,
    /// the UI and input carry on at real time, because they read the wall clock
    /// and not this.
    ///
    /// **Not an editor concept.** It is the pause menu, the slow-motion hit,
    /// the fast-forward of a strategy game — things a game wants on its own.
    /// The editor happens to be one caller among them: it sets `0.0` while
    /// editing, which is why a body no longer falls before anybody pressed
    /// Play. Its own `PlayMode` stays where it belongs and never reaches the
    /// engine; what crosses is a number any game could set.
    ///
    /// Negative values are refused by [`set_scale`](Self::set_scale) — running
    /// a solver backwards is not a slower forward, and every integrator here
    /// assumes time moves one way.
    ///
    /// Private so [`set_scale`](Self::set_scale) is the only way in — a field
    /// anyone could assign is a field somebody assigns `-1.0` to.
    scale: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            fixed_delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            interpolation_alpha: 0.0,
            frame: 0,
            scale: 1.0,
        }
    }
}

impl Time {
    /// Creates a `Time` with the given fixed step and otherwise default
    /// values (zero real delta, zero alpha, frame 0).
    #[must_use]
    pub fn with_fixed_delta(fixed_delta_seconds: f32) -> Self {
        Self {
            fixed_delta_seconds,
            ..Self::default()
        }
    }

    /// How fast the simulated world runs. `1.0` is real time, `0.0` is stopped.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Sets it, refusing to run time backwards.
    ///
    /// A negative scale is not a slower forward: every integrator in the engine
    /// assumes time moves one way, and a solver run in reverse produces states
    /// no forward run could reach. Clamped rather than rejected, because a
    /// caller computing a scale from a curve should not have to guard the
    /// bottom of it.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.0);
    }

    /// Whether the simulated world is advancing at all.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.scale > 0.0
    }
}

/// Shared, interior-mutable handle to the engine's [`Time`] resource.
///
/// Registered in [`Runtime::resources`](crate::Runtime) under this type so
/// the scheduler can publish a fresh `Time` each frame (write lock) while
/// Flows and game code read it (read lock).
pub type SharedTime = Arc<RwLock<Time>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_60hz_fixed_step() {
        let t = Time::default();
        assert_eq!(t.fixed_delta_seconds, 1.0 / 60.0);
        assert_eq!(t.delta_seconds, 1.0 / 60.0);
        assert_eq!(t.interpolation_alpha, 0.0);
        assert_eq!(t.frame, 0);
    }

    #[test]
    fn with_fixed_delta_overrides_step_only() {
        let t = Time::with_fixed_delta(1.0 / 120.0);
        assert_eq!(t.fixed_delta_seconds, 1.0 / 120.0);
        assert_eq!(t.interpolation_alpha, 0.0);
        assert_eq!(t.frame, 0);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn a_fresh_clock_runs_at_real_time() {
        assert_eq!(Time::default().scale(), 1.0);
        assert!(Time::default().is_running());
    }

    /// **What stops a body falling in the editor.** Zero is not a special case
    /// in the scheduler — the accumulator simply never fills.
    #[test]
    fn a_scale_of_zero_stops_the_world() {
        let mut time = Time::default();
        time.set_scale(0.0);

        assert!(!time.is_running());
    }

    /// Slow motion and fast forward are the same knob, which is the reason it
    /// is a scale and not a boolean: a game wants these on its own, and the
    /// editor is one caller among them.
    #[test]
    fn a_scale_between_the_two_is_slow_motion() {
        let mut time = Time::default();
        time.set_scale(0.25);

        assert_eq!(time.scale(), 0.25);
        assert!(time.is_running());
    }

    /// Running a solver backwards is not a slower forward. Every integrator
    /// here assumes time moves one way, and a reversed step produces states no
    /// forward run could reach.
    #[test]
    fn time_refuses_to_run_backwards() {
        let mut time = Time::default();
        time.set_scale(-1.0);

        assert_eq!(time.scale(), 0.0);
    }
}
