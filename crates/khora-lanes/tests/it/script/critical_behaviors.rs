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

//! **`[Critical]` — the first claim on the frame's fuel, still bounded.** A
//! critical behavior's need (what it spent last frame) is set aside before the
//! others share what is left; when the pool cannot cover even that, the
//! critical behaviors share the whole pool and the others get none. The frame
//! never spends past its fuel (bar the safepoint overdraft, repaid as debt),
//! and the run order is unchanged: on one entity an earlier behavior still
//! runs first, so the critical one reads what it wrote.

use super::scheduled_scene::{frame, int_of, per_turn, ran, runtime_of, view, PLENTY};

const MODULE: &str = "critical.erg";

/// `Plain` first in the module, `Vital` — critical — after it: the same body,
/// so the same cost a turn.
const PLAIN_THEN_VITAL: &str =
    "behavior Plain { int ticks = 0; void Update(float dt) { ticks += 1; } }
     [Critical]
     behavior Vital { int ticks = 0; void Update(float dt) { ticks += 1; } }";

/// **A critical behavior declared after the others still runs all its
/// instances when the pool is short.** Ten `Plain`, three `Vital`, fuel for
/// four turns. Shared equally, each would get two; `Vital` claims first what
/// it spent last frame — its three turns — and `Plain` takes the one left.
/// Every frame: all of `Vital`, one `Plain`, nine deferred, nothing past the
/// pool.
#[test]
fn a_critical_behavior_declared_last_is_served_first_from_a_short_pool() {
    let mut runtime = runtime_of(MODULE, PLAIN_THEN_VITAL);
    let plain: Vec<u32> = (0..10).collect();
    let vital: Vec<u32> = (10..13).collect();
    let view = view(MODULE, &[("Plain", &plain), ("Vital", &vital)], 0.016);
    let turn = per_turn(&view, &mut runtime, 13);
    let pool = turn * 4;

    for at in 0..6 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Vital"),
            vital,
            "frame {at}: the critical behavior claims its three turns first — {report:?}"
        );
        assert_eq!(
            ran(&report, "Plain").len(),
            1,
            "frame {at}: the others share what is left, one turn — {report:?}"
        );
        assert_eq!(report.deferred, 9, "frame {at}: {report:?}");
        assert!(
            report.spent <= pool,
            "frame {at}: spent {} of a pool of {pool}",
            report.spent
        );
    }
}

/// **A pool too small even for the critical behavior goes to it whole.** Ten
/// `Plain`, six `Vital`, fuel for three turns: `Vital` needs six, so it takes
/// all three and `Plain` gets nothing — still bounded by the pool. Its
/// rotation goes on: over two frames every `Vital` runs.
#[test]
fn a_pool_short_of_the_critical_need_goes_to_the_critical_behavior_alone() {
    let mut runtime = runtime_of(MODULE, PLAIN_THEN_VITAL);
    let plain: Vec<u32> = (0..10).collect();
    let vital: Vec<u32> = (10..16).collect();
    let view = view(MODULE, &[("Plain", &plain), ("Vital", &vital)], 0.016);
    let turn = per_turn(&view, &mut runtime, 16);
    let pool = turn * 3;

    let mut reached = std::collections::BTreeSet::new();
    for at in 0..4 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Plain"),
            Vec::<u32>::new(),
            "frame {at}: the others get none of a pool the critical need exceeds — {report:?}"
        );
        assert_eq!(
            ran(&report, "Vital").len(),
            3,
            "frame {at}: the critical behavior spends the whole pool — {report:?}"
        );
        assert!(
            report.spent <= pool,
            "frame {at}: spent {} of a pool of {pool}",
            report.spent
        );
        reached.extend(ran(&report, "Vital"));
    }
    assert_eq!(
        reached.into_iter().collect::<Vec<_>>(),
        vital,
        "the critical behavior's rotation reaches every instance"
    );
}

/// **Two critical behaviors share a pool short of both their needs; the
/// others get nothing.** `Plain` (ten), then `Vital` and `Medic`, critical,
/// four each; fuel for four turns. The two critical behaviors split it, two
/// turns each, and `Plain` sits out.
#[test]
fn critical_behaviors_share_a_pool_short_of_their_needs_and_the_others_get_none() {
    let source = "behavior Plain { int ticks = 0; void Update(float dt) { ticks += 1; } }
         [Critical]
         behavior Vital { int ticks = 0; void Update(float dt) { ticks += 1; } }
         [Critical]
         behavior Medic { int ticks = 0; void Update(float dt) { ticks += 1; } }";
    let mut runtime = runtime_of(MODULE, source);
    let plain: Vec<u32> = (0..10).collect();
    let vital: Vec<u32> = (10..14).collect();
    let medic: Vec<u32> = (20..24).collect();
    let view = view(
        MODULE,
        &[("Plain", &plain), ("Vital", &vital), ("Medic", &medic)],
        0.016,
    );
    let turn = per_turn(&view, &mut runtime, 18);
    let pool = turn * 4;

    for at in 0..4 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            (
                ran(&report, "Plain").len(),
                ran(&report, "Vital").len(),
                ran(&report, "Medic").len()
            ),
            (0, 2, 2),
            "frame {at}: the critical behaviors split the pool — {report:?}"
        );
        assert!(
            report.spent <= pool,
            "frame {at}: spent {} of a pool of {pool}",
            report.spent
        );
    }
}

/// **Served first, run in its place.** On entity 0 `Leader` (first in the
/// module, ten instances) runs before `Vital` (critical, six instances, one of
/// them on entity 0); `Leader` `Set`s `Vital.seen` to its own count, and
/// `Vital` copies `seen`. Fuel for `Vital`'s six turns and two of `Leader`'s:
/// `Vital` runs all six every frame, and on every frame `Leader` also ran on
/// entity 0, `Vital` read what `Leader` wrote that same frame — the critical
/// claim changed who is served, not the order of the turns.
#[test]
fn a_critical_behavior_still_runs_after_an_earlier_behavior_on_its_entity() {
    let source = "behavior Leader {
             int n = 0;
             void Update(float dt) { n += 1; this.Set(Vital { seen: n }); }
         }
         [Critical]
         behavior Vital {
             int seen = 0;
             int copied = 0;
             void Update(float dt) { copied = seen; }
         }";
    let leaders: Vec<u32> = (0..10).collect();
    let vitals: Vec<u32> = (0..6).collect();
    let both = view(MODULE, &[("Leader", &leaders), ("Vital", &vitals)], 0.016);

    // What one turn of each costs, measured apart.
    let leader_turn = {
        let mut alone = runtime_of(MODULE, source);
        per_turn(&view(MODULE, &[("Leader", &[0])], 0.016), &mut alone, 1)
    };
    let vital_turn = {
        let mut alone = runtime_of(MODULE, source);
        per_turn(&view(MODULE, &[("Vital", &[0])], 0.016), &mut alone, 1)
    };

    let mut runtime = runtime_of(MODULE, source);
    frame(&both, &mut runtime, PLENTY);
    frame(&both, &mut runtime, PLENTY);
    let pool = vital_turn * 6 + leader_turn * 2;

    let mut both_ran = 0;
    for at in 0..10 {
        let leader_before = int_of(&runtime, MODULE, 0, "Leader", "n");
        let report = frame(&both, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Vital"),
            vitals,
            "frame {at}: the critical behavior runs every instance — {report:?}"
        );
        assert!(
            report.spent <= pool,
            "frame {at}: spent {} of a pool of {pool}",
            report.spent
        );
        let leader_now = int_of(&runtime, MODULE, 0, "Leader", "n");
        if leader_now > leader_before {
            both_ran += 1;
            assert_eq!(
                int_of(&runtime, MODULE, 0, "Vital", "copied"),
                leader_now,
                "frame {at}: `Vital` ran after `Leader` on entity 0 and read its write"
            );
        }
    }
    assert!(
        both_ran > 0,
        "`Leader`'s rotation reached entity 0 in ten frames"
    );
}

/// **A critical behavior's claim grows with what it has to do.** `Vital`
/// alone, critical, on three entities, then on six — fuel to spare every
/// frame. Its claim is what it spent last frame, three turns, but a pool that
/// covers all six is not short: once its instances have arrived, every one of
/// them runs every frame and none is deferred.
#[test]
fn a_critical_behavior_that_gains_instances_runs_them_all_from_a_plentiful_pool() {
    let source = "[Critical]
         behavior Vital { int ticks = 0; void Update(float dt) { ticks += 1; } }";
    let mut runtime = runtime_of(MODULE, source);
    let three = view(MODULE, &[("Vital", &[0, 1, 2])], 0.016);
    for _ in 0..3 {
        frame(&three, &mut runtime, PLENTY);
    }

    let six: Vec<u32> = (0..6).collect();
    let grown = view(MODULE, &[("Vital", &six)], 0.016);
    for _ in 0..2 {
        frame(&grown, &mut runtime, PLENTY);
    }
    for at in 0..6 {
        let report = frame(&grown, &mut runtime, PLENTY);
        assert_eq!(
            (ran(&report, "Vital"), report.deferred),
            (six.clone(), 0),
            "frame {at}: a pool of {PLENTY} covers six turns — {report:?}"
        );
    }
}

/// **A critical behavior served short for one frame is served whole again
/// when the pool recovers.** Ten `Plain`, six `Vital` (critical), fuel for
/// twelve turns: `Vital` runs all six. One frame the pool falls to two turns;
/// `Vital` gets both. Back at twelve, its first claim covers its six again —
/// it is not held, frame after frame, to the two it was able to spend while
/// `Plain` takes the rest.
#[test]
fn a_critical_behavior_cut_short_for_one_frame_is_served_whole_once_the_pool_recovers() {
    let mut runtime = runtime_of(MODULE, PLAIN_THEN_VITAL);
    let plain: Vec<u32> = (0..10).collect();
    let vital: Vec<u32> = (10..16).collect();
    let view = view(MODULE, &[("Plain", &plain), ("Vital", &vital)], 0.016);
    let turn = per_turn(&view, &mut runtime, 16);
    let pool = turn * 12;

    for at in 0..3 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Vital"),
            vital,
            "set-up, frame {at}: {report:?}"
        );
    }
    let short = frame(&view, &mut runtime, turn * 2);
    assert_eq!(
        ran(&short, "Vital").len(),
        2,
        "set-up: the short frame serves two `Vital` — {short:?}"
    );

    // One frame to recover, at most.
    frame(&view, &mut runtime, pool);
    for at in 0..6 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Vital"),
            vital,
            "frame {at} after the pool recovered: `Vital` claims first — {report:?}"
        );
    }
}

/// **A rate-limited critical behavior claims the turns it takes, not one for
/// every instance.** Twenty `Plain`, then sixty-four `Vital` — critical, at
/// 8 Hz, 1/64-s frames, so about eight of them are due a frame. Fuel for
/// thirty turns: `Vital` needs about eight, and `Plain` shares the rest —
/// every frame some `Plain` runs, rather than `Vital` setting aside the whole
/// pool for sixty-four turns it will not take.
#[test]
fn a_rate_limited_critical_behavior_claims_only_the_turns_it_takes() {
    let source = "behavior Plain { int ticks = 0; void Update(float dt) { ticks += 1; } }
         [Critical]
         [Rate(8hz)]
         behavior Vital { int ticks = 0; void Update(float dt) { ticks += 1; } }";
    let plain: Vec<u32> = (0..20).collect();
    let vital: Vec<u32> = (100..164).collect();
    let turn = {
        let mut alone = runtime_of(MODULE, source);
        per_turn(
            &view(MODULE, &[("Plain", &plain)], 1.0 / 64.0),
            &mut alone,
            20,
        )
    };
    let both = view(MODULE, &[("Plain", &plain), ("Vital", &vital)], 1.0 / 64.0);
    let mut runtime = runtime_of(MODULE, source);
    let pool = turn * 30;
    for _ in 0..16 {
        frame(&both, &mut runtime, pool);
    }

    for at in 0..16 {
        let report = frame(&both, &mut runtime, pool);
        assert!(
            !ran(&report, "Plain").is_empty(),
            "frame {at}: `Plain` ran none, though the frame spent {} of a pool of {pool} — {} turns completed",
            report.spent,
            report.completed
        );
    }
}
