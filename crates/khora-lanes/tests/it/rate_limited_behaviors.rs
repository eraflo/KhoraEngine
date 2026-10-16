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

//! **`[Rate(f)]` — an instance takes its turn at most `f` times a second,**
//! on frames staggered by its entity, and loses no time between its turns:
//! the frames it skips add their delta to what it is owed, and its turn is
//! handed all of it — as `Update`'s `dt`, to its timers, to an `await`. A
//! skipped frame is not a deferral; mail waits for the turn. The owed time is
//! saved, and kept across a hot reload. A behavior with no rate runs every
//! frame with the frame's delta, as before.
//!
//! And the same rule for an instance that sits out for lack of fuel: the time
//! it missed is owed, not lost.
//!
//! The frames below last 1/64 s and the rate is 8 Hz — a period of exactly
//! eight frames, every sum exact in binary — except where the sixty-frames,
//! ten-hertz case is the point.

use std::collections::{BTreeMap, BTreeSet};

use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot};
use khora_lanes::script_lane::{ScriptRunReport, ScriptRuntime};

use super::saves::{loaded, through_record};
use super::scheduled_scene::{
    build, float_of, frame, frame_with, int_of, per_turn, runtime_of, view, PLENTY,
};

const MODULE: &str = "paced.erg";

/// A frame of 1/64 s.
const DELTA: f32 = 1.0 / 64.0;

/// Frames in one period of `[Rate(8hz)]` at [`DELTA`].
const PERIOD: usize = 8;

/// `Paced`, at 8 Hz, counts its turns, the time they were handed, its
/// timer's firings and what it heard; `Steady`, with no rate, the same.
const PACED: &str = "[Rate(8hz)]
behavior Paced {
    int turns = 0;
    float total = 0.0;
    float last = 0.0;
    int fired = 0;
    int hits = 0;
    void Update(float dt) { turns += 1; total += dt; last = dt; }
    every 0.25s { fired += 1; }
    on Hit() { hits += 1; }
}
behavior Steady {
    int turns = 0;
    float last = 0.0;
    void Update(float dt) { turns += 1; last = dt; }
}
event Hit;";

fn turns(runtime: &ScriptRuntime, behavior: &str, index: u32) -> i64 {
    int_of(runtime, MODULE, index, behavior, "turns")
}

/// Runs frames until `index`'s `Paced` takes a turn, at most two periods;
/// how many frames that took.
fn until_a_turn(view: &khora_data::flow::ScriptView, runtime: &mut ScriptRuntime) -> usize {
    let before = turns(runtime, "Paced", 0);
    for at in 1..=2 * PERIOD {
        frame(view, runtime, PLENTY);
        if turns(runtime, "Paced", 0) > before {
            return at;
        }
    }
    panic!("`Paced` took no turn in two periods");
}

/// **A rate-limited instance runs once a period, and a frame it skips is not
/// a deferral.** Sixteen `Paced` at 8 Hz and one `Steady`, 1/64-s frames,
/// plenty of fuel. Over two periods each `Paced` runs exactly twice, eight
/// frames apart; `Steady` runs every frame, handed the frame's delta; the
/// report never counts a deferral.
#[test]
fn a_rate_limited_instance_runs_once_a_period_and_a_skip_is_not_a_deferral() {
    let paced: Vec<u32> = (0..16).collect();
    let view = view(MODULE, &[("Paced", &paced), ("Steady", &[100])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    for _ in 0..2 * PERIOD {
        frame(&view, &mut runtime, PLENTY);
    }

    let mut on: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for at in 0..2 * PERIOD {
        let before: Vec<i64> = paced.iter().map(|&i| turns(&runtime, "Paced", i)).collect();
        let steady = turns(&runtime, "Steady", 100);
        let report = frame(&view, &mut runtime, PLENTY);
        assert_eq!(
            report.deferred, 0,
            "frame {at}: a skipped frame is not a deferral — {report:?}"
        );
        assert_eq!(
            (
                turns(&runtime, "Steady", 100) - steady,
                float_of(&runtime, MODULE, 100, "Steady", "last")
            ),
            (1, DELTA),
            "frame {at}: a behavior with no rate runs every frame on the frame's delta"
        );
        for (&index, before) in paced.iter().zip(&before) {
            let now = turns(&runtime, "Paced", index);
            assert!(now - before <= 1, "frame {at}: entity {index} ran twice");
            if now > *before {
                on.entry(index).or_default().push(at);
            }
        }
    }

    for &index in &paced {
        let frames = on.get(&index).cloned().unwrap_or_default();
        assert_eq!(
            frames.len(),
            2,
            "entity {index}: one turn a period, two in two — ran on frames {frames:?}"
        );
        assert_eq!(
            frames[1] - frames[0],
            PERIOD,
            "entity {index}: its turns are a period apart — {frames:?}"
        );
    }
    let mut per_frame: BTreeMap<usize, usize> = BTreeMap::new();
    for frames in on.values() {
        *per_frame.entry(frames[0]).or_default() += 1;
    }
    assert!(
        per_frame.len() > 1 && per_frame.values().all(|&count| count <= 8),
        "the instances are staggered over the period, not run together: {per_frame:?}"
    );
}

/// The frames, of `frames` after a two-period warm-up, on which each `Paced`
/// of `order` took its turn.
fn schedule(order: &[u32], frames: usize) -> BTreeMap<u32, Vec<usize>> {
    let view = view(MODULE, &[("Paced", order)], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    for _ in 0..2 * PERIOD {
        frame(&view, &mut runtime, PLENTY);
    }
    let mut on: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for at in 0..frames {
        let before: Vec<i64> = order.iter().map(|&i| turns(&runtime, "Paced", i)).collect();
        frame(&view, &mut runtime, PLENTY);
        for (&index, before) in order.iter().zip(&before) {
            if turns(&runtime, "Paced", index) > *before {
                on.entry(index).or_default().push(at);
            }
        }
    }
    on
}

/// **The stagger is the entity's, not the view's.** The same sixteen `Paced`
/// listed in entity order and in reverse take their turns on the same frames:
/// four turns each in four periods, on the same frames both ways.
#[test]
fn rate_limited_turns_are_staggered_by_entity_whatever_the_view_order() {
    let forward: Vec<u32> = (0..16).collect();
    let backward: Vec<u32> = forward.iter().rev().copied().collect();

    let one = schedule(&forward, 4 * PERIOD);
    let other = schedule(&backward, 4 * PERIOD);

    for &index in &forward {
        let frames = one.get(&index).cloned().unwrap_or_default();
        assert_eq!(
            frames.len(),
            4,
            "entity {index}: four turns in four periods — {frames:?}"
        );
    }
    assert_eq!(one, other, "the same frames whatever the view's order");
}

/// **At sixty frames a second, ten hertz is about every sixth frame.** Twelve
/// instances at `[Rate(10hz)]`, two seconds of 1/60-s frames: each takes about
/// twenty turns, each handed about a tenth of a second, and no frame is a
/// deferral.
#[test]
fn a_ten_hertz_instance_at_sixty_frames_a_second_runs_about_every_sixth_frame() {
    let source = "[Rate(10hz)]
         behavior Paced {
             int turns = 0;
             float last = 0.0;
             void Update(float dt) { turns += 1; last = dt; }
         }";
    let delta = 1.0 / 60.0;
    let paced: Vec<u32> = (0..12).collect();
    let view = view(MODULE, &[("Paced", &paced)], delta);
    let mut runtime = runtime_of(MODULE, source);
    for _ in 0..12 {
        frame(&view, &mut runtime, PLENTY);
    }
    let before: Vec<i64> = paced.iter().map(|&i| turns(&runtime, "Paced", i)).collect();

    for at in 0..120 {
        let report = frame(&view, &mut runtime, PLENTY);
        assert_eq!(report.deferred, 0, "frame {at}: {report:?}");
    }
    for (&index, before) in paced.iter().zip(&before) {
        let ran = turns(&runtime, "Paced", index) - before;
        assert!(
            (19..=21).contains(&ran),
            "entity {index}: {ran} turns in two seconds at 10 Hz"
        );
        let last = float_of(&runtime, MODULE, index, "Paced", "last");
        assert!(
            last >= 0.1 - 1e-4 && last <= 0.1 + delta + 1e-4,
            "entity {index}: its turn is handed the tenth of a second since the last — {last}"
        );
    }
}

/// **A turn is handed the time since the last one.** One `Paced` and one
/// `Steady` over eight periods: at each of `Paced`'s turns `dt` is the eight
/// frames since its previous turn, and its running total grows by exactly
/// that — no time lost, none counted twice. `Steady` is handed 1/64 s every
/// frame.
#[test]
fn a_rate_limited_turn_is_handed_the_time_since_its_last_turn() {
    let view = view(MODULE, &[("Paced", &[0]), ("Steady", &[100])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    until_a_turn(&view, &mut runtime);

    let mut since = 0;
    let mut total = float_of(&runtime, MODULE, 0, "Paced", "total");
    let mut seen = 0;
    for at in 0..8 * PERIOD {
        let before = turns(&runtime, "Paced", 0);
        frame(&view, &mut runtime, PLENTY);
        since += 1;
        assert_eq!(
            float_of(&runtime, MODULE, 100, "Steady", "last"),
            DELTA,
            "frame {at}: no rate, the frame's delta"
        );
        if turns(&runtime, "Paced", 0) > before {
            let owed = since as f32 * DELTA;
            let now = float_of(&runtime, MODULE, 0, "Paced", "total");
            assert_eq!(
                (
                    since,
                    float_of(&runtime, MODULE, 0, "Paced", "last"),
                    now - total
                ),
                (PERIOD, owed, owed),
                "frame {at}: handed the {since} frames since its last turn"
            );
            total = now;
            since = 0;
            seen += 1;
        }
    }
    assert!(seen >= 7, "eight periods, {seen} turns");
}

/// **A rate-limited instance's timers run on the time it skipped.** Four
/// seconds of 1/64-s frames: `Paced` takes about thirty-two turns, and its
/// `every 0.25s` fires about sixteen times — not the two it would if each turn
/// moved the timer by one frame's delta.
#[test]
fn a_rate_limited_instance_s_timers_count_the_skipped_time() {
    let view = view(MODULE, &[("Paced", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    until_a_turn(&view, &mut runtime);
    let (turned, fired) = (
        turns(&runtime, "Paced", 0),
        int_of(&runtime, MODULE, 0, "Paced", "fired"),
    );

    for _ in 0..256 {
        frame(&view, &mut runtime, PLENTY);
    }

    let turned = turns(&runtime, "Paced", 0) - turned;
    let fired = int_of(&runtime, MODULE, 0, "Paced", "fired") - fired;
    assert!(
        (31..=33).contains(&turned),
        "four seconds at 8 Hz: {turned} turns"
    );
    assert!(
        (15..=17).contains(&fired),
        "four seconds of `every 0.25s`: fired {fired} times"
    );
}

/// **An `await` in a rate-limited instance counts the skipped time.**
/// `Striker` at 8 Hz hears `Go` and awaits a quarter of a second: the strike
/// lands sixteen to twenty-four frames (a quarter second, give or take a
/// period) after it heard — two of its turns, not sixteen — and in between
/// the instance takes its turns at its rate, not every frame.
#[test]
fn an_await_in_a_rate_limited_instance_counts_the_skipped_time() {
    let source = "[Rate(8hz)]
         behavior Striker {
             int heard = 0;
             int struck = 0;
             int turns = 0;
             async void Strike() { await 0.25s; struck += 1; }
             on Go() { heard += 1; Strike(); }
             void Update(float dt) { turns += 1; }
         }
         event Go;";
    let view = view(MODULE, &[("Striker", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, source);
    for _ in 0..2 * PERIOD {
        frame(&view, &mut runtime, PLENTY);
    }

    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(super::scheduled_scene::entity(0), "Go"));
    let mut heard_at = None;
    for at in 0..2 * PERIOD {
        let report = frame_with(&view, &mail, &mut runtime, PLENTY);
        mail = report.undelivered;
        if int_of(&runtime, MODULE, 0, "Striker", "heard") == 1 {
            heard_at = Some(at);
            break;
        }
    }
    assert!(heard_at.is_some(), "`Go` was heard within two periods");

    let mut waited = 0;
    let mut ran_on = 0;
    while int_of(&runtime, MODULE, 0, "Striker", "struck") == 0 {
        assert!(waited < 4 * PERIOD, "the strike never landed");
        let report = frame_with(&view, &mail, &mut runtime, PLENTY);
        mail = report.undelivered;
        waited += 1;
        if report.completed > 0 {
            ran_on += 1;
        }
    }
    assert!(
        (16..=24).contains(&waited),
        "a quarter second of 1/64-s frames: struck {waited} frames after hearing"
    );
    assert!(
        ran_on <= 4,
        "the waiting instance took {ran_on} turns in {waited} frames — its rate's, not every frame's"
    );
}

/// **Mail for a rate-limited instance waits for its turn, and is heard
/// once.** A `Hit` sent on the frame after a turn is not heard on the frames
/// `Paced` skips; it is heard on its next turn — the very frame `turns` moves
/// — and never again.
#[test]
fn an_event_for_a_rate_limited_instance_waits_for_its_turn_and_is_heard_once() {
    let view = view(MODULE, &[("Paced", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    until_a_turn(&view, &mut runtime);

    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(super::scheduled_scene::entity(0), "Hit"));
    let turned = turns(&runtime, "Paced", 0);
    let report = frame_with(&view, &mail, &mut runtime, PLENTY);
    assert_eq!(
        (
            turns(&runtime, "Paced", 0) - turned,
            int_of(&runtime, MODULE, 0, "Paced", "hits")
        ),
        (0, 0),
        "the frame after a turn is skipped, and the `Hit` waits"
    );
    mail = report.undelivered;

    let mut heard_on_its_turn = false;
    for _ in 0..PERIOD {
        let before = turns(&runtime, "Paced", 0);
        let report = frame_with(&view, &mail, &mut runtime, PLENTY);
        mail = report.undelivered;
        let hits = int_of(&runtime, MODULE, 0, "Paced", "hits");
        if turns(&runtime, "Paced", 0) > before {
            assert_eq!(hits, 1, "heard on its turn");
            heard_on_its_turn = true;
            break;
        }
        assert_eq!(hits, 0, "not heard on a frame it skips");
    }
    assert!(heard_on_its_turn, "its next turn came within a period");

    for _ in 0..3 * PERIOD {
        let report = frame_with(&view, &mail, &mut runtime, PLENTY);
        mail = report.undelivered;
    }
    assert_eq!(
        int_of(&runtime, MODULE, 0, "Paced", "hits"),
        1,
        "one `Hit`, heard once"
    );
}

/// The latest record the lane wrote of `index`'s `Paced` among `reports` —
/// what the World's companion holds once each frame's writeback has landed.
fn latest_record(reports: &[ScriptRunReport], index: u32) -> ScriptSnapshot {
    reports
        .iter()
        .rev()
        .find_map(|report| {
            report
                .state
                .iter()
                .find(|update| update.entity.index == index && update.behavior == "Paced")
                .map(|update| update.snapshot.clone())
        })
        .expect("the lane wrote the instance's state")
}

/// **The time owed is saved.** `Paced` takes a turn, then skips three
/// frames; the game is saved there, through the scene's record, and loaded
/// into a fresh runtime. Its next turn is handed the three frames before the
/// save as well as those after the load: its running total — saved with its
/// fields — grows by exactly that.
#[test]
fn a_rate_limited_instance_s_owed_time_survives_a_save() {
    let view = view(MODULE, &[("Paced", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    let mut reports = Vec::new();
    for _ in 0..2 * PERIOD {
        reports.push(frame(&view, &mut runtime, PLENTY));
    }
    // Up to a turn, recorded.
    let before = turns(&runtime, "Paced", 0);
    let mut turned = false;
    for _ in 0..2 * PERIOD {
        reports.push(frame(&view, &mut runtime, PLENTY));
        if turns(&runtime, "Paced", 0) > before {
            turned = true;
            break;
        }
    }
    assert!(turned, "`Paced` took a turn within two periods");
    let total = float_of(&runtime, MODULE, 0, "Paced", "total");
    let turned = turns(&runtime, "Paced", 0);
    for _ in 0..3 {
        reports.push(frame(&view, &mut runtime, PLENTY));
    }
    assert_eq!(
        turns(&runtime, "Paced", 0),
        turned,
        "three frames skipped after a turn"
    );
    let saved = through_record(&latest_record(&reports, 0));

    // Closed and opened again.
    let mut runtime = runtime_of(MODULE, PACED);
    let mut arriving = view.clone();
    arriving.instances[0].arrival = Some(loaded(saved));
    frame(&arriving, &mut runtime, PLENTY);
    let mut after = 1;
    while turns(&runtime, "Paced", 0) == turned {
        assert!(after < 2 * PERIOD, "no turn in two periods after the load");
        frame(&view, &mut runtime, PLENTY);
        after += 1;
    }

    let handed = float_of(&runtime, MODULE, 0, "Paced", "total") - total;
    assert_eq!(
        handed,
        (3 + after) as f32 * DELTA,
        "the turn after the load is handed the three frames before the save and the {after} after"
    );
}

/// **The time owed is kept across a hot reload.** `Paced` takes a turn,
/// skips three frames, and its module is edited — a field added above the
/// others. Its next turn is handed the three frames before the edit as well
/// as those after.
#[test]
fn a_rate_limited_instance_s_owed_time_survives_a_hot_reload() {
    let view = view(MODULE, &[("Paced", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, PACED);
    for _ in 0..2 * PERIOD {
        frame(&view, &mut runtime, PLENTY);
    }
    until_a_turn(&view, &mut runtime);
    let total = float_of(&runtime, MODULE, 0, "Paced", "total");
    let turned = turns(&runtime, "Paced", 0);
    for _ in 0..3 {
        frame(&view, &mut runtime, PLENTY);
    }
    assert_eq!(turns(&runtime, "Paced", 0), turned, "three frames skipped");

    let edited = PACED.replacen("int turns = 0;", "int extra = 0;\n    int turns = 0;", 1);
    runtime.reload(MODULE, build(&edited));
    let mut after = 0;
    while turns(&runtime, "Paced", 0) == turned {
        assert!(after < 2 * PERIOD, "no turn in two periods after the edit");
        frame(&view, &mut runtime, PLENTY);
        after += 1;
    }

    let handed = float_of(&runtime, MODULE, 0, "Paced", "total") - total;
    assert_eq!(
        handed,
        (3 + after) as f32 * DELTA,
        "the turn after the edit is handed the three frames before it and the {after} after"
    );
}

/// `Walker`, with no rate: counts its turns and the time they were handed —
/// every turn the same cost, so a pool of whole turns cuts none.
const WALKER: &str = "behavior Walker {
    int turns = 0;
    float total = 0.0;
    float last = 0.0;
    void Update(float dt) { turns += 1; total += dt; last = dt; }
}";

/// `Walker` with a timer.
const TIMED_WALKER: &str = "behavior Walker {
    int turns = 0;
    int fired = 0;
    void Update(float dt) { turns += 1; }
    every 0.125s { fired += 1; }
}";

/// **An instance that sits out for lack of fuel is handed the time it
/// missed.** Ten `Walker`s and fuel for three turns a frame: each sits out
/// some frames between its turns. At every turn `dt` is the frames since its
/// previous turn, and its total grows by exactly that — the frames it sat out
/// are owed, not lost.
#[test]
fn an_instance_that_sits_out_for_fuel_is_handed_the_time_it_missed() {
    let walkers: Vec<u32> = (0..10).collect();
    let view = view(MODULE, &[("Walker", &walkers)], DELTA);
    let mut runtime = runtime_of(MODULE, WALKER);
    let turn = per_turn(&view, &mut runtime, 10);

    let mut since = vec![0usize; walkers.len()];
    let mut total: Vec<f32> = walkers
        .iter()
        .map(|&i| float_of(&runtime, MODULE, i, "Walker", "total"))
        .collect();
    let mut gaps = BTreeSet::new();
    for at in 0..40 {
        let before: Vec<i64> = walkers
            .iter()
            .map(|&i| int_of(&runtime, MODULE, i, "Walker", "turns"))
            .collect();
        frame(&view, &mut runtime, turn * 3);
        for (n, &index) in walkers.iter().enumerate() {
            since[n] += 1;
            if int_of(&runtime, MODULE, index, "Walker", "turns") > before[n] {
                let owed = since[n] as f32 * DELTA;
                let now = float_of(&runtime, MODULE, index, "Walker", "total");
                assert_eq!(
                    (
                        float_of(&runtime, MODULE, index, "Walker", "last"),
                        now - total[n]
                    ),
                    (owed, owed),
                    "frame {at}, entity {index}: handed the {} frames since its last turn",
                    since[n]
                );
                gaps.insert(since[n]);
                total[n] = now;
                since[n] = 0;
            }
        }
    }
    assert!(
        gaps.iter().any(|&gap| gap > 1),
        "set-up: some instance sat out a frame — gaps {gaps:?}"
    );
}

/// **An instance that sits out for lack of fuel keeps its timers on time.**
/// Ten `Walker`s, fuel for about three turns a frame, four seconds: each
/// `every 0.125s` fires close to its thirty-two times, not the handful it
/// would if the frames it sat out stopped its clock.
#[test]
fn an_instance_that_sits_out_for_fuel_keeps_its_timers_on_time() {
    let walkers: Vec<u32> = (0..10).collect();
    let view = view(MODULE, &[("Walker", &walkers)], DELTA);
    let mut runtime = runtime_of(MODULE, TIMED_WALKER);
    let turn = per_turn(&view, &mut runtime, 10);
    let before: Vec<i64> = walkers
        .iter()
        .map(|&i| int_of(&runtime, MODULE, i, "Walker", "fired"))
        .collect();

    for _ in 0..256 {
        frame(&view, &mut runtime, turn * 3);
    }

    for (&index, before) in walkers.iter().zip(&before) {
        let fired = int_of(&runtime, MODULE, index, "Walker", "fired") - before;
        assert!(
            fired >= 28,
            "entity {index}: four seconds of `every 0.125s` fired {fired} times"
        );
    }
}

/// `Walker` dividing by zero on every turn.
const FAULTING_WALKER: &str = "behavior Walker {
    int zero = 0;
    int n = 1;
    float last = 0.0;
    void Update(float dt) { last = dt; n = n / zero; }
}";

/// The same `Walker`, edited so it no longer faults.
const FIXED_WALKER: &str = "behavior Walker {
    int zero = 0;
    int n = 1;
    float last = 0.0;
    void Update(float dt) { last = dt; n = n / (zero + 1); }
}";

/// **A faulted instance owes no time.** `Walker` faults on its first turn and
/// is disabled; a second of frames passes; an edit fixes it and re-enables
/// it. Its first `Update` since is handed the frame's delta — a disabled
/// instance was not kept out by its rate nor by the fuel, so the second it
/// spent disabled is not time its next turn makes up.
#[test]
fn an_instance_re_enabled_by_an_edit_is_handed_the_frame_s_delta() {
    let view = view(MODULE, &[("Walker", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, FAULTING_WALKER);
    let report = frame(&view, &mut runtime, PLENTY);
    assert_eq!(report.faulted, 1, "set-up: it faulted — {report:?}");
    for _ in 0..64 {
        frame(&view, &mut runtime, PLENTY);
    }

    runtime.reload(MODULE, build(FIXED_WALKER));
    let report = frame(&view, &mut runtime, PLENTY);
    assert_eq!(
        (report.completed, report.faulted),
        (1, 0),
        "set-up: the edit re-enabled it — {report:?}"
    );
    assert_eq!(
        float_of(&runtime, MODULE, 0, "Walker", "last"),
        DELTA,
        "the turn after the fix is handed the frame's delta, not the second it lay disabled"
    );
}

/// **An edit that raises an instance's rate takes effect within the new
/// period.** `Paced` at 1 Hz takes a turn; its module is edited to 8 Hz. Its
/// next turn comes once it is owed a period of the new rate — within eight
/// frames, give or take one — not after the rest of the old one-second wait.
#[test]
fn an_edit_raising_a_rate_takes_effect_within_the_new_period() {
    let slow = PACED.replacen("[Rate(8hz)]", "[Rate(1hz)]", 1);
    let view = view(MODULE, &[("Paced", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, &slow);
    let before = turns(&runtime, "Paced", 0);
    let mut turned = false;
    for _ in 0..2 * 64 {
        frame(&view, &mut runtime, PLENTY);
        if turns(&runtime, "Paced", 0) > before {
            turned = true;
            break;
        }
    }
    assert!(
        turned,
        "set-up: `Paced` took a turn at 1 Hz within two seconds"
    );
    let turned = turns(&runtime, "Paced", 0);

    runtime.reload(MODULE, build(PACED));
    let mut after = 0;
    while turns(&runtime, "Paced", 0) == turned {
        after += 1;
        assert!(
            after <= PERIOD + 1,
            "no turn in {after} frames after raising the rate to 8 Hz — a period is {PERIOD}"
        );
        frame(&view, &mut runtime, PLENTY);
    }
}

/// **A rate past the frame's precision still lets the frame end.** `Paced`
/// at ten billion hertz runs every frame, once; the frame returns. Run apart,
/// so a frame that never returns fails the test instead of hanging it.
#[test]
fn a_rate_far_above_the_frame_rate_runs_once_a_frame_and_the_frame_ends() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let source = "[Rate(10000000000hz)]
             behavior Paced { int turns = 0; void Update(float dt) { turns += 1; } }";
        let view = view(MODULE, &[("Paced", &[0])], DELTA);
        let mut runtime = runtime_of(MODULE, source);
        for _ in 0..4 {
            frame(&view, &mut runtime, PLENTY);
        }
        let _ = done.send(turns(&runtime, "Paced", 0));
    });
    let ran = finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("four frames of a ten-gigahertz behavior returned within twenty seconds");
    assert_eq!(ran, 4, "one turn a frame, however high the rate");
}
