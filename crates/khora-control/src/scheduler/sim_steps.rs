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

//! Fixed-timestep arithmetic: how many game ticks a frame runs.

pub(super) use super::sub_steps::compute_sub_steps;

/// The most game ticks a frame runs; the excess time is dropped.
///
/// The engine's one cap: the script lane bounds the steps it owes by it too.
pub(super) const MAX_SIM_STEPS: u32 = khora_core::time::MAX_SIM_STEPS;

/// Returns the whole ticks of `fixed_delta` a frame runs, the time carried over, and the
/// render alpha in `[0, 1)`.
///
/// `dt` is already clamped. Past `max_steps` the excess time is dropped, so the
/// accumulator stays bounded; a non-finite total saturates the same way and carries
/// nothing, so no `NaN` can stop the clock. A non-positive `fixed_delta` runs one step and
/// carries nothing.
pub(super) fn compute_sim_steps(
    accumulator: f32,
    dt: f32,
    fixed_delta: f32,
    max_steps: u32,
) -> SimSteps {
    if fixed_delta <= 0.0 {
        return SimSteps {
            steps: 1,
            new_accumulator: 0.0,
            alpha: 0.0,
        };
    }

    let mut acc = accumulator + dt;
    if !acc.is_finite() {
        return SimSteps {
            steps: max_steps,
            new_accumulator: 0.0,
            alpha: 0.0,
        };
    }
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

    // Float subtraction can leave a tiny negative residue.
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
pub(super) struct SimSteps {
    pub(super) steps: u32,
    pub(super) new_accumulator: f32,
    pub(super) alpha: f32,
}
