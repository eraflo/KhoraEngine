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

//! **An edit or a load in the middle of a debt keeps what the debt has
//! counted** (Ergon spec 10): the frames a behavior sits out repaying fuel
//! its body overdrew are that body's run time; a hot reload carrying the body
//! into edited code keeps its count — those frames included — as a reload
//! into the same code does, and a game loaded while the debt still runs does
//! not put the fault off either.

use khora_core::script::ScriptSnapshot;
use khora_lanes::script_lane::ScriptRuntime;

use super::watchdog_stage::{build, entity, Stage, MODULE, SLICE};

/// An `Update` that never finishes, each call of `WatchdogBoulder` (two
/// thousand fuel, defined beside the debt tests) overdrawing its turn by
/// about ninety-nine frames of [`SLICE`].
const HAULER: &str = "behavior Hauler {
                          int n = 0;
                          void Update(float dt) { while (true) { WatchdogBoulder(); n += 1; } }
                      }";

/// The hauler counting its laps by two — an edit to its `Update`'s code that
/// carries the body on, part-way.
const HAULER_EDITED: &str = "behavior Hauler {
                                 int n = 0;
                                 void Update(float dt) { while (true) { WatchdogBoulder(); n += 2; } }
                             }";

/// Frames of one second until the hauler is disabled, `edit` applied after
/// the fiftieth — in the middle of its first debt.
fn frames_to_fault(edit: Option<&str>) -> usize {
    let subject = entity(0);
    let mut stage = Stage::new(HAULER, &[(subject, "Hauler")]);
    stage.frames(50, 1.0, SLICE);
    assert!(!stage.disabled(subject, "Hauler"), "fifty seconds in");
    if let Some(edit) = edit {
        let reports = stage.runtime.reload(MODULE, build(edit));
        assert!(
            stage.instance(subject, "Hauler").pending.is_some(),
            "the edit carried the body on: {reports:?}"
        );
    }
    50 + stage
        .frames_until_disabled(subject, "Hauler", 1.0, SLICE, 400)
        .expect("the hauler faults in time")
}

/// **An edit mid-debt does not put the fault off**: the hauler faults on the
/// same frame whether or not its module was edited fifty seconds in — the
/// fifty seconds it sat out repaying are kept like the rest of its count.
#[test]
fn an_edit_in_the_middle_of_a_debt_keeps_the_seconds_it_counted() {
    let untouched = frames_to_fault(None);
    let same_code = frames_to_fault(Some(HAULER));
    let edited = frames_to_fault(Some(HAULER_EDITED));
    assert_eq!(
        same_code, untouched,
        "a reload into the same code changes nothing: {untouched} {same_code} {edited}"
    );
    assert!(
        edited.abs_diff(untouched) <= 2,
        "faulted after {untouched} s untouched, {edited} s with an edit fifty seconds in"
    );
}

/// **A game loaded in the middle of a debt does not put the fault off**: the
/// lane keeps the behavior's debt across the load, so the body the save
/// restored sits out the rest of it — and those frames count as they would
/// have for the body before the load.
#[test]
fn a_load_in_the_middle_of_a_debt_does_not_put_the_fault_off() {
    let untouched = frames_to_fault(None);

    let subject = entity(0);
    let mut stage = Stage::new(HAULER, &[(subject, "Hauler")]);
    let mut saved: Option<ScriptSnapshot> = None;
    for _ in 0..50 {
        let report = stage.frame(1.0, SLICE);
        if let Some(update) = report.state.iter().find(|update| update.entity == subject) {
            saved = Some(update.snapshot.clone());
        }
    }
    let saved = saved.expect("the lane wrote the hauler");
    assert!(saved.pending.is_some(), "the save holds the body part-way");

    // Loaded into the running game: the same lane, its debt still running.
    let mut loaded = Stage::loaded(HAULER, "Hauler", saved);
    loaded.runtime = std::mem::replace(&mut stage.runtime, ScriptRuntime::new());
    let after = loaded
        .frames_until_disabled(subject, "Hauler", 1.0, SLICE, 400)
        .expect("the loaded hauler faults in time");
    assert!(
        (50 + after).abs_diff(untouched) <= 2,
        "faulted after {untouched} s untouched, {} s with a load fifty seconds in",
        50 + after
    );
}
