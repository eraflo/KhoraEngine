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

//! Closing the game and opening it again.
//!
//! A behavior is more than its fields, and saving only those was the gap: a
//! guard reloaded had forgotten which state it was in, how far its countdowns
//! had run, and that it was half-way through an attack. Each of those is a
//! separate promise, so each has its own test here.
//!
//! The shape of every one is the same — run some frames, take the snapshot the
//! lane wrote, build a **fresh** runtime from it, and check the guard is who it
//! was. A fresh runtime is the point: sharing one would prove nothing about what
//! actually travels.
//!
//! The guard below is the one from the design — a state with its own data and
//! its own schedule — because that is the shape a real behavior has, and a
//! persistence test against a simpler one proves less than it appears to.

use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::{compile, entity, runtime_of, MODULE};
use crate::script_lane::{run_behaviors, Resumed, ScriptRuntime};
use khora_script::vm::ResumeTier;

/// The guard from the design: two states, each with its own data, and a
/// schedule that belongs to the patrol rather than to the guard.
const GUARD: &str = r#"
behavior Guard {
    int health = 100;

    state Patrol {
        int laps = 0;

        every 0.5s {
            laps += 1;
        }
    }

    state Chase {
        int missed = 7;
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// One guard, restored from `saved` if there is anything to restore.
fn view(delta: f32, saved: Option<ScriptSnapshot>) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            // What the lane wrote back arrives as observed state.
            arrival: saved.map(|observed| ScriptArrival {
                fields: Vec::new(),
                observed: Some(observed),
            }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

/// Runs one frame and returns what the lane recorded for the scene.
fn frame(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    delta: f32,
    saved: Option<ScriptSnapshot>,
    events: &EventQueue,
) -> Option<ScriptSnapshot> {
    let report = run_behaviors(&view(delta, saved), events, runtime, host, u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

fn quiet(runtime: &mut ScriptRuntime, host: &mut Host, delta: f32) -> Option<ScriptSnapshot> {
    frame(runtime, host, delta, None, &EventQueue::new())
}

fn slot(runtime: &ScriptRuntime, index: usize) -> Option<i64> {
    match runtime.peek(entity(0), "Guard")?.fields.get(index)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

/// Loads a snapshot into a runtime that has never seen this entity.
fn reload(source: &str, saved: ScriptSnapshot) -> (ScriptRuntime, Host) {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        0.0,
        Some(saved),
        &EventQueue::new(),
    );
    (runtime, host)
}

// ─── Which state it was in ──────────────────────────────────────────────────

/// **The gap this closes.** A guard saved while chasing reloaded as a guard that
/// had forgotten it ever saw anything.
#[test]
fn the_state_it_was_in_survives() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();

    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    let saved = frame(&mut runtime, &mut host, 0.016, None, &spotted).expect("it did work");

    assert_eq!(saved.state.as_deref(), Some("Chase"));

    let (revived, _) = reload(GUARD, saved);
    let layout = revived
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared")
        .clone();
    assert_eq!(
        slot(&revived, layout.state_slot()),
        Some(1),
        "still chasing, at whatever discriminant `Chase` now has"
    );
}

/// And the state's **own** data, which is the thing `state` exists to give: a
/// `Chase` that lost its `missed` counter is not the chase that was saved.
#[test]
fn the_states_own_data_survives() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();

    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    let saved = frame(&mut runtime, &mut host, 0.016, None, &spotted).expect("it did work");

    assert_eq!(
        saved.state_fields,
        vec![("missed".to_owned(), ScriptValue::Int(7))]
    );
}

/// A state the reloaded script no longer declares is dropped rather than
/// resolved to whichever state now sits at that index — that would put a guard
/// into a state nobody put it in.
#[test]
fn a_state_the_script_dropped_does_not_become_a_different_one() {
    let saved = ScriptSnapshot {
        state: Some("Chase".to_owned()),
        state_fields: vec![("missed".to_owned(), ScriptValue::Int(7))],
        ..ScriptSnapshot::default()
    };

    let (revived, _) = reload(
        "behavior Guard {
             int health = 100;
             state Patrol { int steps = 0; }
         }",
        saved,
    );

    let layout = revived
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared")
        .clone();
    assert_eq!(
        slot(&revived, layout.state_slot()),
        Some(0),
        "it starts over in the first state, not in `Patrol` holding a chase's data"
    );
}

// ─── Countdowns ─────────────────────────────────────────────────────────────

/// **Why a countdown and not a deadline.** What is left survives a save exactly;
/// an absolute time would resume either instantly or after the whole gap,
/// depending only on how long the game was closed.
#[test]
fn a_countdown_keeps_what_is_left_of_it() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();

    quiet(&mut runtime, &mut host, 0.0);
    let saved = quiet(&mut runtime, &mut host, 0.2).expect("the timer ticked");

    let timer = saved.timers.first().expect("one countdown: {saved:?}");
    assert!(
        timer.remaining.expect("still counting") < 0.5,
        "0.2s of a 0.5s interval has gone: {timer:?}"
    );
    assert!(timer.repeating, "`every`, not `after`");
}

/// **A spent `after` is spent.** Recording it as zero would make loading the
/// save fire it a second time, which for `after 10s => Despawn(this)` is an
/// entity that dies twice.
#[test]
fn a_countdown_that_fired_does_not_fire_again_on_load() {
    let source = r#"
    behavior Guard {
        int health = 100;
        int died = 0;

        after 0.1s {
            died += 1;
        }
    }
    "#;
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    quiet(&mut runtime, &mut host, 0.0);
    let saved = quiet(&mut runtime, &mut host, 0.2).expect("it fired");
    assert_eq!(saved.field("died"), Some(&ScriptValue::Int(1)));
    assert_eq!(
        saved.timers.first().and_then(|t| t.remaining),
        None,
        "spent, not due in zero seconds"
    );

    let (mut revived, mut host) = reload(source, saved);
    quiet(&mut revived, &mut host, 0.2);

    assert_eq!(slot(&revived, 1), Some(1), "once, across the save");
}

// ─── Mid-`await` ────────────────────────────────────────────────────────────

const WAITER: &str = r#"
behavior Guard {
    int health = 100;
    int fired = 0;

    async void Attack() {
        await 1.0s;
        fired += 1;
    }

    on Spotted(int by) {
        Attack();
    }
}
"#;

/// Starts an attack and stops in the middle of its wind-up.
fn caught_mid_attack() -> ScriptSnapshot {
    let mut runtime = runtime_of(WAITER);
    let mut host = Host::new();

    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    frame(&mut runtime, &mut host, 0.016, None, &spotted).expect("it did work")
}

/// **The promise the whole suspension design was built to keep.** A save taken
/// half-way through an attack loads half-way through the attack.
#[test]
fn a_sequence_stopped_at_an_await_survives_a_save() {
    let saved = caught_mid_attack();
    assert!(saved.pending.is_some(), "the machine was written down");
    assert_eq!(saved.field("fired"), Some(&ScriptValue::Int(0)));

    let (mut revived, mut host) = reload(WAITER, saved);
    // The wait had a second left; a frame short of it changes nothing, and the
    // frame that reaches it runs the rest of the body.
    quiet(&mut revived, &mut host, 0.5);
    assert_eq!(slot(&revived, 1), Some(0), "still waiting");

    quiet(&mut revived, &mut host, 0.6);
    assert_eq!(
        slot(&revived, 1),
        Some(1),
        "and then it fired, exactly once"
    );
}

/// **Refused, not trusted.** A machine holds a position in code. An edit that
/// moves code around that position is followed by name; one that removes the
/// member the body started in leaves nothing to resume into — resuming anyway
/// would run whatever now sits there, chosen by an edit nobody connected to it.
#[test]
fn a_sequence_is_abandoned_when_its_handler_is_gone() {
    let saved = caught_mid_attack();
    assert!(saved.pending.is_some());

    // The same behavior with the handler that started the attack gone.
    let edited = r#"
    behavior Guard {
        int health = 100;
        int fired = 0;

        async void Attack() {
            await 1.0s;
            fired += 1;
        }

        on Seen(int by) {
            Attack();
        }
    }
    "#;

    let (mut revived, mut host) = reload(edited, saved);
    quiet(&mut revived, &mut host, 2.0);

    assert_eq!(
        slot(&revived, 1),
        Some(0),
        "the attack was dropped rather than resumed into different code"
    );
}

/// An edit that only retunes a value outside the attack still resumes it —
/// throwing it away would punish an author for changing a number. The program
/// is a different one now, so the load reports the attack as resumed
/// `Unchanged`: every function on its stack is the same code.
#[test]
fn a_sequence_survives_an_edit_that_only_changes_a_literal() {
    let saved = caught_mid_attack();

    let retuned = WAITER.replace("int health = 100;", "int health = 120;");
    assert_ne!(
        compile(&retuned).fingerprint(),
        compile(WAITER).fingerprint(),
        "a literal is code: the program changed"
    );

    let mut revived = runtime_of(&retuned);
    let mut host = Host::new();
    let loaded = run_behaviors(
        &view(0.0, Some(saved)),
        &EventQueue::new(),
        &mut revived,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        loaded.resumes,
        vec![Resumed {
            behavior: "Guard".to_owned(),
            member: "Spotted".to_owned(),
            tier: Ok(ResumeTier::Unchanged),
        }],
        "the load reports how the attack came back"
    );
    quiet(&mut revived, &mut host, 1.2);

    assert_eq!(slot(&revived, 1), Some(1), "it carried on");
}

// ─── Loading is not spawning ────────────────────────────────────────────────

/// `OnSpawn` announced the entity in the run that was saved. Loading must not
/// announce it again — a chest that drops its loot on `OnSpawn` would refill
/// itself every time the player reloaded.
#[test]
fn loading_a_save_does_not_run_on_spawn_again() {
    let source = r#"
    behavior Guard {
        int health = 100;
        int spawns = 0;

        void OnSpawn() {
            spawns += 1;
        }
    }
    "#;
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    let saved = quiet(&mut runtime, &mut host, 0.016).expect("it did work");
    assert_eq!(saved.field("spawns"), Some(&ScriptValue::Int(1)));
    // What makes the load a load and not a fresh start: the save records
    // that the instance had spawned.
    assert!(
        saved.lifecycle.spawned,
        "the save records the spawn: {saved:?}"
    );

    let (revived, _) = reload(source, saved);
    assert_eq!(slot(&revived, 1), Some(1), "still one spawn");
}

/// The guard after an edit that gave `Chase` a new piece of data, above the
/// one it had, and gave `Patrol` a default that is not zero.
const CHASE_GAINS_DATA: &str = r#"
behavior Guard {
    int health = 100;

    state Patrol {
        int laps = 5;

        every 0.5s {
            laps += 1;
        }
    }

    state Chase {
        int spotted = 4;
        int missed = 7;
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// **Data an edit added to the state a save caught starts at its own
/// default** — not at the first state's, which the initialiser entered on
/// the way and which shares those slots.
#[test]
fn data_added_to_the_saved_state_since_the_save_starts_at_its_declared_default() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    let saved = frame(&mut runtime, &mut host, 0.016, None, &spotted).expect("it did work");
    assert_eq!(saved.state.as_deref(), Some("Chase"));

    let (mut revived, mut host) = reload(CHASE_GAINS_DATA, saved);
    quiet(&mut revived, &mut host, 0.0);
    let layout = revived
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared")
        .clone();
    let held = crate::script_lane::persistence::snapshot_from_store(
        &layout,
        &revived.peek(entity(0), "Guard").expect("live").fields,
    );

    assert_eq!(
        held.state_fields,
        vec![
            ("spotted".to_owned(), ScriptValue::Int(4)),
            ("missed".to_owned(), ScriptValue::Int(7)),
        ],
        "the new datum takes its declared default; the saved one its value"
    );
}

/// A chase with a schedule of its own, and data the first state shares slots
/// with.
const CHASE_WITH_SCHEDULE: &str = r#"
behavior Guard {
    int health = 100;
    int pings = 0;

    state Patrol {
        int laps = 5;
        int rounds = 6;
    }

    state Chase {
        int spotted = 4;
        int missed = 7;

        every 1.0s {
            pings += 1;
        }
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// **Entering the saved state, whatever the slice.** A guard saved chasing,
/// with its chase's countdown part-spent, is loaded under a budget that cuts
/// the initialiser — or the entry into the chase after it — at every possible
/// point. It ends exactly where an uncut load does.
#[test]
fn entering_the_saved_state_ends_the_same_under_every_fuel_slice() {
    // Saved before the edit that gave the chase `spotted`.
    let before = CHASE_WITH_SCHEDULE.replace("        int spotted = 4;\n", "");
    assert_ne!(
        before, CHASE_WITH_SCHEDULE,
        "the premise: the edit is undone"
    );
    let mut runtime = runtime_of(&before);
    let mut host = Host::new();
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    frame(&mut runtime, &mut host, 0.0, None, &spotted);
    let saved =
        frame(&mut runtime, &mut host, 0.4, None, &EventQueue::new()).expect("the countdown moved");
    assert_eq!(saved.state.as_deref(), Some("Chase"));

    let load = |fuel: u64| {
        let mut runtime = runtime_of(CHASE_WITH_SCHEDULE);
        let mut host = Host::new();
        let mut observed = Some(saved.clone());
        for _ in 0..200 {
            let view = view(0.0, observed.take());
            run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, fuel);
            if runtime
                .peek(entity(0), "Guard")
                .is_some_and(|instance| instance.initialised)
            {
                break;
            }
        }
        let layout = runtime
            .program(MODULE)
            .and_then(|program| program.layout("Guard"))
            .expect("declared")
            .clone();
        let instance = runtime.peek(entity(0), "Guard").expect("live");
        assert!(
            instance.initialised,
            "fuel {fuel}: the initialiser finished"
        );
        crate::script_lane::persistence::snapshot_from_store(&layout, &instance.fields)
    };

    let whole = load(u64::MAX);
    assert_eq!(
        whole.state_fields,
        vec![
            ("spotted".to_owned(), ScriptValue::Int(4)),
            ("missed".to_owned(), ScriptValue::Int(7)),
        ],
        "the premise: an uncut load enters the chase's own defaults"
    );
    assert_eq!(
        whole
            .timers
            .iter()
            .find(|timer| timer.state.as_deref() == Some("Chase"))
            .and_then(|timer| timer.remaining)
            .map(|left| (left * 1000.0).round() / 1000.0),
        Some(0.6),
        "and the chase's countdown keeps what was left of it over the entry's re-arm"
    );
    for fuel in 1..=60 {
        assert_eq!(load(fuel), whole, "fuel {fuel} per frame");
    }
}
