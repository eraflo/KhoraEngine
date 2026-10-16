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

//! **A fixed step the fuel does not reach is owed, not lost.** A turn that
//! runs out of fuel between two `FixedUpdate` calls leaves the rest owed, and
//! one cut part-way through a `FixedUpdate` leaves that body part-way — it
//! resumes next turn, and its completion counts its step. What is owed runs
//! first next turn, before `Update`; over many frames every step the frames
//! took is run once, as long as no more than [`MAX_SIM_STEPS`] were owed at
//! once — the owed count is capped there, as the scheduler caps a frame's
//! steps. What is owed, and a body part-way, survive a save and a hot reload;
//! the watchdog counts a `FixedUpdate` that never finishes like any body.
//!
//! `Stepper` counts the steps it began and the ones it finished: a step
//! resumed rather than started again begins once.

use khora_core::script::ScriptSnapshot;
use khora_core::time::MAX_SIM_STEPS;
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::{ScriptRunReport, ScriptRuntime, WATCHDOG_FAULT, WATCHDOG_WARN};

use super::saves::{loaded, through_record};
use super::scheduled_scene::{build, entity, frame, int_of, runtime_of, view, PLENTY};

const MODULE: &str = "owed.erg";

/// A frame of 1/32 s.
const FRAME: f32 = 1.0 / 32.0;

/// The fixed step, 1/64 s.
const STEP: f32 = 1.0 / 64.0;

const STEPPER: &str = "behavior Stepper {
    int begun = 0;
    int steps = 0;
    int updates = 0;
    int seen = 0;
    void FixedUpdate(float dt) {
        begun += 1;
        int n = 0;
        while (n < 40) { n += 1; }
        steps += 1;
    }
    void Update(float dt) { updates += 1; seen = steps; }
}";

/// `Stepper` on entity 0, one frame of [`FRAME`] taking `steps` steps.
fn scene(steps: u32) -> ScriptView {
    let mut scene = view(MODULE, &[("Stepper", &[0])], FRAME);
    scene.fixed_steps = steps;
    scene.fixed_delta_seconds = STEP;
    scene
}

fn int(runtime: &ScriptRuntime, field: &str) -> i64 {
    int_of(runtime, MODULE, 0, "Stepper", field)
}

/// What one fixed step costs, measured over a fresh `Stepper`: a frame of no
/// step against a frame of one, with plenty. The frames below count the one
/// step this runs.
fn step_cost(runtime: &mut ScriptRuntime) -> u64 {
    frame(&scene(0), runtime, PLENTY);
    let none = frame(&scene(0), runtime, PLENTY).spent;
    let one = frame(&scene(1), runtime, PLENTY).spent;
    assert_eq!(int(runtime, "steps"), 1, "set-up: one step, one call");
    let step = one.saturating_sub(none);
    assert!(
        step > 40,
        "set-up: a step costs its loop — {none} then {one}"
    );
    step
}

/// **Every step the frames took runs once, in the end.** Forty frames
/// alternating two steps and none, with fuel for a step and a half a frame:
/// the fuel falls short on the frames of two — `steps` lags what the frames
/// took — and is enough on average. Once the fuel is plenty, the next turn
/// runs every step still owed before its `Update`, and the count is exactly
/// the steps the frames took — each begun once: a step cut part-way was
/// resumed, not started again.
#[test]
fn steps_the_fuel_does_not_reach_are_owed_and_every_step_runs_in_the_end() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    let step = step_cost(&mut runtime);
    let mut took = 1;
    let mut lagged = false;

    for at in 0..40 {
        let steps = if at % 2 == 0 { 2 } else { 0 };
        took += steps;
        frame(&scene(steps), &mut runtime, step * 3 / 2);
        lagged |= int(&runtime, "steps") < i64::from(took);
    }
    assert!(lagged, "set-up: the fuel fell short of some step");

    frame(&scene(0), &mut runtime, PLENTY);
    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "seen")),
        (i64::from(took), i64::from(took)),
        "with plenty, every owed step ran — before `Update`"
    );
    for _ in 0..3 {
        frame(&scene(0), &mut runtime, PLENTY);
    }
    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "begun")),
        (i64::from(took), i64::from(took)),
        "{took} steps taken, each run once and begun once"
    );
}

/// **What is owed is capped at the step cap.** Four frames of three steps
/// with no fuel at all — twelve steps, none reached — then plenty: exactly
/// [`MAX_SIM_STEPS`] run, before that turn's `Update`, and no more after.
#[test]
fn owed_steps_are_capped_at_the_simulation_step_cap() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    frame(&scene(0), &mut runtime, PLENTY);

    for at in 0..4 {
        let report = frame(&scene(3), &mut runtime, 0);
        assert_eq!(
            report.deferred, 1,
            "set-up: frame {at} sat out — {report:?}"
        );
    }
    assert_eq!(int(&runtime, "steps"), 0, "set-up: nothing ran");

    frame(&scene(0), &mut runtime, PLENTY);
    let cap = i64::from(MAX_SIM_STEPS);
    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "seen")),
        (cap, cap),
        "twelve owed, {MAX_SIM_STEPS} run, before `Update`"
    );
    for _ in 0..3 {
        frame(&scene(0), &mut runtime, PLENTY);
    }
    assert_eq!(int(&runtime, "steps"), cap, "and nothing more is owed");
    assert_eq!(MAX_SIM_STEPS, 5);
}

/// The latest record the lane wrote of `Stepper` among `reports` — what the
/// World's companion holds once each frame's writeback has landed.
fn latest_record(reports: &[ScriptRunReport]) -> ScriptSnapshot {
    reports
        .iter()
        .rev()
        .find_map(|report| {
            report
                .state
                .iter()
                .find(|update| update.entity == entity(0) && update.behavior == "Stepper")
                .map(|update| update.snapshot.clone())
        })
        .expect("the lane wrote the instance's state")
}

/// After [`step_cost`], one frame of three steps with fuel for a step and a
/// half: the first runs, the second is cut part-way, the third is owed. Its
/// report.
fn cut_in_the_second_of_three(runtime: &mut ScriptRuntime, step: u64) -> ScriptRunReport {
    let report = frame(&scene(3), runtime, step * 3 / 2);
    assert_eq!(
        (int(runtime, "steps"), int(runtime, "begun")),
        (2, 3),
        "set-up: the first of three ran, the second was begun and cut"
    );
    assert!(
        runtime
            .peek(entity(0), "Stepper")
            .is_some_and(|held| held.pending.is_some()),
        "set-up: the second step is part-way"
    );
    report
}

/// **A step part-way and a step owed survive a save.** The second of three
/// steps is cut part-way and the third owed; the game is saved there, through
/// the scene's record, and loaded into a fresh runtime. With plenty, the
/// second finishes where it stopped — begun once — and the third runs: the
/// four steps taken, each run once.
#[test]
fn a_fixed_update_cut_part_way_and_the_steps_owed_survive_a_save() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    let step = step_cost(&mut runtime);
    let report = cut_in_the_second_of_three(&mut runtime, step);
    let saved = through_record(&latest_record(&[report]));

    // Closed and opened again.
    let mut runtime = runtime_of(MODULE, STEPPER);
    let mut arriving = scene(0);
    arriving.instances[0].arrival = Some(loaded(saved));
    frame(&arriving, &mut runtime, PLENTY);
    for _ in 0..3 {
        frame(&scene(0), &mut runtime, PLENTY);
    }

    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "begun")),
        (4, 4),
        "the step part-way finished, the owed one ran — four taken, each begun once"
    );
}

/// **A step part-way and a step owed survive a hot reload.** The same cut;
/// the module is edited — a behavior added beside `Stepper`, whose code is
/// unchanged. With plenty, the second step finishes where it stopped and the
/// third runs.
#[test]
fn a_fixed_update_cut_part_way_and_the_steps_owed_survive_a_hot_reload() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    let step = step_cost(&mut runtime);
    cut_in_the_second_of_three(&mut runtime, step);

    let edited = format!("{STEPPER}\nbehavior Bystander {{ int idle = 0; }}");
    runtime.reload(MODULE, build(&edited));
    for _ in 0..3 {
        frame(&scene(0), &mut runtime, PLENTY);
    }

    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "begun")),
        (4, 4),
        "the step part-way finished, the owed one ran — four taken, each begun once"
    );
}

/// **Steps owed with no body part-way survive a hot reload that relays the
/// fields out.** Two frames of two steps with no fuel owe four; a field is
/// added above the others; with plenty, the four run before `Update`.
#[test]
fn owed_steps_survive_a_hot_reload_that_moves_the_fields() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    frame(&scene(0), &mut runtime, PLENTY);
    frame(&scene(2), &mut runtime, 0);
    frame(&scene(2), &mut runtime, 0);
    assert_eq!(int(&runtime, "steps"), 0, "set-up: nothing ran");

    let edited = STEPPER.replacen("int begun = 0;", "int extra = 0;\n    int begun = 0;", 1);
    runtime.reload(MODULE, build(&edited));
    frame(&scene(0), &mut runtime, PLENTY);

    assert_eq!(
        (int(&runtime, "steps"), int(&runtime, "seen")),
        (4, 4),
        "the four steps owed before the edit ran after it, before `Update`"
    );
}

/// A `FixedUpdate` that never finishes.
const SPINNER: &str = "behavior Spinner {
    int n = 0;
    void FixedUpdate(float dt) { while (true) { n += 1; } }
}";

/// `frames` one-second frames of one step of `Spinner`; the slow bodies they
/// reported.
fn spin(runtime: &mut ScriptRuntime, frames: usize) -> Vec<khora_lanes::script_lane::Slow> {
    let mut scene = view(MODULE, &[("Spinner", &[0])], 1.0);
    scene.fixed_steps = 1;
    scene.fixed_delta_seconds = STEP;
    let mut slow = Vec::new();
    for _ in 0..frames {
        slow.extend(frame(&scene, runtime, 500).slow);
    }
    slow
}

fn spinner_disabled(runtime: &ScriptRuntime) -> bool {
    runtime
        .peek(entity(0), "Spinner")
        .is_some_and(|held| held.disabled)
}

/// **The watchdog counts a `FixedUpdate` like any body**: cut by fuel frame
/// after frame — one-second frames of one step — it is part-way and quiet
/// for nine seconds, reported once past [`WATCHDOG_WARN`] — naming
/// `FixedUpdate` — and not again through its fiftieth second; past
/// [`WATCHDOG_FAULT`] it faults: disabled, the reason naming the member, the
/// body dropped.
#[test]
fn a_fixed_update_that_never_finishes_is_reported_and_faults_like_any_body() {
    let mut runtime = runtime_of(MODULE, SPINNER);

    let mut slow = spin(&mut runtime, 9);
    assert!(
        runtime
            .peek(entity(0), "Spinner")
            .is_some_and(|held| held.pending.is_some()),
        "after nine frames the `FixedUpdate` is part-way"
    );
    assert!(slow.is_empty(), "nine seconds is fine: {slow:?}");

    slow.extend(spin(&mut runtime, 3));
    assert_eq!(
        slow.len(),
        1,
        "reported past {WATCHDOG_WARN} seconds: {slow:?}"
    );
    assert_eq!(slow[0].member, "FixedUpdate");

    slow.extend(spin(&mut runtime, 46));
    assert_eq!(slow.len(), 1, "reported once: {slow:?}");
    assert!(!spinner_disabled(&runtime), "58 seconds is no fault");

    let mut faulted_by = None;
    for at in 59..=62 {
        spin(&mut runtime, 1);
        if spinner_disabled(&runtime) {
            faulted_by = Some(at);
            break;
        }
    }
    assert!(
        faulted_by.is_some(),
        "faulted by its sixty-second second, past {WATCHDOG_FAULT}"
    );
    let held = runtime.peek(entity(0), "Spinner").expect("the instance");
    let fault = held.fault.as_ref().expect("the fault is recorded");
    assert!(
        fault.reason.contains("FixedUpdate"),
        "the reason names the member: {}",
        fault.reason
    );
    assert!(held.pending.is_none(), "the body is dropped");
}

/// `Stepper` whose `FixedUpdate` divides by zero, before it counts.
const FAULTING_STEPPER: &str = "behavior Stepper {
    int zero = 0;
    int begun = 0;
    int steps = 0;
    int updates = 0;
    int seen = 0;
    void FixedUpdate(float dt) {
        begun += 1;
        steps = steps / zero;
    }
    void Update(float dt) { updates += 1; seen = steps; }
}";

/// **A fault owes no step.** The first of a frame's three steps faults and
/// disables the instance; it lies disabled through two frames of two steps;
/// the author fixes the code. The frame the edit runs it again takes no step:
/// it is handed the frame's steps — none — as it is handed the frame's time,
/// not the steps of the turn that faulted.
#[test]
fn an_edit_running_a_faulted_instance_again_owes_it_no_step_from_before_the_fault() {
    let mut runtime = runtime_of(MODULE, FAULTING_STEPPER);
    frame(&scene(0), &mut runtime, PLENTY);
    let report = frame(&scene(3), &mut runtime, PLENTY);
    assert_eq!(
        report.faulted, 1,
        "set-up: the first step faulted: {report:?}"
    );
    for _ in 0..2 {
        frame(&scene(2), &mut runtime, PLENTY);
    }

    runtime.reload(MODULE, build(STEPPER));
    frame(&scene(0), &mut runtime, PLENTY);

    assert_eq!(
        int(&runtime, "updates"),
        2,
        "set-up: the fixed code ran its `Update`"
    );
    assert_eq!(
        int(&runtime, "steps"),
        0,
        "a frame of no step after the fix calls no `FixedUpdate` — the steps of the \
         turn that faulted are not owed"
    );
}

/// **Steps owed while sitting out survive a save.** With plenty, a frame of no
/// step; then two frames of two steps with no fuel at all: four owed. The game
/// is saved there, from what the lane last wrote to the World, and loaded into
/// a fresh runtime: with plenty, the four run.
#[test]
fn fixed_steps_owed_while_sitting_out_survive_a_save() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    let mut reports = vec![frame(&scene(0), &mut runtime, PLENTY)];
    for _ in 0..2 {
        reports.push(frame(&scene(2), &mut runtime, 0));
    }
    assert_eq!(int(&runtime, "steps"), 0, "set-up: nothing ran");
    let saved = through_record(&latest_record(&reports));

    let mut runtime = runtime_of(MODULE, STEPPER);
    let mut arriving = scene(0);
    arriving.instances[0].arrival = Some(loaded(saved));
    frame(&arriving, &mut runtime, PLENTY);
    frame(&scene(0), &mut runtime, PLENTY);

    assert_eq!(
        int(&runtime, "steps"),
        4,
        "the four steps owed when the game was saved ran after the load"
    );
}

/// Sums the steps it is handed.
const INTEGRATOR: &str = "behavior Integrator {
    float total = 0.0;
    void FixedUpdate(float dt) { total += dt; }
}";

/// **A step owed is the step it was taken at.** Two frames of two steps of
/// 1/120 s with no fuel owe four — 1/30 s of simulated time. The fixed step
/// then becomes 1/30 s — GORNA moving physics to its low-power step — and a
/// frame of no step runs what is owed: `FixedUpdate` is handed the 1/30 s the
/// steps stood for, not four of the new step.
#[test]
fn owed_steps_are_handed_the_step_they_were_taken_at() {
    let mut runtime = runtime_of(MODULE, INTEGRATOR);
    let at = |steps, step| {
        let mut scene = view(MODULE, &[("Integrator", &[0])], FRAME);
        scene.fixed_steps = steps;
        scene.fixed_delta_seconds = step;
        scene
    };
    frame(&at(0, 1.0 / 120.0), &mut runtime, PLENTY);
    for _ in 0..2 {
        frame(&at(2, 1.0 / 120.0), &mut runtime, 0);
    }

    frame(&at(0, 1.0 / 30.0), &mut runtime, PLENTY);

    let total = super::scheduled_scene::float_of(&runtime, MODULE, 0, "Integrator", "total");
    assert!(
        (total - 4.0 / 120.0).abs() < 1e-6,
        "four steps of 1/120 s integrate 1/30 s, whatever the step is now — got {total}"
    );
}

/// `Stepper` with nothing left of its `FixedUpdate`, listening for what an
/// edit abandoned.
const STEPPER_WITHOUT_FIXED_UPDATE: &str = r#"behavior Stepper {
    int begun = 0;
    int steps = 0;
    int updates = 0;
    int seen = 0;
    int lost_fixed = 0;
    int lost_other = 0;
    void Update(float dt) { updates += 1; seen = steps; }
    void OnResumeFailed(string member) {
        if (member == "FixedUpdate") { lost_fixed += 1; } else { lost_other += 1; }
    }
}"#;

/// **An edit removing `FixedUpdate` mid-step abandons that step by its name.**
/// The second of three steps is cut part-way and the third owed; the author
/// removes `FixedUpdate`. The body part-way is answered once, as
/// `FixedUpdate`, the step owed is dropped with the member, and `Update` runs
/// on.
#[test]
fn an_edit_removing_fixed_update_mid_step_is_answered_as_fixed_update() {
    let mut runtime = runtime_of(MODULE, STEPPER);
    let step = step_cost(&mut runtime);
    cut_in_the_second_of_three(&mut runtime, step);
    let updates = int(&runtime, "updates");

    runtime.reload(MODULE, build(STEPPER_WITHOUT_FIXED_UPDATE));
    for _ in 0..3 {
        frame(&scene(1), &mut runtime, PLENTY);
    }

    assert_eq!(
        (int(&runtime, "lost_fixed"), int(&runtime, "lost_other")),
        (1, 0),
        "the body part-way is heard once, named `FixedUpdate`"
    );
    assert_eq!(int(&runtime, "steps"), 2, "no step ran after the edit");
    assert_eq!(
        int(&runtime, "updates"),
        updates + 3,
        "`Update` ran each frame after the edit"
    );
    assert_eq!(
        runtime
            .peek(entity(0), "Stepper")
            .map(|held| held.fixed_owed.len()),
        Some(0),
        "nothing is owed to a member that is gone"
    );
}

// ─── Each step owed is the step it was taken at ─────────────────────────────

/// Writes down, step by step, which step it was handed: a 1 for a short step
/// (1/120 s), a 2 for a long one (1/30 s).
const RECORDER: &str = "behavior Recorder {
    int seq = 0;
    void FixedUpdate(float dt) {
        if (dt > 0.02) { seq = seq * 10 + 2; } else { seq = seq * 10 + 1; }
    }
}";

const SHORT: f32 = 1.0 / 120.0;
const LONG: f32 = 1.0 / 30.0;

/// `Recorder` on entity 0, one frame taking `steps` steps of `step`.
fn recorded(steps: u32, step: f32) -> ScriptView {
    let mut scene = view(MODULE, &[("Recorder", &[0])], FRAME);
    scene.fixed_steps = steps;
    scene.fixed_delta_seconds = step;
    scene
}

fn seq(runtime: &ScriptRuntime) -> i64 {
    int_of(runtime, MODULE, 0, "Recorder", "seq")
}

/// **Owed steps run oldest first, each at its own step.** Two frames of two
/// short steps with no fuel owe four; the step then becomes long, and a frame
/// of one long step with plenty runs the four short ones, then its own.
#[test]
fn owed_steps_run_oldest_first_each_at_the_step_it_was_taken_at() {
    let mut runtime = runtime_of(MODULE, RECORDER);
    frame(&recorded(0, SHORT), &mut runtime, PLENTY);
    for _ in 0..2 {
        frame(&recorded(2, SHORT), &mut runtime, 0);
    }

    frame(&recorded(1, LONG), &mut runtime, PLENTY);

    assert_eq!(seq(&runtime), 11112, "four short steps, then the long one");
}

/// **The cap keeps the oldest steps.** Three frames of two short steps with
/// no fuel take six — five are owed, the sixth dropped; a frame of two long
/// steps with no fuel adds none. With plenty, the five short ones run.
#[test]
fn the_step_cap_keeps_the_oldest_steps_owed_and_drops_the_newest() {
    let mut runtime = runtime_of(MODULE, RECORDER);
    frame(&recorded(0, SHORT), &mut runtime, PLENTY);
    for _ in 0..3 {
        frame(&recorded(2, SHORT), &mut runtime, 0);
    }
    frame(&recorded(2, LONG), &mut runtime, 0);

    frame(&recorded(0, LONG), &mut runtime, PLENTY);

    assert_eq!(seq(&runtime), 11111, "the five oldest, all short");
}

/// **Steps of mixed lengths owed survive a save in every encoding**, in order:
/// two short steps then a long one, owed by frames with no fuel, saved from
/// what the lane last wrote and loaded into a fresh runtime, run as they were
/// taken.
#[test]
fn owed_steps_of_mixed_lengths_survive_a_save_in_every_encoding() {
    for (encoding, carry) in super::saves::every_encoding() {
        let mut runtime = runtime_of(MODULE, RECORDER);
        let mut reports = vec![frame(&recorded(0, SHORT), &mut runtime, PLENTY)];
        reports.push(frame(&recorded(2, SHORT), &mut runtime, 0));
        reports.push(frame(&recorded(1, LONG), &mut runtime, 0));
        let saved = reports
            .iter()
            .rev()
            .find_map(|report| {
                report
                    .state
                    .iter()
                    .find(|update| update.entity == entity(0) && update.behavior == "Recorder")
                    .map(|update| update.snapshot.clone())
            })
            .expect("the lane wrote the instance's state");
        let saved = carry(&saved);

        let mut runtime = runtime_of(MODULE, RECORDER);
        let mut arriving = recorded(0, SHORT);
        arriving.instances[0].arrival = Some(loaded(saved));
        frame(&arriving, &mut runtime, PLENTY);

        assert_eq!(seq(&runtime), 112, "{encoding}: short, short, long");
    }
}

// ─── What a frame with steps records ─────────────────────────────────────────

/// A behavior with no `FixedUpdate` and nothing to do each frame.
const IDLE: &str = "behavior Idle { int hp = 40; }";

/// **A quiet turn writes nothing, steps or not.** `Idle` has no
/// `FixedUpdate`: a frame of no step writes no record of it, and neither does
/// a frame of one — its steps are nobody's, and a quiet frame writes nothing
/// to the World.
#[test]
fn a_quiet_turn_writes_no_record_on_a_frame_with_steps() {
    let mut runtime = runtime_of(MODULE, IDLE);
    let at = |steps| {
        let mut scene = view(MODULE, &[("Idle", &[0])], FRAME);
        scene.fixed_steps = steps;
        scene.fixed_delta_seconds = STEP;
        scene
    };
    frame(&at(0), &mut runtime, PLENTY);
    let still = frame(&at(0), &mut runtime, PLENTY);
    assert!(still.state.is_empty(), "set-up: quiet: {:?}", still.state);

    let stepped = frame(&at(1), &mut runtime, PLENTY);

    assert!(
        stepped.state.is_empty(),
        "a quiet turn on a frame of one step writes nothing: {:?}",
        stepped.state
    );
}

/// Counts its steps; `hp` is written by the editor.
const KEEPER: &str = "behavior Keeper {
    int hp = 40;
    int steps = 0;
    void FixedUpdate(float dt) { steps += 1; }
}";

/// **A frame sat out with steps keeps a field written outside.** `Keeper`
/// runs once at 40 hp; the editor then writes 99 to its component, and the
/// next frame — two steps, no fuel — it sits out. What the lane writes back
/// that frame must not put the 40 it still holds over the 99: the turn after,
/// with plenty, it reads 99.
#[test]
fn a_frame_sat_out_with_steps_does_not_write_back_over_a_field_written_outside() {
    use khora_core::script::ScriptValue;
    let row = |hp, steps| {
        vec![
            ("hp".to_owned(), ScriptValue::Int(hp)),
            ("steps".to_owned(), ScriptValue::Int(steps)),
        ]
    };
    let mut runtime = runtime_of(MODULE, KEEPER);
    let mut scene = view(MODULE, &[("Keeper", &[0])], FRAME);
    scene.fixed_delta_seconds = STEP;
    scene.instances[0].row = row(40, 0);
    frame(&scene, &mut runtime, PLENTY);
    assert_eq!(int_of(&runtime, MODULE, 0, "Keeper", "hp"), 40, "set-up");

    // The editor writes 99; the frame has two steps and no fuel.
    scene.instances[0].row = row(99, 0);
    scene.fixed_steps = 2;
    let sat_out = frame(&scene, &mut runtime, 0);

    // What the World holds once that frame's writeback has landed.
    let mut world_row = row(99, 0);
    for update in sat_out
        .state
        .iter()
        .filter(|update| update.entity == entity(0))
    {
        for (name, value) in &update.snapshot.fields {
            if update.changed.contains(name) {
                if let Some((_, held)) = world_row.iter_mut().find(|(known, _)| known == name) {
                    *held = value.clone();
                }
            }
        }
    }
    assert_eq!(
        world_row[0].1,
        ScriptValue::Int(99),
        "the frame it sat out wrote its stale hp over the editor's: {:?}",
        sat_out.state
    );

    scene.instances[0].row = world_row;
    scene.fixed_steps = 0;
    frame(&scene, &mut runtime, PLENTY);
    assert_eq!(
        int_of(&runtime, MODULE, 0, "Keeper", "hp"),
        99,
        "the editor's write is the one the behavior reads"
    );
}
