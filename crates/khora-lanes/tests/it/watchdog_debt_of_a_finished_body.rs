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

//! **A debt is the run time of the body that overdrew it, and no other's**
//! (Ergon spec 10): when the body that overspent finishes in the very turn it
//! overspent, the frames the behavior then sits out repaying belong to
//! nobody — not to the body that starts once the debt is paid, which
//! overdrew none of it.

use khora_lanes::script_lane::{Body, WATCHDOG_WARN};
use khora_script::ergon_fn;

use super::watchdog_stage::{entity, Stage, SLICE};

/// A native costing a hundred slices: one call overdraws a turn by about
/// ninety-nine frames of fuel.
#[ergon_fn(name = "WatchdogBoulder", cost = 2000)]
fn watchdog_boulder() -> f32 {
    0.0
}

/// `OnSpawn` makes one costly call and is done; a timer due from the first
/// second runs a short loop — a few frames of [`SLICE`], nowhere near the
/// watchdog.
const SETTLER: &str = "behavior Settler {
                           int ticks = 0;
                           void OnSpawn() { WatchdogBoulder(); }
                           every 1s {
                               int i = 0;
                               while (i < 6) { i += 1; }
                               ticks += 1;
                           }
                       }";

/// The largest count a pending timer body of the settler on entity 0 shows,
/// over frames of one second until the timer has run once — and how many
/// frames that took. At most `limit` frames.
fn first_timer_run(stage: &mut Stage, limit: usize) -> (Option<f32>, usize) {
    let subject = entity(0);
    let mut largest: Option<f32> = None;
    for frame in 1..=limit {
        stage.frame(1.0, SLICE);
        let instance = stage.instance(subject, "Settler");
        if let Some(pending) = instance
            .pending
            .as_ref()
            .filter(|pending| matches!(pending.body, Body::Timer { .. }))
        {
            largest = Some(largest.map_or(pending.running, |was| was.max(pending.running)));
        }
        if stage.int(subject, "Settler", "ticks").unwrap_or(0) > 0 {
            return (largest, frame);
        }
    }
    panic!("the timer never ran in {limit} frames");
}

/// **The body that starts after an overdrawing body finished is not charged
/// that debt**: the timer waits, due, while its behavior repays `OnSpawn`'s
/// call — about a hundred seconds — and once it runs, its count is the few
/// frames it ran, not the debt.
#[test]
fn a_body_started_after_the_overdrawing_one_finished_is_not_charged_its_debt() {
    let subject = entity(0);
    let mut stage = Stage::new(SETTLER, &[(subject, "Settler")]);
    let (largest, frames) = first_timer_run(&mut stage, 300);
    assert!(
        frames > 50,
        "the timer waited out OnSpawn's debt before it ran: it ran by frame {frames}"
    );
    let largest = largest.expect("the timer's body ran over more than one frame");
    assert!(
        largest <= 5.0,
        "the timer ran a few frames, yet was counted {largest} s"
    );
    assert!(!stage.disabled(subject, "Settler"));
    assert_eq!(stage.warnings(subject, "Settler"), 0, "{:?}", stage.slow);
}

/// **A short body is never reported nor faulted for another body's debt**:
/// the timer's loop takes a few frames each time; over three minutes it is
/// never reported nor stopped.
#[test]
fn a_short_timer_behind_an_overdrawing_on_spawn_is_never_faulted() {
    let subject = entity(0);
    let mut stage = Stage::new(SETTLER, &[(subject, "Settler")]);
    for _ in 0..180 {
        stage.frame(1.0, SLICE);
        assert!(
            !stage.disabled(subject, "Settler"),
            "faulted after {} s: {:?}",
            stage.elapsed,
            stage.instance(subject, "Settler").fault
        );
        let running = stage
            .instance(subject, "Settler")
            .pending
            .as_ref()
            .map_or(0.0, |pending| pending.running);
        assert!(
            running < WATCHDOG_WARN,
            "after {} s the timer is counted {running} s",
            stage.elapsed
        );
    }
    assert_eq!(stage.warnings(subject, "Settler"), 0, "{:?}", stage.slow);
    assert!(
        stage.int(subject, "Settler", "ticks").unwrap_or(0) > 1,
        "the timer ran, more than once"
    );
}
