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

//! **`[Budget(d)]` — a per-frame ceiling.** A budgeted behavior never spends
//! more than `d` of the frame's time, converted to fuel at the runtime's
//! measured rate (instructions per millisecond); what it would have had
//! beyond passes on to the behaviors after it. A ceiling, not a reservation:
//! a behavior that spends less leaves the rest to the others.
//!
//! [`run_behaviors`](khora_lanes::script_lane::run_behaviors) never corrects
//! the rate — only the lane, timing a whole run, does — so here it is the
//! runtime's starting rate, read through [`ScriptRuntime::rate`].

use khora_lanes::script_lane::ScriptRuntime;

use super::scheduled_scene::{build, frame, int_of, per_turn, ran, runtime_of, view, PLENTY};

const MODULE: &str = "budget.erg";

/// A turn of a few dozen instructions: twenty of them are far past a
/// ceiling of a hundredth of a millisecond.
const LOOPING_BODY: &str =
    "int ticks = 0; void Update(float dt) { int n = 0; while (n < 20) { n += 1; } ticks += 1; }";

/// The ceiling of a hundredth of a millisecond, in fuel, at `runtime`'s rate.
fn hundredth_of_a_ms(runtime: &ScriptRuntime) -> u64 {
    (0.01 * runtime.rate()) as u64
}

/// What one `Capped` turn costs, measured on an unbudgeted twin.
fn capped_turn() -> u64 {
    let source = format!("behavior Capped {{ {LOOPING_BODY} }}");
    let mut runtime = runtime_of(MODULE, &source);
    per_turn(&view(MODULE, &[("Capped", &[0])], 0.016), &mut runtime, 1)
}

/// **A budgeted behavior spends no more than its ceiling a frame, whatever
/// the pool.** Twenty `Capped` instances under `[Budget(0.01ms)]` and plenty
/// of fuel: each frame spends at most the ceiling (plus the longest stretch
/// between two safepoints, the overdraft every turn may run), the instances it
/// cannot reach wait — and its rotation still reaches every one of them.
#[test]
fn a_budgeted_behavior_spends_no_more_than_its_ceiling_a_frame() {
    let source = format!("[Budget(0.01ms)]\nbehavior Capped {{ {LOOPING_BODY} }}");
    let program = build(&source);
    let overdraft = program.max_overdraft();
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program);
    let ceiling = hundredth_of_a_ms(&runtime);
    let turn = capped_turn();
    let instances: Vec<u32> = (0..20).collect();
    assert!(
        turn * 20 > ceiling + overdraft,
        "set-up: twenty turns ({}) are past the ceiling ({ceiling}) and its overdraft ({overdraft})",
        turn * 20
    );
    let view = view(MODULE, &[("Capped", &instances)], 0.016);

    for at in 0..12 {
        let report = frame(&view, &mut runtime, PLENTY);
        assert!(
            report.spent <= ceiling + overdraft,
            "frame {at}: spent {} past a ceiling of {ceiling} (+{overdraft} overdraft) — {report:?}",
            report.spent
        );
        assert!(
            report.deferred > 0,
            "frame {at}: the instances past the ceiling wait — {report:?}"
        );
    }
    let never: Vec<u32> = instances
        .iter()
        .copied()
        .filter(|&index| int_of(&runtime, MODULE, index, "Capped", "ticks") == 0)
        .collect();
    assert!(never.is_empty(), "never ran in twelve frames: {never:?}");
}

/// **What a budgeted behavior cannot spend passes on.** `Capped` (first,
/// forty instances, `[Budget(0.01ms)]`) and `Tick` after it, with fuel for
/// the ceiling and every `Tick`. Shared equally, `Capped` would take half the
/// pool and leave `Tick` short; held to its ceiling, it leaves `Tick` enough
/// for every instance, every frame.
#[test]
fn what_a_budgeted_behavior_cannot_spend_passes_on_to_the_next() {
    let source = format!(
        "[Budget(0.01ms)]
         behavior Capped {{ {LOOPING_BODY} }}
         behavior Tick {{ int ticks = 0; void Update(float dt) {{ ticks += 1; }} }}"
    );
    let program = build(&source);
    let overdraft = program.max_overdraft();
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program);
    let ceiling = hundredth_of_a_ms(&runtime);
    let capped = capped_turn();
    let tick = {
        let mut alone = runtime_of(MODULE, &source);
        per_turn(&view(MODULE, &[("Tick", &[0])], 0.016), &mut alone, 1)
    };

    // Enough `Tick`s that half the pool does not cover them.
    let ticks = (ceiling + overdraft) / tick + 5;
    let pool = ceiling + overdraft + ticks * tick;
    assert!(
        capped * 40 >= pool / 2,
        "set-up: `Capped` would spend half the pool ({}) unbudgeted: {}",
        pool / 2,
        capped * 40
    );
    let cappeds: Vec<u32> = (0..40).collect();
    let tickers: Vec<u32> = (100..100 + ticks as u32).collect();
    let view = view(MODULE, &[("Capped", &cappeds), ("Tick", &tickers)], 0.016);
    frame(&view, &mut runtime, PLENTY);
    frame(&view, &mut runtime, PLENTY);

    for at in 0..6 {
        let report = frame(&view, &mut runtime, pool);
        assert_eq!(
            ran(&report, "Tick"),
            tickers,
            "frame {at}: `Tick` gets what `Capped` could not spend — {report:?}"
        );
        assert!(
            report.spent <= pool,
            "frame {at}: spent {} of a pool of {pool}",
            report.spent
        );
    }
}

/// **A budget is a ceiling, not a reservation.** `Light`, first, budgeted at
/// a whole millisecond — far more than the frame holds — spends one cheap turn;
/// `Tick` after it gets everything else, and with fuel for exactly the eleven
/// turns every one of them runs, every frame.
#[test]
fn a_budget_is_a_ceiling_not_a_reservation() {
    let source = "[Budget(1ms)]
         behavior Light { int ticks = 0; void Update(float dt) { ticks += 1; } }
         behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }";
    let mut runtime = runtime_of(MODULE, source);
    let tickers: Vec<u32> = (1..11).collect();
    let view = view(MODULE, &[("Light", &[0]), ("Tick", &tickers)], 0.016);
    let turn = per_turn(&view, &mut runtime, 11);
    assert!(
        turn * 11 < runtime.rate() as u64,
        "set-up: the frame holds less than `Light`'s millisecond"
    );

    for at in 0..6 {
        let report = frame(&view, &mut runtime, turn * 11);
        assert_eq!(
            (ran(&report, "Light"), ran(&report, "Tick")),
            (vec![0], tickers.clone()),
            "frame {at}: nothing is held back for `Light` — {report:?}"
        );
    }
}

/// **Under a budget smaller than one turn, every instance still gets its
/// turns.** Two `Capped` under `[Budget(0.0005ms)]` — a ceiling of a few dozen
/// instructions — each turn a loop of hundreds: a turn takes many frames. The
/// rotation does not stay on the instance whose turn the ceiling cut: over
/// four hundred frames both finish turns, not one of them every turn.
#[test]
fn under_a_budget_smaller_than_a_turn_every_instance_still_finishes_turns() {
    let source = "[Budget(0.0005ms)]
         behavior Capped {
             int ticks = 0;
             void Update(float dt) { int n = 0; while (n < 200) { n += 1; } ticks += 1; }
         }";
    let mut runtime = runtime_of(MODULE, source);
    let view = view(MODULE, &[("Capped", &[0, 1])], 0.016);
    for _ in 0..400 {
        frame(&view, &mut runtime, PLENTY);
    }
    let ticks: Vec<i64> = [0, 1]
        .iter()
        .map(|&index| int_of(&runtime, MODULE, index, "Capped", "ticks"))
        .collect();
    assert!(
        ticks.iter().all(|&ticks| ticks >= 2),
        "turns finished in four hundred frames, by entity: {ticks:?}"
    );
}
