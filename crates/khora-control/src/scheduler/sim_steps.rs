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

//! Fixed-timestep arithmetic: how many simulation sub-steps a frame runs.

/// Upper bound on fixed simulation sub-steps run in a single frame. When the
/// accumulator would demand more, the excess time is dropped (the sim runs
/// in slow-motion rather than freezing). Bounds worst-case per-frame cost.
pub(super) const MAX_SIM_STEPS: u32 = 5;

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
pub(super) struct SimSteps {
    pub(super) steps: u32,
    pub(super) new_accumulator: f32,
    pub(super) alpha: f32,
}
