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

//! **The watchdog** (Ergon spec 10): a body that keeps running without
//! finishing — a loop with no `await`, cut by fuel frame after frame — is
//! reported once past ten seconds of game time, and faults past sixty:
//! disabled, its fault recorded, its body dropped, until an edit. Waiting is
//! not running, and frames the instance is not run do not count.
//!
//! Game time is the `dt` the frames are given: frames of one second reach a
//! minute in sixty frames. Where the plan leaves open whether the frame a
//! body starts in counts, the bounds below allow either.

use khora_core::ecs::entity::EntityId;
use khora_lanes::script_lane::{WATCHDOG_FAULT, WATCHDOG_WARN};

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

/// **The limits are ten and sixty game seconds**, the first below the second.
#[test]
#[expect(
    clippy::assertions_on_constants,
    reason = "the limits are constants, and this pins them"
)]
fn the_watchdog_limits_are_ten_and_sixty_game_seconds() {
    assert_eq!(WATCHDOG_WARN, 10.0);
    assert_eq!(WATCHDOG_FAULT, 60.0);
    assert!(WATCHDOG_WARN < WATCHDOG_FAULT);
}

/// **A body cut by fuel counts the game time it runs**: five frames of one
/// second, then four of half a second.
#[test]
fn a_body_cut_by_fuel_counts_the_game_time_it_has_run() {
    let (mut stage, subject) = spinner();
    stage.frames(5, 1.0, SLICE);
    let after_five = stage
        .running(subject, "Spinner")
        .expect("the Update is part-way");
    assert!(
        (4.0..=5.0).contains(&after_five),
        "five one-second frames of running: {after_five}"
    );

    stage.frames(4, 0.5, SLICE);
    let after_nine = stage.running(subject, "Spinner").expect("still part-way");
    assert!(
        (after_five + 2.0 - 1e-4..=after_five + 2.0 + 1e-4).contains(&after_nine),
        "four half-second frames more: {after_five} then {after_nine}"
    );
}

/// **An `await` resets the count, and its wait adds nothing**: a sequence
/// that ran a few seconds then waits half a minute has run nothing while it
/// waits.
#[test]
fn an_await_resets_the_count_and_its_wait_adds_nothing() {
    let source = "behavior Patient {
                      async void Work() {
                          int i = 0;
                          while (i < 6) { WatchdogWall(); i += 1; }
                          await 30.0s;
                          while (true) { WatchdogWall(); }
                      }
                      on Go() { Work(); }
                  }";
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Patient")]);
    stage.frame_with(1.0, SLICE, &event(subject, "Go"));
    stage.frames(2, 1.0, SLICE);
    let before = stage
        .running(subject, "Patient")
        .expect("the sequence is part-way");
    assert!(before >= 2.0, "three frames of running: {before}");

    // Until it stands at its wait.
    let mut waits = false;
    for _ in 0..20 {
        stage.frame(1.0, SLICE);
        if stage
            .instance(subject, "Patient")
            .pending
            .as_ref()
            .is_some_and(|pending| pending.remaining > 0.0)
        {
            waits = true;
            break;
        }
    }
    assert!(waits, "the sequence reached its `await`");
    for _ in 0..10 {
        assert_eq!(
            stage.running(subject, "Patient"),
            Some(0.0),
            "a body waiting has not run since its `await`"
        );
        stage.frame(1.0, SLICE);
    }
}

/// **A spinning `Update` is reported once, past ten seconds**: nothing for
/// its first nine, one report by its twelfth naming the entity, the
/// behavior, the member and the site it stands at — and no other through its
/// fiftieth, with the instance still running.
#[test]
fn a_spinning_update_is_reported_once_past_ten_seconds() {
    let (mut stage, subject) = spinner();
    stage.frames(9, 1.0, SLICE);
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        0,
        "nine seconds is fine"
    );

    stage.frames(3, 1.0, SLICE);
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        1,
        "reported past ten seconds: {:?}",
        stage.slow
    );
    let slow = &stage.slow[0];
    assert_eq!(slow.member, "Update");
    assert!(!slow.site.is_empty(), "the site is named: {slow:?}");

    stage.frames(38, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, "Spinner"), 1, "reported once");
    assert!(
        !stage.disabled(subject, "Spinner"),
        "fifty seconds is no fault"
    );
}

/// **A spinning `Update` faults past sixty seconds**: running through its
/// fifty-eighth, disabled by its sixty-second — its fault recorded, written
/// back with its state the frame it faults, its body gone — and it takes no
/// more turns.
#[test]
fn a_spinning_update_faults_past_sixty_seconds() {
    let (mut stage, subject) = spinner();
    stage.frames(58, 1.0, SLICE);
    assert!(
        !stage.disabled(subject, "Spinner"),
        "58 seconds is no fault"
    );

    let mut faulting = None;
    for _ in 0..4 {
        let report = stage.frame(1.0, SLICE);
        if stage.disabled(subject, "Spinner") {
            faulting = Some(report);
            break;
        }
    }
    let report = faulting.expect("faulted by its sixty-second second");
    assert_eq!(report.faulted, 1, "the frame counts the fault");
    let instance = stage.instance(subject, "Spinner");
    let fault = instance.fault.as_ref().expect("the fault is recorded");
    assert!(
        fault.reason.contains("Update"),
        "the reason names the member: {}",
        fault.reason
    );
    assert!(instance.pending.is_none(), "the body is dropped");
    assert!(
        report
            .state
            .iter()
            .any(|update| update.entity == subject && update.snapshot.lifecycle.fault.is_some()),
        "the companion records the fault the frame it happens"
    );

    let n = stage.int(subject, "Spinner", "n");
    let after = stage.frames(20, 1.0, SLICE);
    assert_eq!(stage.int(subject, "Spinner", "n"), n, "no more turns");
    assert_eq!(after.spent, 0, "a disabled instance spends nothing");
}

/// **An edit re-enables a body the watchdog faulted**, and its count starts
/// again: neither the frames it stood disabled nor the body it lost count —
/// nothing reported for nine seconds after the edit, a report by twelve.
#[test]
fn an_edit_re_enables_a_body_the_watchdog_faulted_and_counts_from_zero() {
    let (mut stage, subject) = spinner();
    let faulted = stage.frames_until_disabled(subject, "Spinner", 1.0, SLICE, 70);
    assert!(faulted.is_some(), "the spinner faulted");
    stage.frames(100, 1.0, SLICE);
    let warned = stage.warnings(subject, "Spinner");

    let edited = "behavior Spinner {
                      int n = 0;
                      int edits = 1;
                      void Update(float dt) { while (true) { n += 1; } }
                  }";
    stage.runtime.reload(
        super::watchdog_stage::MODULE,
        super::watchdog_stage::build(edited),
    );
    let n = stage.int(subject, "Spinner", "n").expect("n");
    stage.frames(9, 1.0, SLICE);
    let instance = stage.instance(subject, "Spinner");
    assert!(!instance.disabled, "the edit re-enabled it");
    assert!(instance.fault.is_none(), "and cleared its fault");
    assert!(
        stage.int(subject, "Spinner", "n").expect("n") > n,
        "it runs again"
    );
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        warned,
        "nine seconds after the edit, nothing reported"
    );

    stage.frames(3, 1.0, SLICE);
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        warned + 1,
        "reported anew past ten seconds"
    );
}

/// Asserts the body `behavior` runs from `source` — started by `first`'s
/// events on the first frame — is reported once past ten seconds with a
/// member `member` accepts, and faults past sixty.
fn watched(source: &str, behavior: &str, first: &str, member: impl Fn(&str) -> bool) {
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, behavior)]);
    if first.is_empty() {
        stage.frame(1.0, SLICE);
    } else {
        stage.frame_with(1.0, SLICE, &event(subject, first));
    }
    stage.frames(8, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, behavior), 0, "nine seconds is fine");
    for _ in 0..5 {
        if stage.warnings(subject, behavior) > 0 {
            break;
        }
        stage.frame(1.0, SLICE);
    }
    assert_eq!(
        stage.warnings(subject, behavior),
        1,
        "reported past ten seconds, by the fourteenth"
    );
    assert!(
        member(&stage.slow[0].member),
        "the member: {:?}",
        stage.slow[0]
    );
    assert!(!stage.slow[0].site.is_empty(), "the site is named");

    while stage.frames < 58 {
        stage.frame(1.0, SLICE);
    }
    assert!(!stage.disabled(subject, behavior), "58 seconds is no fault");
    assert_eq!(stage.warnings(subject, behavior), 1, "reported once");
    let more = stage.frames_until_disabled(subject, behavior, 1.0, SLICE, 6);
    assert!(more.is_some(), "faulted by its sixty-fourth second");
    assert!(stage.instance(subject, behavior).pending.is_none());
}

/// **A spinning timer body is watched like `Update`.**
#[test]
fn a_spinning_timer_body_is_reported_then_faulted() {
    watched(
        "behavior Ticker {
             int n = 0;
             every 1s { while (true) { n += 1; } }
         }",
        "Ticker",
        "",
        |member| member.contains("every"),
    );
}

/// **A spinning event handler is watched like `Update`.**
#[test]
fn a_spinning_event_handler_is_reported_then_faulted() {
    watched(
        "behavior Listener {
             int n = 0;
             on Go() { while (true) { n += 1; } }
         }",
        "Listener",
        "Go",
        |member| member == "Go",
    );
}

/// **A spinning `async` sequence is watched like `Update`.**
#[test]
fn a_spinning_sequence_is_reported_then_faulted() {
    watched(
        "behavior Sequencer {
             int n = 0;
             async void Work() { await 0.5s; while (true) { n += 1; } }
             on Go() { Work(); }
         }",
        "Sequencer",
        "Go",
        |member| member == "Go",
    );
}

/// **A spinning `OnSpawn` is watched like `Update`.**
#[test]
fn a_spinning_on_spawn_is_reported_then_faulted() {
    watched(
        "behavior Herald {
             int n = 0;
             void OnSpawn() { while (true) { n += 1; } }
         }",
        "Herald",
        "",
        |member| member == "OnSpawn",
    );
}

/// **Waiting is not running**: a sequence awaiting two minutes finishes,
/// never reported nor faulted, while a spinner beside it over the same
/// frames is both.
#[test]
fn a_long_await_is_never_reported_while_a_spinner_beside_it_faults() {
    let source = "behavior Waiter {
                      int done = 0;
                      async void Wait() { await 120.0s; done = 1; }
                      on Go() { Wait(); }
                  }
                  behavior Spinner {
                      int n = 0;
                      void Update(float dt) { while (true) { n += 1; } }
                  }";
    let (waiter, spinner) = (entity(0), entity(1));
    let mut stage = Stage::new(source, &[(waiter, "Waiter"), (spinner, "Spinner")]);
    stage.frame_with(1.0, 2_000, &event(waiter, "Go"));
    stage.frames(124, 1.0, 2_000);

    assert_eq!(stage.int(waiter, "Waiter", "done"), Some(1), "it finished");
    assert_eq!(
        stage.warnings(waiter, "Waiter"),
        0,
        "waiting is not running"
    );
    assert!(!stage.disabled(waiter, "Waiter"));
    assert_eq!(
        stage.warnings(spinner, "Spinner"),
        1,
        "the spinner reported"
    );
    assert!(stage.disabled(spinner, "Spinner"), "the spinner faulted");
}

/// **A loop that awaits every second runs for minutes**, never reported nor
/// faulted, while a spinner beside it faults.
#[test]
fn a_loop_awaiting_every_second_runs_for_minutes_unwatched() {
    let source = "behavior Pulse {
                      int beats = 0;
                      async void Beat() { while (true) { await 1.0s; beats += 1; } }
                      on Go() { Beat(); }
                  }
                  behavior Spinner {
                      int n = 0;
                      void Update(float dt) { while (true) { n += 1; } }
                  }";
    let (pulse, spinner) = (entity(0), entity(1));
    let mut stage = Stage::new(source, &[(pulse, "Pulse"), (spinner, "Spinner")]);
    stage.frame_with(1.0, 2_000, &event(pulse, "Go"));
    stage.frames(299, 1.0, 2_000);

    let beats = stage.int(pulse, "Pulse", "beats").expect("beats");
    assert!(beats >= 290, "five minutes of beats: {beats}");
    assert_eq!(stage.warnings(pulse, "Pulse"), 0);
    assert!(!stage.disabled(pulse, "Pulse"));
    assert!(stage.disabled(spinner, "Spinner"), "the spinner faulted");
}

/// **Frames starved of fuel do not count**: a spinner run eight seconds then
/// starved a hundred is not reported, and is by five seconds more of running;
/// run fifty-five then starved a hundred it has not faulted, and has by ten
/// more.
#[test]
fn frames_starved_of_fuel_do_not_count() {
    let (mut stage, subject) = spinner();
    stage.frames(8, 1.0, SLICE);
    let before = stage.running(subject, "Spinner");
    stage.frames(100, 1.0, 0);
    assert_eq!(
        stage.warnings(subject, "Spinner"),
        0,
        "starved, not running"
    );
    assert_eq!(
        stage.running(subject, "Spinner"),
        before,
        "starved frames add nothing"
    );
    assert!(before.is_some_and(|running| running > 0.0));

    stage.frames(5, 1.0, SLICE);
    assert_eq!(stage.warnings(subject, "Spinner"), 1, "thirteen of running");

    stage.frames(42, 1.0, SLICE);
    stage.frames(100, 1.0, 0);
    assert!(!stage.disabled(subject, "Spinner"), "55 seconds of running");
    let more = stage.frames_until_disabled(subject, "Spinner", 1.0, SLICE, 10);
    assert!(more.is_some(), "faulted by sixty-five seconds of running");
}

/// How many frames of `fuel` the body of `behavior` in `source` takes to
/// raise its `int` field `marker` to `value`, counted at no game time.
fn frames_to(source: &str, behavior: &str, start: &str, marker: &str, value: i64) -> usize {
    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, behavior)]);
    if start.is_empty() {
        stage.frame(0.0, SLICE);
    } else {
        stage.frame_with(0.0, SLICE, &event(subject, start));
    }
    for frames in 2..=2_000 {
        stage.frame(0.0, SLICE);
        if stage.int(subject, behavior, marker) == Some(value) {
            return frames;
        }
    }
    panic!("`{behavior}.{marker}` never reached {value}");
}

/// **An `await` resets the count**: a sequence running forty-five seconds,
/// awaiting a second, then running forty-five more finishes unfaulted —
/// while the same sequence with no `await` between faults.
#[test]
fn an_await_between_two_long_stretches_resets_the_count() {
    let stretch = |pause: &str| {
        format!(
            "behavior Worker {{
                 int stage = 0;
                 async void Work() {{
                     int i = 0;
                     while (i < 40) {{ WatchdogWall(); i += 1; }}
                     stage = 1;
                     {pause}
                     i = 0;
                     while (i < 40) {{ WatchdogWall(); i += 1; }}
                     stage = 2;
                 }}
                 on Go() {{ Work(); }}
             }}"
        )
    };
    let patient = stretch("await 1.0s;");
    let unbroken = stretch("");
    let half = frames_to(&unbroken, "Worker", "Go", "stage", 1);
    let whole = frames_to(&unbroken, "Worker", "Go", "stage", 2);
    let delta = 45.0 / half as f32;
    assert!(
        (whole - half) as f32 * delta < 55.0,
        "the second stretch is no longer than the first: {half} then {whole}"
    );

    let subject = entity(0);
    for (source, faults) in [(&patient, false), (&unbroken, true)] {
        let mut stage = Stage::new(source, &[(subject, "Worker")]);
        stage.frame_with(delta, SLICE, &event(subject, "Go"));
        let mut finished = false;
        for _ in 0..(whole * 2) {
            stage.frame(delta, SLICE);
            if stage.int(subject, "Worker", "stage") == Some(2) || stage.disabled(subject, "Worker")
            {
                finished = stage.int(subject, "Worker", "stage") == Some(2);
                break;
            }
        }
        assert_eq!(
            stage.disabled(subject, "Worker"),
            faults,
            "faulted? the `await` makes the difference ({} s of game time)",
            stage.elapsed
        );
        assert_eq!(finished, !faults, "the one with the `await` finished");
    }
}

/// **A body that finishes inside ten seconds is never reported**, run after
/// run for over a minute — while a spinner over the same frames is.
#[test]
fn a_body_finishing_inside_ten_seconds_is_never_reported() {
    let source = "behavior Brisk {
                      int runs = 0;
                      void Update(float dt) {
                          int i = 0;
                          while (i < 40) { WatchdogWall(); i += 1; }
                          runs += 1;
                      }
                  }";
    let per_run = frames_to(source, "Brisk", "", "runs", 1);
    let delta = 5.0 / per_run as f32;
    let subject = entity(0);
    let mut brisk = Stage::new(source, &[(subject, "Brisk")]);
    let (mut spinning, spinner) = spinner();
    while brisk.elapsed < 80.0 {
        brisk.frame(delta, SLICE);
        spinning.frame(delta, SLICE);
    }

    assert!(brisk
        .int(subject, "Brisk", "runs")
        .is_some_and(|runs| runs >= 10));
    assert_eq!(
        brisk.warnings(subject, "Brisk"),
        0,
        "every run inside ten seconds"
    );
    assert!(!brisk.disabled(subject, "Brisk"));
    assert_eq!(
        spinning.warnings(spinner, "Spinner"),
        1,
        "the spinner reported"
    );
}

/// **A body finishing after thirty seconds is reported, never faulted, and
/// the next run counts from zero**: three runs of about thirty-five seconds
/// each — over a minute and a half in all — are reported once each and never
/// fault.
#[test]
fn a_body_finishing_after_thirty_seconds_is_reported_per_run_and_never_faulted() {
    let source = "behavior Slowpoke {
                      int runs = 0;
                      void Update(float dt) {
                          int i = 0;
                          while (i < 40) { WatchdogWall(); i += 1; }
                          runs += 1;
                      }
                  }";
    let per_run = frames_to(source, "Slowpoke", "", "runs", 1);
    let three = frames_to(source, "Slowpoke", "", "runs", 3);
    let delta = 35.0 / per_run as f32;
    assert!(
        three as f32 * delta > 90.0 && (three as f32 * delta) < 3.0 * 55.0,
        "three runs of about 35 s: {per_run} frames, then {three} for three"
    );

    let subject = entity(0);
    let mut stage = Stage::new(source, &[(subject, "Slowpoke")]);
    for _ in 0..(three * 2) {
        stage.frame(delta, SLICE);
        if stage.int(subject, "Slowpoke", "runs") == Some(3) {
            break;
        }
    }
    assert_eq!(stage.int(subject, "Slowpoke", "runs"), Some(3));
    assert!(
        !stage.disabled(subject, "Slowpoke"),
        "no run reached a minute"
    );
    assert_eq!(
        stage.warnings(subject, "Slowpoke"),
        3,
        "each run reported once: {:?}",
        stage.slow
    );
}
