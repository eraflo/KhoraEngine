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

//! A hot reload carries what a behavior *is*, not only its fields.
//!
//! An author edits a guard while it is chasing someone, with a countdown half
//! run. The guard after the edit is still chasing, with the chase's own data,
//! and the countdown has what was left of it — carried by name (the state's
//! name, the state's slot names, the schedule's identity), the way a save is.
//! A state or a schedule the edit removed is dropped; one it added starts as
//! the new script says.

use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_script::native::Host;

use super::{compile, entity, runtime_of, view_of, MODULE};
use crate::script_lane::persistence::snapshot_from_store;
use crate::script_lane::{run_behaviors, ScriptRuntime};

/// A guard with a behavior-level countdown and two states, the second with
/// data an event changes.
const GUARD: &str = r#"
behavior Guard {
    int health = 100;
    int ticks = 0;

    every 2.0s {
        ticks += 1;
    }

    state Patrol {
        int laps = 0;
    }

    state Chase {
        int missed = 0;

        on Lost(int by) {
            missed += by;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// The guard after an edit that touches neither state nor the countdown: a
/// field inserted at the top, a default retuned.
const RETUNED: &str = r#"
behavior Guard {
    int rage = 0;
    int health = 120;
    int ticks = 0;

    every 2.0s {
        ticks += 1;
    }

    state Patrol {
        int laps = 0;
    }

    state Chase {
        int missed = 0;

        on Lost(int by) {
            missed += by;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// The guard after an edit that removed `Chase` and added `Search` in its
/// place — the state that now sits at `Chase`'s index.
const CHASE_REMOVED: &str = r#"
behavior Guard {
    int health = 100;
    int ticks = 0;

    every 2.0s {
        ticks += 1;
    }

    state Patrol {
        int laps = 0;
    }

    state Search {
        int clues = 0;
    }

    on Spotted(int by) {
        become Search;
    }
}
"#;

/// The guard after an edit that inserted an `every 1.0s` above the existing
/// `every 2.0s`.
const TIMER_ADDED: &str = r#"
behavior Guard {
    int health = 100;
    int ticks = 0;
    int pulses = 0;

    every 1.0s {
        pulses += 1;
    }

    every 2.0s {
        ticks += 1;
    }

    state Patrol {
        int laps = 0;
    }

    state Chase {
        int missed = 0;

        on Lost(int by) {
            missed += by;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

fn frame(runtime: &mut ScriptRuntime, host: &mut Host, delta: f32, events: &EventQueue) {
    let mut view = view_of(1);
    view.delta_seconds = delta;
    run_behaviors(&view, events, runtime, host, u64::MAX);
}

fn told(name: &str, by: i64) -> EventQueue {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(entity(0), name).with(ScriptValue::Int(by)));
    queue
}

/// The live instance, read by name against the program loaded now.
fn held(runtime: &ScriptRuntime) -> ScriptSnapshot {
    let layout = runtime
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared");
    let instance = runtime.peek(entity(0), "Guard").expect("the guard is live");
    snapshot_from_store(layout, &instance.fields)
}

/// A guard chasing with `missed = 3`, after its first frames.
fn chasing() -> (ScriptRuntime, Host) {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    frame(&mut runtime, &mut host, 0.0, &told("Spotted", 1));
    frame(&mut runtime, &mut host, 0.0, &told("Lost", 3));
    let before = held(&runtime);
    assert_eq!(
        before.state.as_deref(),
        Some("Chase"),
        "the premise: chasing"
    );
    assert_eq!(
        before.state_fields,
        vec![("missed".to_owned(), ScriptValue::Int(3))],
        "the premise: the chase's own data moved off its default"
    );
    (runtime, host)
}

/// A guard whose 2s countdown has 0.5s left.
fn half_run_countdown() -> (ScriptRuntime, Host) {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());
    frame(&mut runtime, &mut host, 1.5, &EventQueue::new());
    let before = held(&runtime);
    assert_eq!(
        remaining(&before, 2.0),
        Some(0.5),
        "the premise: 0.5s left: {before:?}"
    );
    (runtime, host)
}

/// What is left of the countdown declared with `interval`, rounded to the
/// millisecond so an exact float is not what is compared.
fn remaining(snapshot: &ScriptSnapshot, interval: f32) -> Option<f32> {
    snapshot
        .timers
        .iter()
        .find(|timer| timer.interval == interval)
        .and_then(|timer| timer.remaining)
        .map(|seconds| (seconds * 1000.0).round() / 1000.0)
}

/// **In `Chase`, still in `Chase`.** An edit that does not touch the state
/// leaves the guard where it was, with the chase's own data — not back in
/// the first state with every default.
#[test]
fn a_reload_keeps_the_current_state() {
    let (mut runtime, mut host) = chasing();

    runtime.reload(MODULE, compile(RETUNED));
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());

    let after = held(&runtime);
    assert_eq!(
        after.state.as_deref(),
        Some("Chase"),
        "still chasing: {after:?}"
    );
    assert_eq!(
        after.state_fields,
        vec![("missed".to_owned(), ScriptValue::Int(3))],
        "with the chase's own data"
    );
    assert_eq!(
        after.field("health"),
        Some(&ScriptValue::Int(100)),
        "and the field kept its value over the retuned default, as before"
    );
}

/// **What is left of a countdown, not a fresh one.** A 2s schedule with 0.5s
/// left still has 0.5s after the edit, and fires on the frame that spends it.
#[test]
fn a_reload_keeps_what_is_left_of_a_countdown() {
    let (mut runtime, mut host) = half_run_countdown();

    runtime.reload(MODULE, compile(RETUNED));
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());

    let after = held(&runtime);
    assert_eq!(
        remaining(&after, 2.0),
        Some(0.5),
        "the countdown kept what was left of it: {after:?}"
    );

    frame(&mut runtime, &mut host, 0.6, &EventQueue::new());
    assert_eq!(
        held(&runtime).field("ticks"),
        Some(&ScriptValue::Int(1)),
        "and fired once the half second left was spent"
    );
}

/// **A state the edit removed is dropped, never re-resolved by position.**
/// The guard starts over in the first state with that state's defaults — not
/// in `Search`, which took `Chase`'s index, holding a chase's data — while
/// what the edit kept (a field, the countdown) still crosses by name.
#[test]
fn a_reload_drops_a_state_the_edit_removed() {
    let (mut runtime, mut host) = chasing();
    frame(&mut runtime, &mut host, 1.5, &EventQueue::new());

    runtime.reload(MODULE, compile(CHASE_REMOVED));
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());

    let after = held(&runtime);
    assert_eq!(
        after.state.as_deref(),
        Some("Patrol"),
        "back in the first state, not in whatever sits at `Chase`'s index: {after:?}"
    );
    assert_eq!(
        after.state_fields,
        vec![("laps".to_owned(), ScriptValue::Int(0))],
        "with the first state's own defaults"
    );
    assert_eq!(
        remaining(&after, 2.0),
        Some(0.5),
        "while the countdown the edit kept still crossed it"
    );
}

/// **A schedule the edit added starts full**, and inserting it above an
/// existing one does not hand it that one's countdown: the old one keeps
/// what was left of it, matched by what its author wrote.
#[test]
fn a_timer_added_by_the_edit_starts_full() {
    let (mut runtime, mut host) = half_run_countdown();

    runtime.reload(MODULE, compile(TIMER_ADDED));
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());

    let after = held(&runtime);
    assert_eq!(
        (remaining(&after, 1.0), remaining(&after, 2.0)),
        (Some(1.0), Some(0.5)),
        "the new `every 1.0s` is full; the old `every 2.0s` kept its 0.5s: {after:?}"
    );
}

/// A guard whose first state's data sits in the slots a chase's data uses.
const PATROLLING: &str = r#"
behavior Guard {
    state Patrol {
        int laps = 7;
    }

    state Chase {
        int missed = 0;

        on Lost(int by) {
            missed += by;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// The guard after an edit that gave `Chase` a new piece of data, above the
/// one it had.
const CHASE_GAINS_DATA: &str = r#"
behavior Guard {
    state Patrol {
        int laps = 7;
    }

    state Chase {
        int spotted = 4;
        int missed = 0;

        on Lost(int by) {
            missed += by;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// **Data the edit added to the current state starts at its own default.**
/// The guard stays in `Chase` across the edit and keeps `missed` by name; the
/// `spotted` the edit declared in `Chase` is new, so it starts at the `4` its
/// author wrote — not at whatever the first state's entry left in that slot.
#[test]
fn data_the_edit_added_to_the_current_state_starts_at_its_declared_default() {
    let mut runtime = runtime_of(PATROLLING);
    let mut host = Host::new();
    frame(&mut runtime, &mut host, 0.0, &told("Spotted", 1));
    frame(&mut runtime, &mut host, 0.0, &told("Lost", 3));
    assert_eq!(held(&runtime).state.as_deref(), Some("Chase"));

    runtime.reload(MODULE, compile(CHASE_GAINS_DATA));
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());

    let after = held(&runtime);
    assert_eq!(after.state.as_deref(), Some("Chase"), "{after:?}");
    assert_eq!(
        after.state_fields,
        vec![
            ("spotted".to_owned(), ScriptValue::Int(4)),
            ("missed".to_owned(), ScriptValue::Int(3)),
        ],
        "the new datum takes its declared default; the kept one its value"
    );
}

/// A guard with a one-shot schedule.
const ONE_SHOT: &str = r#"
behavior Guard {
    int fired = 0;

    after 1.0s {
        fired += 1;
    }
}
"#;

/// The same, after an edit that adds a field and touches nothing else.
const ONE_SHOT_RETUNED: &str = r#"
behavior Guard {
    int rage = 0;
    int fired = 0;

    after 1.0s {
        fired += 1;
    }
}
"#;

/// **A spent `after` stays spent.** It fired before the edit; the edit
/// re-runs the initialiser, which arms every schedule — but the carried
/// countdown says "spent", so it does not fire a second time.
#[test]
fn an_after_spent_before_a_reload_does_not_fire_again() {
    let mut runtime = runtime_of(ONE_SHOT);
    let mut host = Host::new();
    frame(&mut runtime, &mut host, 0.0, &EventQueue::new());
    frame(&mut runtime, &mut host, 1.5, &EventQueue::new());
    assert_eq!(held(&runtime).field("fired"), Some(&ScriptValue::Int(1)));

    runtime.reload(MODULE, compile(ONE_SHOT_RETUNED));
    for _ in 0..3 {
        frame(&mut runtime, &mut host, 1.0, &EventQueue::new());
    }

    assert_eq!(
        held(&runtime).field("fired"),
        Some(&ScriptValue::Int(1)),
        "the one-shot fired once, before the edit"
    );
}

/// A guard whose schedule's body waits part-way.
const SLOW_TICK: &str = r#"
behavior Guard {
    int ticks = 0;

    every 2.0s {
        Slow();
    }

    async void Slow() {
        await 1.0s;
        ticks += 1;
    }

    state Patrol {
        int laps = 0;
    }

    state Chase {
        int missed = 0;
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// **A reload that changes nothing changes nothing.** The same frames, with
/// and without an identical recompile in the middle — while chasing, with a
/// schedule's body waiting part-way — leave the guard identical.
#[test]
fn a_reload_that_changes_nothing_leaves_the_instance_as_it_was() {
    let run = |reload: bool| {
        let mut runtime = runtime_of(SLOW_TICK);
        let mut host = Host::new();
        frame(&mut runtime, &mut host, 0.0, &told("Spotted", 1));
        frame(&mut runtime, &mut host, 2.0, &EventQueue::new());
        assert!(
            runtime
                .peek(entity(0), "Guard")
                .is_some_and(|instance| instance.pending.is_some()),
            "the premise: the schedule's body is waiting"
        );
        if reload {
            runtime.reload(MODULE, compile(SLOW_TICK));
        }
        for _ in 0..7 {
            frame(&mut runtime, &mut host, 0.5, &EventQueue::new());
        }
        held(&runtime)
    };

    assert_eq!(run(true), run(false));
}
