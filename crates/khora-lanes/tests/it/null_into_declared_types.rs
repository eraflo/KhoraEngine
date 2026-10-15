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

//! `null` lands only where the declared type has room for it.
//!
//! A field holding `null` now crosses a save as itself. The other side of that
//! is that every road a value takes into a slot — an engine-raised event, a
//! hot reload, a load into an edited script, a state entered — has to keep a
//! `null` out of an `int`, which no `int` declaration allows. And an arrival
//! has to treat a `null` its row holds as the value it is — an author's, or a
//! game's — not as a field left unset.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const MODULE: &str = "null_into_declared_types.erg";

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

/// The guard's view for one frame, arriving with `fields` on its row, and
/// restored from what an earlier session `observed` of it, if either.
fn view(
    delta: f32,
    fields: Vec<(String, ScriptValue)>,
    observed: Option<ScriptSnapshot>,
) -> ScriptView {
    // A companion holds none of the behavior's own fields: they are its row's.
    let observed = observed.map(|observed| ScriptSnapshot {
        fields: Vec::new(),
        ..observed
    });
    let arrival =
        (!fields.is_empty() || observed.is_some()).then_some(ScriptArrival { fields, observed });
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
            entity: subject(),
            program: 0,
            arrival,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
            row: Vec::new(),
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

/// `name` raised on the guard, carrying `args`.
fn raised(name: &str, args: &[ScriptValue]) -> EventQueue {
    let event = args
        .iter()
        .cloned()
        .fold(ScriptEvent::new(subject(), name), ScriptEvent::with);
    let mut queue = EventQueue::new();
    queue.push(event);
    queue
}

/// What the guard's store holds in the behavior-level field `name`.
fn field_value(runtime: &ScriptRuntime, name: &str) -> Option<Persisted> {
    let slot = runtime
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .and_then(|layout| layout.slot_of(name))
        .unwrap_or_else(|| panic!("`{name}` is a field of the guard"));
    runtime.peek(subject(), "Guard")?.fields.get(slot).cloned()
}

fn null() -> Option<Persisted> {
    Some(Persisted::Scalar(Value::Null))
}

fn int(value: i64) -> Option<Persisted> {
    Some(Persisted::Scalar(Value::Int(value)))
}

fn best(value: ScriptValue) -> Vec<(String, ScriptValue)> {
    vec![("best".to_owned(), value)]
}

// ─── An engine-raised event ─────────────────────────────────────────────────

/// **An event payload never carries `null`.** A script's `Raise` refuses one;
/// an event the *engine* raises — a `ScriptEvent` built in Rust, which can now
/// hold `ScriptValue::Null` — must be refused at delivery the same way a list
/// is, not handed to `int amount` as a value no `int` can be. Delivered, it
/// puts `null` into `int last`, and the next save records an `int` field as
/// `null`.
#[test]
fn an_engine_raised_null_never_reaches_an_int_parameter() {
    const SOURCE: &str = r#"behavior Guard {
                                int last = 0;
                                on Hurt(int amount) { last = amount; }
                            }
                            event Hurt(int amount);"#;
    let mut runtime = runtime_of(SOURCE);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Hurt", &[ScriptValue::Null]),
    );

    assert_ne!(
        field_value(&runtime, "last"),
        null(),
        "`int last` holds null: the event handed `null` to `int amount` (saved: {saved:?})"
    );
}

// ─── An edit that takes the `?` away ────────────────────────────────────────

const OPTIONAL_BEST: &str = r#"behavior Guard {
                                   int? best = 4;
                                   on Clear() { best = null; }
                               }
                               event Clear;"#;

const REQUIRED_BEST: &str = r#"behavior Guard {
                                   int best = 4;
                                   on Bump() { best += 1; }
                               }
                               event Bump;"#;

/// **A hot reload carries `best` by name — but an `int` cannot hold `null`.**
/// `int? best` cleared, then edited to `int best = 4`: the carried `null` must
/// not land in a slot the new declaration says is never null. Carried, the
/// next `best += 1` faults on a value the checker would never have let the
/// author write.
#[test]
fn a_null_does_not_cross_a_reload_into_a_field_no_longer_optional() {
    let mut runtime = runtime_of(OPTIONAL_BEST);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Clear", &[]),
    );
    assert_eq!(field_value(&runtime, "best"), null(), "the premise");

    runtime.reload(MODULE, build(REQUIRED_BEST));
    frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &EventQueue::new(),
    );

    assert_ne!(
        field_value(&runtime, "best"),
        null(),
        "`int best` holds the null carried from `int? best`"
    );
}

/// An edit that keeps `best` optional carries its `null` by name, like any
/// value — not back to its default `4`.
#[test]
fn a_null_field_crosses_a_reload_that_keeps_it_optional() {
    let mut runtime = runtime_of(OPTIONAL_BEST);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Clear", &[]),
    );
    assert_eq!(field_value(&runtime, "best"), null(), "the premise");

    runtime.reload(
        MODULE,
        build(
            r#"behavior Guard {
                   int added = 2;
                   int? best = 4;
                   on Clear() { best = null; }
               }
               event Clear;"#,
        ),
    );
    frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &EventQueue::new(),
    );

    assert_eq!(field_value(&runtime, "best"), null(), "still cleared");
    assert_eq!(field_value(&runtime, "added"), int(2), "the new field");
}

/// The same edit between a save and its load: `int? best` saved as `null`,
/// loaded into a script that now declares `int best = 4`.
#[test]
fn a_saved_null_does_not_load_into_a_field_no_longer_optional() {
    let mut runtime = runtime_of(OPTIONAL_BEST);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Clear", &[]),
    )
    .expect("the guard did work, so the lane recorded it");
    assert_eq!(saved.field("best"), Some(&ScriptValue::Null), "the premise");

    let mut revived = runtime_of(REQUIRED_BEST);
    let mut host = Host::new();
    frame(
        &mut revived,
        &mut host,
        &view(0.0, saved.fields.clone(), Some(saved)),
        &EventQueue::new(),
    );

    assert_ne!(
        field_value(&revived, "best"),
        null(),
        "`int best` loaded as null from the save of `int? best`"
    );
}

// ─── A state entered ────────────────────────────────────────────────────────

/// **A declared-null datum is written on entry, like any other default.**
/// Every state shares the data slots: `Rest`'s `int? z;` sits where `Watch`'s
/// `seen` was. Entering `Rest` has to give `z` its `null`, or `z` reads — and
/// a save records — the `4` `Watch` left behind.
#[test]
fn a_declared_null_state_datum_is_not_what_the_previous_state_left() {
    const SOURCE: &str = r#"behavior Guard {
                                int health = 100;
                                state Watch {
                                    int seen = 4;
                                    on Go() { become Rest; }
                                }
                                state Rest {
                                    int? z;
                                }
                            }
                            event Go;"#;
    let mut runtime = runtime_of(SOURCE);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, Vec::new(), None),
        &raised("Go", &[]),
    )
    .expect("the guard did work, so the lane recorded it");

    assert_eq!(saved.state.as_deref(), Some("Rest"), "the premise");
    assert_eq!(
        saved.state_fields,
        vec![("z".to_owned(), ScriptValue::Null)],
        "`z` is its declared null, not `Watch`'s leftover"
    );
}

// ─── The row's values ───────────────────────────────────────────────────────

/// **The game's `null` is a value like any other.** `best` arrived authored as
/// `7` and the game cleared it: the save's row holds `null`, and a load gives
/// back `null` in every encoding — not the `7` it arrived with, nor the
/// declared `4`.
#[test]
fn a_field_the_game_cleared_loads_as_null() {
    let mut runtime = runtime_of(OPTIONAL_BEST);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, best(ScriptValue::Int(7)), None),
        &raised("Clear", &[]),
    )
    .expect("the guard did work, so the lane recorded it");
    assert_eq!(saved.field("best"), Some(&ScriptValue::Null), "the premise");

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        let mut revived = runtime_of(OPTIONAL_BEST);
        let mut host = Host::new();
        frame(
            &mut revived,
            &mut host,
            &view(0.0, carried.fields.clone(), Some(carried)),
            &EventQueue::new(),
        );
        assert_eq!(
            field_value(&revived, "best"),
            null(),
            "through {encoding}: the game's `null` stands"
        );
    }
}

/// **A restored instance takes what its row holds, `null` included.** Loaded
/// from the same save, `best` is whatever the row arriving with it says — a
/// `null`, a `6` — and, the row holding no `best`, its declared `4`.
#[test]
fn a_restored_instance_takes_its_rows_value_a_null_included() {
    let mut runtime = runtime_of(OPTIONAL_BEST);
    let mut host = Host::new();
    let saved = frame(
        &mut runtime,
        &mut host,
        &view(0.016, best(ScriptValue::Null), None),
        &EventQueue::new(),
    )
    .expect("the guard did work, so the lane recorded it");
    assert_eq!(
        field_value(&runtime, "best"),
        null(),
        "an authored null reached it"
    );

    for (encoding, carry) in every_encoding() {
        let cases = [
            (best(ScriptValue::Null), null(), "the row holds null"),
            (best(ScriptValue::Int(6)), int(6), "the row holds 6"),
            (Vec::new(), int(4), "the row holds no `best`"),
        ];
        for (row, expected, case) in cases {
            let mut revived = runtime_of(OPTIONAL_BEST);
            let mut host = Host::new();
            frame(
                &mut revived,
                &mut host,
                &view(0.0, row, Some(carry(&saved))),
                &EventQueue::new(),
            );
            assert_eq!(
                field_value(&revived, "best"),
                expected,
                "through {encoding}, {case}"
            );
        }
    }
}
