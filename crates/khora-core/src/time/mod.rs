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
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            fixed_delta_seconds: DEFAULT_FIXED_DELTA_SECONDS,
            interpolation_alpha: 0.0,
            frame: 0,
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
