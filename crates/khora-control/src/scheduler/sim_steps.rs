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
/// The engine's one cap, which the script lane bounds the steps it owes by.
pub(super) const MAX_SIM_STEPS: u32 = khora_core::time::MAX_SIM_STEPS;

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
/// a runaway). A non-finite total — an infinite time scale — saturates the
/// same way and carries nothing, so one such frame cannot leave a `NaN` that
/// would stop the clock for good. A non-positive `fixed_delta` is treated as a
/// single step with no leftover (degenerate guard; callers pass a positive
/// step).
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

/// The fraction of a step by which the sub-clock may fall short and still
/// count the step as covered. `n` ticks summed in `f32` land a few ulps under
/// `n` steps of the same length; without this slack a step equal to the tick
/// would lose a sub-step to rounding and physics would drift behind the game.
/// The shortfall is carried as a debt, never forgiven, so the slack moves a
/// step earlier and never adds time.
const SUB_STEP_SLACK: f32 = 1e-3;

/// Physics sub-step arithmetic: how many steps of the declared physics
/// `step` cover the simulated time the gameplay ticks just `advanced`.
///
/// The carried-over `accumulator` plus `advanced` is consumed in whole
/// `step`s; the remainder is returned to be carried into the next frame
/// (also across a change of `step`). It lies in `[-SLACK·step, step)`: a step
/// taken a hair early is owed back from the next frame's time rather than
/// rounded away, so over any run the steps never cover more time than the
/// ticks advanced, nor fall a step behind it. There is no cap here: the ticks
/// are already capped, so the sub-steps a frame owes are bounded by the time
/// those ticks advanced. A non-positive `step` runs nothing and keeps nothing
/// (degenerate guard; the scheduler only passes positive steps).
pub(super) fn compute_sub_steps(accumulator: f32, advanced: f32, step: f32) -> SubSteps {
    if step <= 0.0 {
        return SubSteps {
            steps: 0,
            new_accumulator: 0.0,
        };
    }

    let acc = accumulator + advanced;
    let steps = (acc / step + SUB_STEP_SLACK).floor().max(0.0) as u32;
    // A step covered by the slack leaves a residue a hair below zero: the
    // time it ran ahead, paid back next frame.
    let new_accumulator = acc - steps as f32 * step;

    SubSteps {
        steps,
        new_accumulator,
    }
}

/// Result of [`compute_sub_steps`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SubSteps {
    pub(super) steps: u32,
    pub(super) new_accumulator: f32,
}
