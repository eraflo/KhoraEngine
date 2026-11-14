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

//! Physics sub-step arithmetic: how many physics steps cover the time the
//! gameplay ticks advanced.

/// The fraction of a step by which the sub-clock may fall short and still count it covered.
///
/// `n` ticks summed in `f32` land a few ulps under `n` steps of the same length: without
/// the slack, physics would lose a sub-step to rounding and drift behind the game. The
/// shortfall is carried as a debt, so the slack moves a step earlier and never adds time.
const SUB_STEP_SLACK: f32 = 1e-3;

/// Result of [`compute_sub_steps`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SubSteps {
    pub(super) steps: u32,
    pub(super) new_accumulator: f32,
}

/// Returns how many steps of `step` cover the time the ticks just `advanced`, and the carry.
///
/// The carry lies in `[-SLACK·step, step)`, kept across a change of `step`: a step taken a
/// hair early is owed back next frame, so the steps never cover more time than the ticks
/// advanced, nor fall a step behind. No cap: the ticks are already capped. A non-positive
/// `step` runs nothing and carries nothing.
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
