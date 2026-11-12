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

//! **The watchdog's count travels with the body** (Ergon spec 10): a save
//! taken part-way through a body that has run forty seconds loads with those
//! forty seconds — it faults about twenty seconds of running later, not
//! sixty — whichever encoding carried it, and a hot reload carrying the body
//! into edited code keeps the count too. A save written before bodies kept
//! a count loads counting from zero.

use khora_core::script::ScriptSnapshot;

use super::saves::every_encoding;
use super::watchdog_stage::{build, entity, recorded, Stage, MODULE, SLICE};

/// An `Update` that never finishes.
const SPINNER: &str = "behavior Spinner {
                           int n = 0;
                           void Update(float dt) { while (true) { n += 1; } }
                       }";

/// The spinner with a field added above its `Update` — an edit leaving the
/// body's own code as it was.
const SPINNER_EDITED: &str = "behavior Spinner {
                                  int n = 0;
                                  int laps = 0;
                                  void Update(float dt) { while (true) { n += 1; } }
                              }";

/// A spinner forty frames of one second into its `Update`, and the snapshot
/// the lane wrote of it that last frame.
fn forty_seconds_in() -> (Stage, ScriptSnapshot) {
    let subject = entity(0);
    let mut stage = Stage::new(SPINNER, &[(subject, "Spinner")]);
    let report = stage.frames(40, 1.0, SLICE);
    assert!(
        !stage.disabled(subject, "Spinner"),
        "forty seconds is no fault"
    );
    let saved = recorded(&report, subject, "Spinner");
    assert!(saved.pending.is_some(), "the save holds the body part-way");
    (stage, saved)
}

/// Asserts a spinner forty seconds in, then `stage`, faults after between
/// fifteen and twenty-five more frames of one second — not sixty.
fn faults_about_twenty_seconds_on(mut stage: Stage, how: &str) {
    let subject = entity(0);
    let more = stage.frames_until_disabled(subject, "Spinner", 1.0, SLICE, 70);
    let more = more.unwrap_or_else(|| panic!("{how}: the spinner never faulted"));
    assert!(
        (15..=25).contains(&more),
        "{how}: forty seconds were kept, so it faults about twenty later, not {more}"
    );
}

/// **A save at forty seconds keeps counting after loading**: whichever
/// encoding carried it, the loaded body faults about twenty seconds of
/// running later.
#[test]
fn a_save_taken_at_forty_seconds_faults_about_twenty_seconds_after_loading() {
    let (_, saved) = forty_seconds_in();
    for (encoding, carry) in every_encoding() {
        let loaded = carry(&saved);
        assert_eq!(loaded, saved, "{encoding}: the save is carried whole");

        let mut stage = Stage::loaded(SPINNER, "Spinner", loaded);
        stage.frame(1.0, SLICE);
        let subject = entity(0);
        assert!(
            stage.instance(subject, "Spinner").pending.is_some(),
            "{encoding}: the body resumed part-way"
        );
        faults_about_twenty_seconds_on(stage, encoding);
    }
}

/// **A save written before bodies kept a count loads counting from zero**:
/// its pending body has no count, so it runs a full minute before it faults.
#[test]
fn a_save_without_a_count_loads_counting_from_zero() {
    let (_, saved) = forty_seconds_in();
    let mut json = serde_json::to_value(&saved).expect("a snapshot serialises");
    if let Some(pending) = json.get_mut("pending").and_then(|p| p.as_object_mut()) {
        pending.remove("running");
    }
    let older: ScriptSnapshot = serde_json::from_value(json).expect("and parses without a count");

    let subject = entity(0);
    let mut stage = Stage::loaded(SPINNER, "Spinner", older);
    let more = stage.frames_until_disabled(subject, "Spinner", 1.0, SLICE, 70);
    let more = more.expect("the loaded spinner faults in time");
    assert!(
        (55..=65).contains(&more),
        "no count saved, so a full minute of running: {more}"
    );
}

/// **A hot reload carrying the body keeps its count**: an edit leaving the
/// spinning `Update` as it was carries the body on, forty seconds already
/// run, and it faults about twenty seconds later.
#[test]
fn a_hot_reload_carrying_the_body_keeps_its_count() {
    let (mut stage, _) = forty_seconds_in();
    let subject = entity(0);
    stage.runtime.reload(MODULE, build(SPINNER_EDITED));
    assert!(
        stage.instance(subject, "Spinner").pending.is_some(),
        "the edit carried the body on"
    );
    faults_about_twenty_seconds_on(stage, "hot reload");
}

/// **A reload into the very same code keeps the count** — the body is not
/// carried at all, and nothing about it changes.
#[test]
fn a_reload_into_the_same_code_keeps_the_count() {
    let (mut stage, _) = forty_seconds_in();
    let subject = entity(0);
    let before = stage.running(subject, "Spinner");
    stage.runtime.reload(MODULE, build(SPINNER));
    assert_eq!(stage.running(subject, "Spinner"), before);
    assert!(before.is_some_and(|running| running >= 39.0));
    faults_about_twenty_seconds_on(stage, "same-code reload");
}
