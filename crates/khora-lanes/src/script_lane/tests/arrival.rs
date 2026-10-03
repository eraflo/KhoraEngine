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

//! An instance's first frame: the authored fields and the observed state
//! arrive apart, and the lane composes them — what was observed over what was
//! authored, and what was never observed as it was authored.

use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::{entity, runtime_of, MODULE};
use crate::script_lane::{run_behaviors, ScriptRuntime};

/// A guard with two fields and two states.
const GUARD: &str = r#"
behavior Guard {
    int health = 100;
    int armour = 5;

    state Patrol {
        int laps = 0;
    }

    state Chase {
        int missed = 7;
    }

    on Spotted(int by) {
        become Chase;
    }
}
"#;

/// A guard whose attack waits a second before it lands.
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

fn view(delta: f32, arrival: Option<ScriptArrival>) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            arrival,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

fn int_at(runtime: &ScriptRuntime, slot: usize) -> Option<i64> {
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

/// The slot the declared field `name` of the guard lives in.
fn slot_of(runtime: &ScriptRuntime, name: &str) -> usize {
    runtime
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared")
        .slot_of(name)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

fn state_slot(runtime: &ScriptRuntime) -> usize {
    runtime
        .program(MODULE)
        .expect("loaded")
        .layout("Guard")
        .expect("declared")
        .state_slot()
}

fn first_frame(source: &str, arrival: ScriptArrival) -> (ScriptRuntime, Host) {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view(0.0, Some(arrival)),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    (runtime, host)
}

/// **Where the guard got to, not where it started.** A field both the author
/// set and the game observed takes the observed value; the state it was in
/// comes from the observed state.
#[test]
fn an_arrivals_observed_values_win_over_authored_ones() {
    let observed = ScriptSnapshot {
        state: Some("Chase".to_owned()),
        state_fields: vec![("missed".to_owned(), ScriptValue::Int(2))],
        ..ScriptSnapshot::default()
    }
    .with_field("health", ScriptValue::Int(40));

    let (runtime, _) = first_frame(
        GUARD,
        ScriptArrival {
            fields: vec![("health".to_owned(), ScriptValue::Int(100))],
            observed: Some(observed),
        },
    );

    assert_eq!(
        int_at(&runtime, slot_of(&runtime, "health")),
        Some(40),
        "the observed health, not the authored one"
    );
    assert_eq!(
        int_at(&runtime, state_slot(&runtime)),
        Some(1),
        "still chasing"
    );
}

/// **A designer's edit is not lost to a save that never saw the field.** A
/// field the observed state lacks — the script gained it, or the save never
/// recorded it — takes the value its author set, not the declared default.
#[test]
fn a_field_the_observed_state_lacks_takes_the_authored_value() {
    let (runtime, _) = first_frame(
        GUARD,
        ScriptArrival {
            fields: vec![
                ("health".to_owned(), ScriptValue::Int(100)),
                ("armour".to_owned(), ScriptValue::Int(9)),
            ],
            observed: Some(ScriptSnapshot::default().with_field("health", ScriptValue::Int(40))),
        },
    );

    assert_eq!(int_at(&runtime, slot_of(&runtime, "health")), Some(40));
    assert_eq!(
        int_at(&runtime, slot_of(&runtime, "armour")),
        Some(9),
        "the authored armour, not the declared 5"
    );
}

/// With nothing observed, the instance starts from what was authored, and
/// what was not authored takes its declared default.
#[test]
fn an_arrival_with_nothing_observed_starts_from_the_authored_fields() {
    let (runtime, _) = first_frame(
        GUARD,
        ScriptArrival {
            fields: vec![("armour".to_owned(), ScriptValue::Int(9))],
            observed: None,
        },
    );

    assert_eq!(int_at(&runtime, slot_of(&runtime, "armour")), Some(9));
    assert_eq!(
        int_at(&runtime, slot_of(&runtime, "health")),
        Some(100),
        "the declared default"
    );
}

/// Runs the waiter until its attack is part-way through its wind-up, and
/// returns what the lane wrote back.
fn caught_mid_attack() -> ScriptSnapshot {
    let mut runtime = runtime_of(WAITER);
    let mut host = Host::new();
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    let report = run_behaviors(
        &view(0.016, None),
        &spotted,
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work")
}

/// **A guard saved mid-attack loads mid-attack.** A pending sequence in the
/// observed state resumes where it stopped, beside authored fields, and
/// finishes exactly once.
#[test]
fn an_observed_pending_sequence_resumes() {
    let saved = caught_mid_attack();
    assert!(saved.pending.is_some(), "the machine was written down");

    let (mut runtime, mut host) = first_frame(
        WAITER,
        ScriptArrival {
            fields: vec![("health".to_owned(), ScriptValue::Int(80))],
            observed: Some(saved),
        },
    );
    let fired = slot_of(&runtime, "fired");
    assert_eq!(int_at(&runtime, fired), Some(0), "still waiting");

    run_behaviors(
        &view(0.5, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        int_at(&runtime, fired),
        Some(0),
        "half a second is not enough"
    );

    run_behaviors(
        &view(0.6, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        int_at(&runtime, fired),
        Some(1),
        "and then it fired, exactly once"
    );
}
