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

//! An instance's weight in a frame: the share of frames it takes a turn in,
//! and how much of the frame's turns are `[Critical]`.

use khora_core::script::EventQueue;
use khora_data::flow::{ScriptProgram, ScriptView};
use khora_script::native::Host;

use super::{runtime_of, view_of, MODULE};
use crate::script::{run_behaviors, ScriptRuntime};

/// A 60 Hz frame, in seconds.
const FRAME: f32 = 1.0 / 60.0;

/// A `[Critical]` `Vital` and a `Slow` that runs at 10 Hz.
const CREW: &str = "[Critical]
behavior Vital { int n = 0; void Update(float dt) { n += 1; } }
[Rate(10hz)]
behavior Slow { int n = 0; void Update(float dt) { n += 1; } }";

/// A runtime that has seen one `Vital` and six `Slow`s for a frame.
fn crew() -> ScriptRuntime {
    let mut view: ScriptView = view_of(7);
    view.programs = ["Vital", "Slow"]
        .into_iter()
        .map(|behavior| ScriptProgram {
            module: MODULE.to_owned(),
            behavior: behavior.to_owned(),
        })
        .collect();
    for (index, instance) in view.instances.iter_mut().enumerate() {
        instance.program = u32::from(index > 0);
    }
    let mut runtime = runtime_of(CREW);
    run_behaviors(
        &view,
        &EventQueue::new(),
        &mut runtime,
        &mut Host::new(),
        u64::MAX,
    );
    assert_eq!(runtime.instance_count(), 7, "set-up: every instance seen");
    runtime
}

/// **A rate-limited instance weighs the share of frames it runs.** `Vital`
/// has no rate and weighs one; each `Slow` at 10 Hz runs a sixth of the 60 Hz
/// frames and weighs a sixth.
#[test]
fn weighted_instances_count_a_rate_limited_one_by_its_share_of_frames() {
    let weighted = crew().weighted_instances(FRAME);
    assert!((weighted - 2.0).abs() < 1e-4, "1 + 6 × 1/6, got {weighted}");
}

/// **The critical share is the critical instances' part of the weighted
/// turns.** `Vital` weighs one of the crew's two: half the frame's turns
/// are critical. A runtime with no instance has no critical share.
#[test]
fn critical_share_is_the_critical_part_of_the_weighted_instances() {
    let share = crew().critical_share(FRAME);
    assert!(
        (share - 0.5).abs() < 1e-4,
        "one critical of two, got {share}"
    );
    assert_eq!(ScriptRuntime::new().critical_share(FRAME), 0.0);
}
