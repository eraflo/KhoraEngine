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

//! **The watchdog counts only a body's own time** (Ergon spec 10): a frame
//! the behavior sits out repaying fuel a body overdrew is that body's run
//! time, and no other's — not another instance of the same behavior, and not
//! a frame handed no fuel at all, which repays nothing.

use khora_core::ecs::entity::EntityId;

use khora_script::arena::Persisted;
use khora_script::vm::Value;

use super::watchdog_stage::{build, entity, Stage, MODULE, SLICE};

/// An `Update` that never finishes, each turn overdrawing its slice: a call
/// of `WatchdogWall` costs more than [`SLICE`].
const WALL: &str = "behavior Wall {
                        int n = 0;
                        void Update(float dt) { while (true) { WatchdogWall(); n += 1; } }
                    }";

/// The count of the body part-way on `subject`, `None` before its first turn.
fn running(stage: &Stage, subject: EntityId) -> Option<f32> {
    let instance = stage.runtime.peek(subject, "Wall")?;
    instance.pending.as_ref().map(|pending| pending.running)
}

/// The sum of the counts of `cast`'s bodies part-way.
fn counted(stage: &Stage, cast: &[EntityId]) -> f32 {
    cast.iter()
        .filter_map(|subject| running(stage, *subject))
        .sum()
}

/// **Instances of one behavior do not share each other's debt**: each game
/// second is one instance's turn, one instance's repayment, or nobody's — so
/// the counts of three overdrawing instances never add up to more game time
/// than has passed.
#[test]
fn instances_of_one_behavior_are_not_charged_each_others_debt() {
    let cast = [entity(0), entity(1), entity(2)];
    let mut stage = Stage::new(WALL, &cast.map(|subject| (subject, "Wall")));
    for _ in 0..30 {
        stage.frame(1.0, SLICE);
        let counted = counted(&stage, &cast);
        assert!(
            counted <= stage.elapsed + 1e-3,
            "after {} s of game time the three bodies are counted {counted} s: {:?}",
            stage.elapsed,
            cast.map(|subject| running(&stage, subject))
        );
    }
}

/// **A frame handed no fuel is not run time, even during a debt**: an
/// overdrawing spinner run a few seconds, then given no fuel for a hundred
/// frames, is neither reported nor faulted on the next frame it runs.
#[test]
fn frames_with_no_fuel_during_a_debt_do_not_count() {
    let subject = entity(0);
    let mut stage = Stage::new(WALL, &[(subject, "Wall")]);
    stage.frames(4, 1.0, SLICE);
    let before = stage
        .running(subject, "Wall")
        .expect("the Update is part-way");
    assert!(before <= 5.0, "four seconds in: {before}");

    stage.frames(100, 1.0, 0);
    stage.frames(3, 1.0, SLICE);
    assert!(
        !stage.disabled(subject, "Wall"),
        "about seven seconds of running faulted it: {:?}",
        stage.instance(subject, "Wall").fault
    );
    assert_eq!(stage.warnings(subject, "Wall"), 0, "{:?}", stage.slow);
}

/// An `Update` that sums the `dt` it is handed, then never finishes, each
/// turn overdrawing its slice.
const LIVING_WALL: &str = "behavior Wall {
                        float lived = 0.0;
                        int n = 0;
                        void Update(float dt) { lived += dt; while (true) { WatchdogWall(); n += 1; } }
                    }";

/// The same, its loop fixed: an edit answering the fault.
const LIVING_WALL_FIXED: &str = "behavior Wall {
                        float lived = 0.0;
                        int n = 0;
                        void Update(float dt) { lived += dt; }
                    }";

/// `Wall`'s `lived` on `subject`.
fn lived(stage: &Stage, subject: EntityId) -> f32 {
    let slot = stage
        .runtime
        .program(MODULE)
        .and_then(|program| program.layout("Wall")?.slot_of("lived"))
        .expect("`Wall` has a field `lived`");
    match stage.instance(subject, "Wall").fields.get(slot) {
        Some(Persisted::Scalar(Value::Float(lived))) => *lived,
        other => panic!("`lived` is a float: {other:?}"),
    }
}

/// **A body the watchdog stops while its behavior sits out a debt leaves its
/// `Update` owed nothing.** A spinner overdraws its slice in half-second
/// frames until the watchdog faults it — on a frame it sits out repaying,
/// not on one of its turns. The edit that fixes it runs its `Update` again,
/// handed at most the frame it runs in — never the minute its stuck body
/// held `Update` off, as a fault on one of its turns already guarantees.
#[test]
fn a_fault_during_a_debt_leaves_the_update_owed_nothing() {
    let subject = entity(0);
    let mut stage = Stage::new(LIVING_WALL, &[(subject, "Wall")]);
    stage
        .frames_until_disabled(subject, "Wall", 0.5, SLICE, 400)
        .expect("the spinner faults in time");
    let before = lived(&stage, subject);
    assert_eq!(
        before, 0.5,
        "set-up: the one `Update` called was handed its frame"
    );

    stage.runtime.reload(MODULE, build(LIVING_WALL_FIXED));
    stage.frame(0.5, 1_000_000);
    let after = lived(&stage, subject);
    assert!(
        after - before <= 0.5,
        "the edit's `Update` was handed {} s: what its faulted body held off came back",
        after - before
    );
}
