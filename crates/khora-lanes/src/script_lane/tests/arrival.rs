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

//! An instance's first frame: its fields and its observed state arrive apart —
//! the fields from its component's row, the rest (its state, its countdowns,
//! a sequence part-way) from its companion — and the lane puts them together.

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
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        components: Default::default(),
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
            row: Vec::new(),
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

/// **Its fields from its component, its state from its companion.** A
/// restored guard takes every field from the row it arrives with and the state
/// it was in from what was observed of it. Fields an observed snapshot still
/// carries are not the guard's: its component is.
#[test]
fn a_restored_arrival_takes_its_fields_from_its_row_and_its_state_from_its_companion() {
    let observed = ScriptSnapshot {
        state: Some("Chase".to_owned()),
        state_fields: vec![("missed".to_owned(), ScriptValue::Int(2))],
        ..ScriptSnapshot::default()
    }
    .with_field("health", ScriptValue::Int(100));

    let (runtime, _) = first_frame(
        GUARD,
        ScriptArrival {
            fields: vec![("health".to_owned(), ScriptValue::Int(40))],
            observed: Some(observed),
        },
    );

    assert_eq!(
        int_at(&runtime, slot_of(&runtime, "health")),
        Some(40),
        "the row's health, not a field the observed snapshot carried"
    );
    assert_eq!(
        int_at(&runtime, state_slot(&runtime)),
        Some(1),
        "still chasing"
    );
}

/// **A field its row lacks takes its declared default**, restored or not —
/// the script gained it since, or the row never held it.
#[test]
fn a_field_a_restored_row_lacks_takes_its_declared_default() {
    let (runtime, _) = first_frame(
        GUARD,
        ScriptArrival {
            fields: vec![("health".to_owned(), ScriptValue::Int(40))],
            observed: Some(ScriptSnapshot {
                state: Some("Chase".to_owned()),
                ..ScriptSnapshot::default()
            }),
        },
    );

    assert_eq!(int_at(&runtime, slot_of(&runtime, "health")), Some(40));
    assert_eq!(
        int_at(&runtime, slot_of(&runtime, "armour")),
        Some(5),
        "the declared armour"
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

/// A guard that remembers where it first appeared.
const HOMING: &str = r#"
behavior Guard {
    Vec3 home;

    void OnSpawn() {
        home = Position();
    }
}
"#;

/// One frame of the homing guard standing at `x`; what the lane recorded.
fn homing_frame(
    runtime: &mut ScriptRuntime,
    x: f32,
    arrival: Option<ScriptArrival>,
) -> Option<ScriptSnapshot> {
    let mut view = view(0.0, arrival);
    view.instances[0].translation = khora_core::math::Vec3::new(x, 0.0, 0.0);
    let mut host = Host::new();
    let report = run_behaviors(&view, &EventQueue::new(), runtime, &mut host, u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

/// **A starting value read from the world is not read again on a load.** The
/// guard appeared at x = 1 and recorded it as `home` in `OnSpawn`; loaded
/// where the game had since moved it, x = 5, it keeps the `home` its row
/// holds — a loaded instance has spawned already, and its fields are the
/// loaded ones.
#[test]
fn a_starting_value_read_from_the_world_survives_a_save_unchanged() {
    let mut first = runtime_of(HOMING);
    let saved = homing_frame(
        &mut first,
        1.0,
        Some(ScriptArrival {
            fields: Vec::new(),
            observed: None,
        }),
    )
    .expect("the arrival ran `OnSpawn`");
    let home = Some(&ScriptValue::Vec3(khora_core::math::Vec3::new(
        1.0, 0.0, 0.0,
    )));
    assert_eq!(saved.field("home"), home, "the premise: home is x = 1");

    let mut loaded = runtime_of(HOMING);
    homing_frame(&mut loaded, 5.0, Some(super::loaded(saved)));
    let layout = loaded
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .expect("declared")
        .clone();
    let held = loaded.peek(entity(0), "Guard").map(|instance| {
        crate::script_lane::persistence::snapshot_from_store(&layout, &instance.fields)
    });

    assert_eq!(
        held.as_ref().and_then(|snapshot| snapshot.field("home")),
        home,
        "a save and a load leave `home` where the game had it"
    );
}

/// **An instance the lane does not hold, and that brings no arrival, starts
/// from its row.** The flow sends an arrival once, the frame a behavior
/// appears; were the lane to forget the instance later — while the flow still
/// lists it — the instance would otherwise start over from its declared
/// defaults, and its writeback would put them over the values its component
/// holds.
#[test]
fn an_instance_the_lane_forgot_starts_from_its_row_not_its_defaults() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let mut view = view(0.0, None);
    view.instances[0].row = vec![
        ("health".to_owned(), ScriptValue::Int(40)),
        ("armour".to_owned(), ScriptValue::Int(9)),
    ];

    let report = run_behaviors(
        &view,
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1_000_000,
    );

    assert_eq!(int_at(&runtime, slot_of(&runtime, "health")), Some(40));
    assert_eq!(int_at(&runtime, slot_of(&runtime, "armour")), Some(9));
    assert!(
        report.state.iter().all(|update| !update
            .changed
            .iter()
            .any(|field| field == "health" || field == "armour")),
        "nothing it holds is written back as changed: {report:?}"
    );
}
