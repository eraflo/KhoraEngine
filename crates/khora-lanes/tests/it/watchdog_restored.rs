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

//! **What the watchdog names, and what it leaves behind** (Ergon spec 10):
//! the member of a state's body and the function a body stands in when it
//! has called into another; a fault the watchdog recorded travels in a save
//! and keeps the loaded instance stopped; and a restored body waiting behind
//! a long `OnLoad` keeps its count, not running while it waits.

use super::watchdog_stage::{entity, recorded, Stage, SLICE};

/// **A state's spinning `Update` is reported by its member path**,
/// `Patrol.Update`.
#[test]
fn a_spinning_state_update_is_reported_by_its_member_path() {
    let source = "behavior Guard {
                      int n = 0;
                      state Patrol {
                          void Update(float dt) { while (true) { n += 1; } }
                      }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Guard")]);
    stage.frames(14, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, "Guard"), 1, "{:?}", stage.slow);
    assert_eq!(stage.slow[0].member, "Patrol.Update");
    let faulted = stage.frames_until_disabled(subject, "Guard", 1.0, SLICE, 60);
    assert!(faulted.is_some());
    let fault = stage
        .instance(subject, "Guard")
        .fault
        .as_ref()
        .expect("fault");
    assert!(fault.reason.contains("`Patrol.Update`"), "{}", fault.reason);
}

/// **A body spinning in a function it called is reported by its member, at
/// the site in that function.**
#[test]
fn a_body_spinning_in_a_called_function_names_where_it_stands() {
    let source = "behavior Caller {
                      int n = 0;
                      void Spin() { while (true) { n += 1; } }
                      void Update(float dt) { Spin(); }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Caller")]);
    stage.frames(14, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, "Caller"), 1, "{:?}", stage.slow);
    let slow = &stage.slow[0];
    assert_eq!(slow.member, "Update", "{slow:?}");
    assert!(slow.site.contains("Spin"), "stands in `Spin`: {slow:?}");
}

/// **A save of an instance the watchdog stopped loads stopped**: its fault
/// is in the save, and the loaded instance takes no turn.
#[test]
fn a_save_of_an_instance_the_watchdog_stopped_loads_stopped() {
    let source = "behavior Spinner {
                      int n = 0;
                      void Update(float dt) { while (true) { n += 1; } }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Spinner")]);
    let mut saved = None;
    for _ in 0..70 {
        let report = stage.frame(1.0, SLICE);
        if stage.disabled(subject, "Spinner") {
            saved = Some(recorded(&report, subject, "Spinner"));
            break;
        }
    }
    let saved = saved.expect("the spinner faulted");
    let reason = saved
        .lifecycle
        .fault
        .clone()
        .expect("the save holds the fault");
    assert!(saved.pending.is_none(), "and no body part-way");

    let mut loaded = Stage::loaded(source, "Spinner", saved);
    loaded.frame(1.0, SLICE);
    let n = loaded.int(subject, "Spinner", "n");
    loaded.frames(20, 1.0, SLICE);
    assert!(loaded.disabled(subject, "Spinner"), "loaded stopped");
    assert_eq!(loaded.int(subject, "Spinner", "n"), n, "no turn taken");
    assert_eq!(
        loaded
            .instance(subject, "Spinner")
            .fault
            .as_ref()
            .map(|f| &f.reason),
        Some(&reason.reason)
    );
}

/// **A restored body behind a long `OnLoad` keeps its count**: saved forty
/// seconds in, it waits out an `OnLoad` of several frames without counting
/// them, then faults about twenty seconds of its own running later.
#[test]
fn a_restored_body_behind_a_long_on_load_keeps_its_count() {
    let source = "behavior Spinner {
                      int n = 0;
                      int loaded = 0;
                      void OnLoad() {
                          int i = 0;
                          while (i < 4) { WatchdogWall(); i += 1; }
                          loaded = 1;
                      }
                      void Update(float dt) { while (true) { n += 1; } }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Spinner")]);
    let report = stage.frames(40, 1.0, SLICE);
    let saved = recorded(&report, subject, "Spinner");
    assert!(saved.pending.is_some());

    let mut loaded = Stage::loaded(source, "Spinner", saved);
    let mut on_load = 0;
    while loaded.int(subject, "Spinner", "loaded") != Some(1) {
        loaded.frame(1.0, SLICE);
        on_load += 1;
        assert!(on_load < 40, "`OnLoad` finishes");
    }
    assert!(
        on_load >= 3,
        "the premise: `OnLoad` takes frames ({on_load})"
    );
    assert!(!loaded.disabled(subject, "Spinner"));
    let more = loaded
        .frames_until_disabled(subject, "Spinner", 1.0, SLICE, 70)
        .expect("the restored body faults");
    assert!(
        (15..=25).contains(&more),
        "forty seconds kept, `OnLoad`'s {on_load} frames not counted: faulted {more} frames later"
    );
}
