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

//! **What scripting asks of a frame is measured.** The runtime's demand is
//! the fuel the due instances need: each priced at its behavior's last
//! *full* turn — from the turn's start to its completion, the pieces of a
//! turn cut across frames summed — a rate-limited instance weighted by the
//! share of frames it runs, and two microseconds at the current rate for an
//! instance whose behavior has completed no turn yet. The `[Critical]`
//! behaviors' part is counted apart: it is what the cheapest row must cover.

use khora_core::script::{EventQueue, ScriptEvent};
use khora_lanes::script::ScriptRuntime;

use super::scheduled_scene::{entity, frame, frame_with, int_of, runtime_of, view, PLENTY};

const MODULE: &str = "demand.erg";

/// A frame of 1/60 s.
const FRAME: f32 = 1.0 / 60.0;

/// `Worker`: a handler of a few hundred iterations, nothing else.
const WORKER: &str = "behavior Worker {
    int done = 0;
    on Work() { int i = 0; while (i < 300) { i += 1; } done += 1; }
}
event Work;";

/// `Worker` handling one `Work`, each frame's slice `fuel`, until it has: the
/// runtime, what the frames spent from the one that delivered it, and the
/// last frame's spending.
fn worked_with(fuel: u64) -> (ScriptRuntime, u64, u64) {
    let view = view(MODULE, &[("Worker", &[0])], FRAME);
    let mut runtime = runtime_of(MODULE, WORKER);
    frame(&view, &mut runtime, PLENTY);
    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), "Work"));
    let (mut spent, mut last) = (0, 0);
    let mut frames = 0;
    while int_of(&runtime, MODULE, 0, "Worker", "done") == 0 {
        assert!(frames < 1_000, "`Work` was never finished");
        let report = frame_with(&view, &mail, &mut runtime, fuel);
        mail = report.undelivered;
        spent += report.spent;
        last = report.spent;
        frames += 1;
    }
    (runtime, spent, last)
}

/// **A turn cut into pieces is priced at their sum.** `Worker`'s handler run
/// whole costs some fuel; cut by a slice of forty units across many frames,
/// the turn that finishes it is priced at every piece it took — about the
/// whole turn — not at the last piece.
#[test]
fn a_turn_cut_into_pieces_is_priced_at_their_sum() {
    let (whole_runtime, whole, _) = worked_with(PLENTY);
    let (cut_runtime, pieces, last) = worked_with(40);
    assert!(
        last * 4 < pieces,
        "set-up: the turn was cut into pieces — last {last} of {pieces}"
    );

    assert_eq!(
        whole_runtime.demand(FRAME).all,
        whole,
        "a turn run whole is priced at what it spent"
    );
    let demand = cut_runtime.demand(FRAME).all;
    assert_eq!(
        demand, pieces,
        "the cut turn is priced at its pieces summed, not at the last ({last})"
    );
    assert!(
        demand.abs_diff(whole) * 10 <= whole,
        "a turn cut into pieces costs about the turn run whole: {demand} vs {whole}"
    );
}

/// One `[Critical]` `Vital` and two `Plain`s, each turn a loop.
const CREW: &str = "[Critical]
behavior Vital { int n = 0; void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; } }
behavior Plain { int n = 0; void Update(float dt) { int i = 0; while (i < 300) { i += 1; } n += 1; } }";

/// **Only the critical behaviors count in the critical demand.** One
/// `[Critical]` `Vital` and two `Plain`s, three frames with plenty: the
/// demand is what a frame of all three spends, and its critical part what
/// `Vital` spends alone.
#[test]
fn only_the_critical_behaviors_count_in_the_critical_demand() {
    let view = view(MODULE, &[("Vital", &[0]), ("Plain", &[1, 2])], FRAME);
    let mut runtime = runtime_of(MODULE, CREW);
    frame(&view, &mut runtime, PLENTY);
    frame(&view, &mut runtime, PLENTY);
    let report = frame(&view, &mut runtime, PLENTY);
    assert_eq!(report.completed, 3, "set-up: {report:?}");
    assert!(
        report.critical_spent > 0 && report.critical_spent < report.spent,
        "set-up: {report:?}"
    );

    let demand = runtime.demand(FRAME);
    assert_eq!(demand.all, report.spent, "every due instance's turn");
    assert_eq!(
        demand.critical, report.critical_spent,
        "`Vital`'s turn alone"
    );
}

/// `Tick` with no rate, a loop per turn.
const UNRATED: &str = "behavior Tick {
    int n = 0;
    void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; }
}";

/// The same `Tick` at 10 Hz.
const TEN_HERTZ: &str = "[Rate(10hz)]
behavior Tick {
    int n = 0;
    void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; }
}";

/// The same `Tick` at 120 Hz.
const HUNDRED_TWENTY_HERTZ: &str = "[Rate(120hz)]
behavior Tick {
    int n = 0;
    void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; }
}";

/// The demand of sixty `Tick`s of `source` after twelve 1/60-s frames.
fn sixty_ticks(source: &str) -> u64 {
    let entities: Vec<u32> = (0..60).collect();
    let view = view(MODULE, &[("Tick", &entities)], FRAME);
    let mut runtime = runtime_of(MODULE, source);
    for _ in 0..12 {
        frame(&view, &mut runtime, PLENTY);
    }
    runtime.demand(FRAME).all
}

/// **A rate-limited instance is weighted by the share of frames it runs.**
/// Sixty `Tick`s at 10 Hz in 1/60-s frames each run a sixth of the frames:
/// their demand is a sixth of sixty unrated ones'. At 120 Hz — more than
/// once a frame — each counts as one.
#[test]
fn a_rate_limited_instance_is_weighted_by_the_share_of_frames_it_runs() {
    let unrated = sixty_ticks(UNRATED);
    let ten_hertz = sixty_ticks(TEN_HERTZ);
    let fast = sixty_ticks(HUNDRED_TWENTY_HERTZ);
    assert!(unrated > 0, "set-up: sixty turns cost something");

    let sixth = unrated as f64 / 6.0;
    assert!(
        (ten_hertz as f64 - sixth).abs() <= sixth * 0.01 + 60.0,
        "sixty at 10 Hz in 1/60-s frames are ten instances' demand: {ten_hertz} vs {unrated} unrated"
    );
    assert_eq!(
        fast, unrated,
        "a rate above the frame rate counts each instance once"
    );
}

/// **Before any turn is known, an instance is two microseconds.** Three
/// `Tick`s seen by a frame with no fuel have completed no turn: each is
/// priced at two microseconds at the runtime's rate.
#[test]
fn an_instance_with_no_known_turn_is_priced_at_two_microseconds() {
    let view = view(MODULE, &[("Tick", &[0, 1, 2])], FRAME);
    let mut runtime = runtime_of(MODULE, UNRATED);
    let report = frame(&view, &mut runtime, 0);
    assert_eq!(
        (report.completed, runtime.instance_count()),
        (0, 3),
        "set-up: seen, none run — {report:?}"
    );

    let each = 0.002 * runtime.rate();
    let demand = runtime.demand(FRAME);
    assert!(
        (demand.all as f64 - 3.0 * each).abs() <= 3.0,
        "three instances at two microseconds of {} instr/ms: {demand:?}",
        runtime.rate()
    );
    assert_eq!(demand.critical, 0, "none is critical");
}

/// `Door`: a handler and nothing else — a turn with no event runs no code.
const DOOR: &str = "behavior Door {
    int opened = 0;
    on Open() { opened += 1; }
}
event Open;";

/// **A behavior whose turns run nothing is priced at what they took.** Three
/// `Door`s complete three turns each with no `Open` to hear: their turns are
/// whole, and cost nothing, so they are known — priced at zero and measured,
/// not at the two-microsecond guess for a turn never seen.
#[test]
fn a_behavior_whose_turns_run_nothing_is_priced_at_what_they_took() {
    let view = view(MODULE, &[("Door", &[0, 1, 2])], FRAME);
    let mut runtime = runtime_of(MODULE, DOOR);
    let mut completed = 0;
    for _ in 0..3 {
        let report = frame(&view, &mut runtime, PLENTY);
        assert_eq!(
            report.spent, 0,
            "set-up: a quiet turn costs nothing — {report:?}"
        );
        completed += report.completed;
    }
    assert_eq!(completed, 9, "set-up: every turn completed");

    assert!(
        runtime.demand_is_measured(),
        "nine whole turns ran: the price is measured"
    );
    assert_eq!(
        runtime.demand(FRAME).all,
        0,
        "what the whole turns took, not {} instr/ms × 2 µs each",
        runtime.rate()
    );
}
