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

//! **`FixedUpdate(float dt)` — a fixed clock in the turn.** Each turn calls
//! it once per fixed step the frame took — the view's `fixed_steps` — with
//! `dt` the fixed step, after a body resumed from the last turn and the
//! timers, and before the events and `Update`. A frame of no step calls it
//! not at all while `Update` runs, which is what a pause is. A state's own
//! `FixedUpdate` is the one called while the behavior is in it; a system's
//! runs once a step too; a `FixedUpdate` that faults disables the instance
//! like any member; a behavior without one is unaffected.
//!
//! Frames last 1/32 s and the step is 1/64 s, so every sum is exact.

use khora_core::script::{EventQueue, ScriptEvent};
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::ScriptRuntime;

use super::scheduled_scene::{
    entity, float_of, frame, frame_with, int_of, runtime_of, view, PLENTY,
};

const MODULE: &str = "stepped.erg";

/// A frame of 1/32 s.
const FRAME: f32 = 1.0 / 32.0;

/// The fixed step, 1/64 s — two to a frame.
const STEP: f32 = 1.0 / 64.0;

/// `view` taking `steps` fixed steps of [`STEP`].
fn stepped(mut view: ScriptView, steps: u32) -> ScriptView {
    view.fixed_steps = steps;
    view.fixed_delta_seconds = STEP;
    view
}

/// The scene of `behavior` on entity 0, one frame of [`FRAME`] taking
/// `steps` fixed steps.
fn scene(behavior: &str, steps: u32) -> ScriptView {
    stepped(view(MODULE, &[(behavior, &[0])], FRAME), steps)
}

fn int(runtime: &ScriptRuntime, behavior: &str, field: &str) -> i64 {
    int_of(runtime, MODULE, 0, behavior, field)
}

fn mail(name: &str) -> EventQueue {
    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), name));
    mail
}

/// Each member appends its digit to `seq` — a timer 2, `FixedUpdate` 1, an
/// event 3 — and `Update` closes the turn with 4, keeps what the turn did in
/// `seen` and starts the next from nothing.
const CLOCKED: &str = "behavior Clocked {
    int seq = 0;
    int seen = 0;
    int steps = 0;
    int updates = 0;
    float step_dt = 0.0;
    float update_dt = 0.0;
    every 0.01s { seq = seq * 10 + 2; }
    on Hit() { seq = seq * 10 + 3; }
    void FixedUpdate(float dt) { seq = seq * 10 + 1; steps += 1; step_dt = dt; }
    void Update(float dt) { seen = seq * 10 + 4; seq = 0; updates += 1; update_dt = dt; }
}
event Hit;";

/// **The order of a turn**: the timers, then `FixedUpdate` once per step —
/// two this frame, each handed the fixed step — then the events, then
/// `Update`, handed the frame's delta.
#[test]
fn fixed_update_runs_once_per_step_after_the_timers_and_before_events_and_update() {
    let mut runtime = runtime_of(MODULE, CLOCKED);
    frame(&scene("Clocked", 0), &mut runtime, PLENTY);
    assert_eq!(
        int(&runtime, "Clocked", "seen"),
        24,
        "set-up: a frame of no step, the timer then `Update`"
    );

    frame_with(&scene("Clocked", 2), &mail("Hit"), &mut runtime, PLENTY);

    assert_eq!(
        int(&runtime, "Clocked", "seen"),
        21134,
        "timer, two steps, the event, `Update`"
    );
    assert_eq!(int(&runtime, "Clocked", "steps"), 2, "one call a step");
    assert_eq!(
        (
            float_of(&runtime, MODULE, 0, "Clocked", "step_dt"),
            float_of(&runtime, MODULE, 0, "Clocked", "update_dt"),
        ),
        (STEP, FRAME),
        "`FixedUpdate` is handed the fixed step, `Update` the frame's delta"
    );
}

/// `Clocked` with a sequence that waits: `Go` starts it, it waits a hundredth
/// of a second and appends 5.
const RESUMING: &str = "behavior Resuming {
    int seq = 0;
    int seen = 0;
    every 0.01s { seq = seq * 10 + 2; }
    on Go() { Later(); }
    async void Later() { await 0.01s; seq = seq * 10 + 5; }
    void FixedUpdate(float dt) { seq = seq * 10 + 1; }
    void Update(float dt) { seen = seq * 10 + 4; seq = 0; }
}
event Go;";

/// **A body left part-way by the last turn finishes first**: the sequence
/// `Go` started is waiting; the next turn resumes it, then the timer, then
/// the frame's two steps, then `Update`.
#[test]
fn a_body_resumed_from_the_last_turn_runs_before_fixed_update() {
    let mut runtime = runtime_of(MODULE, RESUMING);
    frame(&scene("Resuming", 0), &mut runtime, PLENTY);
    frame_with(&scene("Resuming", 0), &mail("Go"), &mut runtime, PLENTY);
    assert!(
        runtime
            .peek(entity(0), "Resuming")
            .is_some_and(|held| held.pending.is_some()),
        "set-up: the sequence waits"
    );

    frame(&scene("Resuming", 2), &mut runtime, PLENTY);

    assert_eq!(
        int(&runtime, "Resuming", "seen"),
        252114,
        "the timer of the turn `Go` was heard in, then the resumed body, the timer, two steps, `Update`"
    );
}

/// `Counter` counts its steps and its `Update`s; `Plain` declares no
/// `FixedUpdate`.
const COUNTED: &str = "behavior Counter {
    int steps = 0;
    int updates = 0;
    float step_dt = 0.0;
    void FixedUpdate(float dt) { steps += 1; step_dt = dt; }
    void Update(float dt) { updates += 1; }
}
behavior Plain {
    int updates = 0;
    float last = 0.0;
    void Update(float dt) { updates += 1; last = dt; }
}";

/// **As many calls as the frame has steps, in one turn**: frames of 0, 3, 0,
/// 1, 5 and 2 steps call `FixedUpdate` that many times each, and `Update`
/// once each — a frame of no step calls no `FixedUpdate` and still runs
/// `Update`. `Plain`, with no `FixedUpdate`, runs once a frame on the
/// frame's delta whatever the step count.
#[test]
fn fixed_update_is_called_as_many_times_as_the_frame_has_steps() {
    let mut runtime = runtime_of(MODULE, COUNTED);
    let scene = |steps| {
        stepped(
            view(MODULE, &[("Counter", &[0]), ("Plain", &[1])], FRAME),
            steps,
        )
    };

    for (at, steps) in [0u32, 3, 0, 1, 5, 2].into_iter().enumerate() {
        let before = int(&runtime, "Counter", "steps");
        let report = frame(&scene(steps), &mut runtime, PLENTY);
        assert_eq!(report.deferred, 0, "frame {at}: {report:?}");
        assert_eq!(
            int(&runtime, "Counter", "steps") - before,
            i64::from(steps),
            "frame {at}: {steps} steps, as many calls"
        );
        assert_eq!(
            int(&runtime, "Counter", "updates"),
            at as i64 + 1,
            "frame {at}: one `Update` a frame"
        );
        assert_eq!(
            (
                int_of(&runtime, MODULE, 1, "Plain", "updates"),
                float_of(&runtime, MODULE, 1, "Plain", "last")
            ),
            (at as i64 + 1, FRAME),
            "frame {at}: a behavior with no `FixedUpdate` runs once, on the frame's delta"
        );
    }
    assert_eq!(float_of(&runtime, MODULE, 0, "Counter", "step_dt"), STEP);
}

/// **Paused, no step: no `FixedUpdate`, and `Update(0)` runs.** A running
/// frame of one step, three paused frames — no time, no step — then a running
/// frame again: two calls in all, and an `Update` every frame, handed zero
/// while paused.
#[test]
fn a_paused_frame_calls_no_fixed_update_while_update_runs_with_zero() {
    let mut runtime = runtime_of(MODULE, COUNTED);
    let running = stepped(view(MODULE, &[("Counter", &[0])], STEP), 1);
    let paused = stepped(view(MODULE, &[("Counter", &[0])], 0.0), 0);

    frame(&running, &mut runtime, PLENTY);
    assert_eq!(int(&runtime, "Counter", "steps"), 1, "running: one step");
    for at in 0..3 {
        frame(&paused, &mut runtime, PLENTY);
        assert_eq!(
            int(&runtime, "Counter", "steps"),
            1,
            "paused frame {at}: no `FixedUpdate`"
        );
        assert_eq!(
            int(&runtime, "Counter", "updates"),
            at + 2,
            "paused frame {at}: `Update` still runs"
        );
    }
    frame(&running, &mut runtime, PLENTY);
    assert_eq!(
        int(&runtime, "Counter", "steps"),
        2,
        "running again: one step more"
    );
}

/// `Walker` has a `FixedUpdate` of its own and one in `Walking`; `Resting`
/// declares none.
const WALKER: &str = "behavior Walker {
    int walking = 0;
    int plain = 0;
    void FixedUpdate(float dt) { plain += 1; }
    state Walking {
        void FixedUpdate(float dt) { walking += 1; }
    }
    state Resting { }
    on Rest() { become Resting; }
}
event Rest;";

/// **A state's `FixedUpdate` is the one called while in it**, as a state's
/// `Update` is: two frames of two steps in `Walking` call its own; the frame
/// `Rest` arrives, its steps still run in `Walking` — before the event — and
/// once `Resting`, which declares none, the behavior's is called.
#[test]
fn a_states_fixed_update_is_called_while_the_behavior_is_in_it() {
    let mut runtime = runtime_of(MODULE, WALKER);

    frame(&scene("Walker", 2), &mut runtime, PLENTY);
    frame_with(&scene("Walker", 2), &mail("Rest"), &mut runtime, PLENTY);
    frame(&scene("Walker", 3), &mut runtime, PLENTY);

    assert_eq!(
        (
            int(&runtime, "Walker", "walking"),
            int(&runtime, "Walker", "plain")
        ),
        (4, 3),
        "four steps walking — the last two before `Rest` was heard — then three resting"
    );
}

/// A system integrating the fixed step.
const INTEGRATOR: &str = "system Integrator {
    int steps = 0;
    int updates = 0;
    float total = 0.0;
    void FixedUpdate(float dt) { steps += 1; total += dt; }
    void Update(float dt) { updates += 1; }
}";

/// **A system's `FixedUpdate` runs once a step too**: frames of three and two
/// steps, five calls summing five steps, and an `Update` a frame.
#[test]
fn a_systems_fixed_update_runs_once_per_step() {
    let mut runtime = runtime_of(MODULE, INTEGRATOR);

    frame(&scene("Integrator", 3), &mut runtime, PLENTY);
    frame(&scene("Integrator", 2), &mut runtime, PLENTY);

    assert_eq!(int(&runtime, "Integrator", "updates"), 2, "set-up: it ran");
    assert_eq!(int(&runtime, "Integrator", "steps"), 5);
    assert_eq!(
        float_of(&runtime, MODULE, 0, "Integrator", "total"),
        5.0 * STEP
    );
}

/// `Fragile` divides by zero in its `FixedUpdate`.
const FRAGILE: &str = "behavior Fragile {
    int zero = 0;
    int n = 1;
    int updates = 0;
    void FixedUpdate(float dt) { n = n / zero; }
    void Update(float dt) { updates += 1; }
}";

/// **A `FixedUpdate` that faults disables the instance**, like any member:
/// the frame counts the fault, the `Update` after it never runs, and nothing
/// runs after.
#[test]
fn a_faulting_fixed_update_disables_the_instance() {
    let mut runtime = runtime_of(MODULE, FRAGILE);
    frame(&scene("Fragile", 0), &mut runtime, PLENTY);
    assert_eq!(
        int(&runtime, "Fragile", "updates"),
        1,
        "set-up: no step, no fault"
    );

    let report = frame(&scene("Fragile", 1), &mut runtime, PLENTY);

    assert_eq!(report.faulted, 1, "the step faulted: {report:?}");
    assert!(
        runtime
            .peek(entity(0), "Fragile")
            .is_some_and(|held| held.disabled),
        "and disabled the instance"
    );
    for _ in 0..3 {
        frame(&scene("Fragile", 1), &mut runtime, PLENTY);
    }
    assert_eq!(
        int(&runtime, "Fragile", "updates"),
        1,
        "no `Update` after the fault, that frame or any other"
    );
}
