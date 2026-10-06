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

//! A field holding `null` is a value a save keeps, not a hole it skips.
//!
//! `int? best = 4; best = null;` read back as "nothing written" drops `best`
//! from the save, and the load's initialiser gives it `4` again — a game that
//! cleared a record finds it restored. Each test runs a behavior, takes the
//! snapshot the lane wrote, carries it through every scene encoding, and loads
//! it into a **fresh** runtime.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const MODULE: &str = "null_fields.erg";

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

/// The guard's view for one frame, arriving with `fields` authored and
/// `observed` from an earlier session, if either.
fn view(
    delta: f32,
    fields: Vec<(String, ScriptValue)>,
    observed: Option<ScriptSnapshot>,
) -> ScriptView {
    let arrival =
        (!fields.is_empty() || observed.is_some()).then_some(ScriptArrival { fields, observed });
    ScriptView {
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            arrival,
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
    view: &ScriptView,
    events: &EventQueue,
) -> Option<ScriptSnapshot> {
    let report = run_behaviors(view, events, runtime, host, u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

/// `name` raised on the guard, with no payload.
fn raised(name: &str) -> EventQueue {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(subject(), name));
    queue
}

/// Loads `saved` into a runtime that has never seen the guard.
fn reload(source: &str, saved: ScriptSnapshot) -> (ScriptRuntime, Host) {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        &view(0.0, Vec::new(), Some(saved)),
        &EventQueue::new(),
    );
    (runtime, host)
}

/// What the guard's store holds in `slot`.
fn slot_value(runtime: &ScriptRuntime, slot: usize) -> Option<Persisted> {
    runtime.peek(subject(), "Guard")?.fields.get(slot).cloned()
}

/// The slot of the behavior-level field `name`.
fn field_slot(runtime: &ScriptRuntime, name: &str) -> usize {
    runtime
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .and_then(|layout| layout.slot_of(name))
        .unwrap_or_else(|| panic!("`{name}` is a field of the guard"))
}

/// The slot of `name`, a datum of `state`.
fn state_datum_slot(runtime: &ScriptRuntime, state: &str, name: &str) -> usize {
    let layout = runtime
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .expect("the guard's layout");
    let index = layout.state_index(state).expect("the state is declared");
    let offset = layout
        .state_at(index)
        .and_then(|state| state.slots.iter().position(|slot| slot == name))
        .unwrap_or_else(|| panic!("`{name}` is a datum of `{state}`"));
    layout.state_data_slot() + offset
}

fn sorted(mut pairs: Vec<(String, ScriptValue)>) -> Vec<(String, ScriptValue)> {
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

// ─── A field ────────────────────────────────────────────────────────────────

const CLEARS_A_FIELD: &str = r#"behavior Guard {
                                    int? best = 4;
                                    on Clear() { best = null; }
                                }"#;

/// **The gap.** `best` set to `null` is saved as `null` and loads as `null`,
/// whatever the encoding — not as the `4` its declaration gives it.
#[test]
fn a_null_field_survives_a_save() {
    let mut runtime = runtime_of(CLEARS_A_FIELD);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Clear"),
    )
    .expect("the guard did work, so the lane recorded it");
    assert_eq!(
        saved.field("best"),
        Some(&ScriptValue::Null),
        "the save records the `null`, rather than leaving the field out: {saved:?}"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.field("best"),
            Some(&ScriptValue::Null),
            "through {encoding}: the `null` travels"
        );

        let (revived, _) = reload(CLEARS_A_FIELD, carried);
        let slot = field_slot(&revived, "best");
        assert_eq!(
            slot_value(&revived, slot),
            Some(Persisted::Scalar(Value::Null)),
            "through {encoding}: `best` loads as null, not as its declared 4"
        );
    }
}

// ─── A state's own data ─────────────────────────────────────────────────────

const CLEARS_A_STATE_DATUM: &str = r#"behavior Guard {
                                          int health = 100;
                                          state Watch {
                                              int? seen = 4;
                                              on Forget() { seen = null; }
                                          }
                                      }"#;

/// The same for a state's own data: `seen` cleared inside `Watch` loads as
/// `null`, not as the `4` entering the state gives it.
#[test]
fn a_null_state_datum_survives_a_save() {
    let mut runtime = runtime_of(CLEARS_A_STATE_DATUM);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Forget"),
    )
    .expect("the guard did work, so the lane recorded it");
    assert_eq!(saved.state.as_deref(), Some("Watch"));
    assert_eq!(
        saved.state_fields,
        vec![("seen".to_owned(), ScriptValue::Null)],
        "the save records the state's `null` datum"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.state_fields, saved.state_fields,
            "through {encoding}: the state's data travels"
        );

        let (revived, _) = reload(CLEARS_A_STATE_DATUM, carried);
        let slot = state_datum_slot(&revived, "Watch", "seen");
        assert_eq!(
            slot_value(&revived, slot),
            Some(Persisted::Scalar(Value::Null)),
            "through {encoding}: `seen` loads as null, not as its declared 4"
        );
    }
}

// ─── The authored base ──────────────────────────────────────────────────────

const DECLARES_NULL_DEFAULTS: &str = r#"behavior Guard {
                                            int speed = 1;
                                            int? best;
                                            int? worst = null;
                                        }"#;

/// **A declared-null default is a default.** The base a snapshot records holds
/// `best: null` for `int? best;` (and for an explicit `= null`) beside the
/// other fields' defaults — and keeps it through every encoding — so a load
/// can tell a field the game left alone from one it changed.
#[test]
fn a_null_default_is_part_of_the_authored_base() {
    let mut runtime = runtime_of(DECLARES_NULL_DEFAULTS);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.0, vec![("speed".to_owned(), ScriptValue::Int(3))], None),
        &EventQueue::new(),
    )
    .expect("the guard did work, so the lane recorded it");

    assert_eq!(
        sorted(saved.authored.clone()),
        vec![
            ("best".to_owned(), ScriptValue::Null),
            ("speed".to_owned(), ScriptValue::Int(3)),
            ("worst".to_owned(), ScriptValue::Null),
        ],
        "the base holds the speed override and both declared-null defaults"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.authored, saved.authored,
            "through {encoding}: the base travels"
        );
    }
}

// ─── Beside a countdown ─────────────────────────────────────────────────────

const CLEARS_WHEN_IT_FIRES: &str = r#"behavior Guard {
                                          int? best = 4;
                                          int fired = 0;
                                          after 0.1s { fired += 1; best = null; }
                                      }"#;

/// A spent `after` is held as `null` too, in its own slot — and is still read
/// as spent, not as a field: beside a `null` field it neither fires again on
/// the load nor shows up among the fields.
#[test]
fn a_spent_after_stays_spent_beside_a_null_field() {
    let mut runtime = runtime_of(CLEARS_WHEN_IT_FIRES);
    let mut host = Host::new();
    let quiet = |delta| view(delta, Vec::new(), None);
    frame(&mut runtime, &mut host, &quiet(0.0), &EventQueue::new());
    let saved = frame(&mut runtime, &mut host, &quiet(0.2), &EventQueue::new()).expect("it fired");

    assert_eq!(saved.timers.len(), 1, "one countdown: {saved:?}");
    assert_eq!(saved.timers[0].remaining, None, "spent, not due in zero");
    assert_eq!(
        sorted(saved.fields.clone()),
        vec![
            ("best".to_owned(), ScriptValue::Null),
            ("fired".to_owned(), ScriptValue::Int(1)),
        ],
        "the fields are the two declared, the cleared one as null"
    );

    for (encoding, carry) in every_encoding() {
        let (mut revived, mut host) = reload(CLEARS_WHEN_IT_FIRES, carry(&saved));
        frame(&mut revived, &mut host, &quiet(0.2), &EventQueue::new());

        let fired = field_slot(&revived, "fired");
        let best = field_slot(&revived, "best");
        assert_eq!(
            slot_value(&revived, fired),
            Some(Persisted::Scalar(Value::Int(1))),
            "through {encoding}: fired once, across the save"
        );
        assert_eq!(
            slot_value(&revived, best),
            Some(Persisted::Scalar(Value::Null)),
            "through {encoding}: `best` stays null"
        );
    }
}
