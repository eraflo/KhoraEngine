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

//! **A body's first frame is watched like the others** (Ergon spec 10): the
//! frame a body starts in and is cut by fuel counts its `dt`, so one long
//! frame — a hitch, a fast time scale — that takes it past ten seconds is
//! reported, and one that takes it past sixty faults it.

use khora_core::ecs::entity::EntityId;

use super::watchdog_stage::{entity, event, Stage, SLICE};

/// An `Update` that never finishes.
const SPINNER: &str = "behavior Spinner {
                           int n = 0;
                           void Update(float dt) { while (true) { n += 1; } }
                       }";

fn spinner() -> (Stage, EntityId) {
    let subject = entity(0);
    (Stage::new(SPINNER, &[(subject, "Spinner")]), subject)
}

/// **A first frame of twelve seconds is reported**: the count passes ten in
/// the frame the body starts, and frames after it do not pass it again.
#[test]
fn a_first_frame_past_ten_seconds_is_reported() {
    let (mut stage, subject) = spinner();
    stage.frame(12.0, SLICE);
    stage.frames(5, 1.0, SLICE);
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        1,
        "seventeen seconds of running, counted {:?}",
        stage.running(subject, "Spinner")
    );
}

/// **An event handler started in a long frame is reported** the same.
#[test]
fn a_handler_started_in_a_frame_past_ten_seconds_is_reported() {
    let source = "behavior Listener {
                      int n = 0;
                      on Go() { while (true) { n += 1; } }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Listener")]);
    stage.frame_with(15.0, SLICE, &event(subject, "Go"));
    stage.frames(5, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, "Listener"), 1, "{:?}", stage.slow);
}

/// **A body that has run past sixty seconds in the frame it starts faults in
/// that frame**: a frame of a hundred seconds leaves nothing part-way.
#[test]
fn a_first_frame_past_sixty_seconds_faults_in_that_frame() {
    let (mut stage, subject) = spinner();
    let report = stage.frame(100.0, SLICE);
    assert!(
        stage.disabled(subject, "Spinner"),
        "counted {:?} s and still running",
        stage.running(subject, "Spinner")
    );
    assert_eq!(report.faulted, 1);
}

/// **Frames of no time, negative time or no number add nothing**, and a
/// spinner given them between seconds of running faults as if they were not
/// there.
#[test]
fn frames_of_no_negative_or_nan_time_add_nothing() {
    let (mut stage, subject) = spinner();
    stage.frames(5, 1.0, SLICE);
    let before = stage.running(subject, "Spinner").expect("part-way");
    for delta in [0.0, -30.0, f32::NAN, -0.0] {
        stage.frame(delta, SLICE);
        assert_eq!(
            stage.running(subject, "Spinner"),
            Some(before),
            "a frame of {delta} s"
        );
    }
    assert!(!stage.disabled(subject, "Spinner"));
}
