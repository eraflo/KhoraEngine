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

//! A struct in a behavior field, or held by a suspended body, is part of the
//! game save — through the lane, frame after frame, and through every scene
//! encoding.
//!
//! A field is saved by name as a `ScriptValue::Struct` of named fields, with no
//! type name: the field's declared type supplies it when the value goes back
//! into the store. Its fields are matched **by name** against the struct as
//! the code now declares it — a survivor keeps its value, a field the struct
//! no longer declares is dropped, a new one takes its declared default or its
//! type's zero; a struct missing a field that has neither does not fit, and
//! the behavior field takes its own default.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{EventQueue, FrozenValue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::bridge::from_persisted;
use khora_script::vm::Program;
use khora_script::{check, compile, ergon_fn, lex, parse};

use super::saves::every_encoding;

/// Text no literal of the scripts below spells.
#[ergon_fn(name = "StructFieldLaneTail")]
fn struct_field_lane_tail() -> String {
    "bcd".to_owned()
}

const MODULE: &str = "struct_fields.erg";

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

fn view_of(delta: f32, observed: Option<ScriptSnapshot>) -> ScriptView {
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
            arrival: observed.map(|observed| ScriptArrival {
                fields: Vec::new(),
                observed: Some(observed),
            }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

/// One frame through the lane itself, the way the scripting agent runs it.
fn lane_frame(view: &ScriptView, runtime: &mut ScriptRuntime) -> ScriptRunReport {
    let lane = BudgetedScriptLane::new();
    let mut deck = OutputDeck::new();
    {
        let mut ctx = LaneContext::new();
        ctx.insert_ref(view);
        ctx.insert(Fuel(u64::MAX));
        ctx.insert_slot(&mut *runtime);
        ctx.insert_slot(&mut deck);
        lane.execute(&mut ctx).expect("the lane runs");
    }
    runtime.last_report().clone()
}

/// Frames of `delta`, the first carrying `observed`.
fn frames(runtime: &mut ScriptRuntime, observed: Option<ScriptSnapshot>, deltas: &[f32]) {
    let mut observed = observed;
    for delta in deltas {
        lane_frame(&view_of(*delta, observed.take()), runtime);
    }
}

/// One frame delivering `event` (no payload) to the guard; what the lane
/// recorded of it.
fn deliver(runtime: &mut ScriptRuntime, event: &str) -> ScriptSnapshot {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), event));
    runtime.set_pending(events);
    let report = lane_frame(&view_of(0.016, None), runtime);
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it")
}

/// What the guard's field `name` holds, as the boundary reads it.
fn field(runtime: &ScriptRuntime, name: &str) -> Option<ScriptValue> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(name)?;
    let stored = runtime.peek(subject(), "Guard")?.fields.get(slot)?;
    from_persisted(stored).ok().flatten()
}

fn fault_of(runtime: &ScriptRuntime) -> Option<String> {
    runtime
        .peek(subject(), "Guard")?
        .fault
        .as_ref()
        .map(|fault| fault.reason.clone())
}

/// A struct as the boundary holds it: named fields, no type name.
fn fields(named: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        named
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

fn text(value: &str) -> ScriptValue {
    ScriptValue::Str(value.to_owned())
}

const TYPES: &str = r#"struct Loot { int value; int weight = 5; string label = "x"; }
                       struct Pack { Loot[] items; int count; }"#;

const BEHAVIOR: &str = r#"behavior Guard {
                              Loot loot = Loot { value: 1 };
                              Pack pack = Pack { items: [Loot { value: 2 }], count: 1 };
                              int total = 0;
                              on Bump() {
                                  loot.value = 9;
                                  loot.weight = 7;
                                  loot.label = "b" + StructFieldLaneTail();
                                  pack.items[0].value = 4;
                              }
                              on Sum() { total = loot.value * 100 + loot.weight * 10 + loot.label.Length + pack.items[0].value * 1000; }
                          }"#;

fn guard() -> String {
    format!("{TYPES}\n{BEHAVIOR}")
}

/// The guard's `loot` after `Bump`.
fn bumped_loot() -> ScriptValue {
    fields(&[
        ("value", ScriptValue::Int(9)),
        ("weight", ScriptValue::Int(7)),
        ("label", text("bbcd")),
    ])
}

/// The guard's `pack` after `Bump`.
fn bumped_pack() -> ScriptValue {
    fields(&[
        (
            "items",
            ScriptValue::Array(vec![fields(&[
                ("value", ScriptValue::Int(4)),
                ("weight", ScriptValue::Int(5)),
                ("label", text("x")),
            ])]),
        ),
        ("count", ScriptValue::Int(1)),
    ])
}

/// The guard, its fields written by `Bump`, and what the lane recorded.
fn bumped(source: &str) -> (ScriptRuntime, ScriptSnapshot) {
    let mut runtime = runtime_of(source);
    lane_frame(&view_of(0.0, None), &mut runtime);
    let saved = deliver(&mut runtime, "Bump");
    assert_eq!(fault_of(&runtime), None, "the premise: it did not fault");
    (runtime, saved)
}

// ─── Fields across frames ───────────────────────────────────────────────────

/// **A struct field built in one frame reads in the next**, through the lane:
/// its fields by name in declaration order, the text and the array of structs
/// inside it.
#[test]
fn a_struct_field_reads_back_in_the_next_frame() {
    let (mut runtime, _) = bumped(&guard());
    deliver(&mut runtime, "Sum");
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(field(&runtime, "total"), Some(ScriptValue::Int(4974)));
    assert_eq!(field(&runtime, "loot"), Some(bumped_loot()));
    assert_eq!(field(&runtime, "pack"), Some(bumped_pack()));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **A struct field is saved and loaded by value**, whatever the encoding: the
/// lane records it as its named fields, and a fresh runtime loading the save
/// holds that struct — and computes with it.
#[test]
fn a_struct_field_survives_a_save() {
    let (_, saved) = bumped(&guard());
    assert_eq!(
        saved.field("loot"),
        Some(&bumped_loot()),
        "the save records the struct by its fields: {saved:?}"
    );
    assert_eq!(saved.field("pack"), Some(&bumped_pack()));

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.field("loot"),
            saved.field("loot"),
            "through {encoding}: the struct travels"
        );

        let mut revived = runtime_of(&guard());
        lane_frame(&view_of(0.0, Some(carried)), &mut revived);
        assert_eq!(
            field(&revived, "loot"),
            Some(bumped_loot()),
            "through {encoding}: `loot` loads as saved, not as its declared default"
        );
        assert_eq!(field(&revived, "pack"), Some(bumped_pack()));
        deliver(&mut revived, "Sum");
        assert_eq!(fault_of(&revived), None, "through {encoding}: no fault");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(4974)),
            "through {encoding}: the loaded structs compute"
        );
    }
}

/// **Into the store through the declared type.** A save written by hand —
/// the struct's fields in another order, one left out — loads as the struct
/// the field declares: every field there, in declaration order, the one left
/// out at its default. An array of structs the same, element by element.
#[test]
fn a_struct_value_goes_back_through_its_declared_type() {
    let saved = ScriptSnapshot::default()
        .with_field(
            "loot",
            fields(&[("label", text("hand")), ("value", ScriptValue::Int(3))]),
        )
        .with_field(
            "pack",
            fields(&[
                ("count", ScriptValue::Int(2)),
                (
                    "items",
                    ScriptValue::Array(vec![
                        fields(&[("value", ScriptValue::Int(6))]),
                        fields(&[
                            ("weight", ScriptValue::Int(8)),
                            ("value", ScriptValue::Int(7)),
                        ]),
                    ]),
                ),
            ]),
        );

    for (encoding, carry) in every_encoding() {
        let mut runtime = runtime_of(&guard());
        lane_frame(&view_of(0.0, Some(carry(&saved))), &mut runtime);
        assert_eq!(fault_of(&runtime), None, "{encoding}: it did not fault");
        assert_eq!(
            field(&runtime, "loot"),
            Some(fields(&[
                ("value", ScriptValue::Int(3)),
                ("weight", ScriptValue::Int(5)),
                ("label", text("hand")),
            ])),
            "{encoding}: by name, the missing field at its default"
        );
        assert_eq!(
            field(&runtime, "pack"),
            Some(fields(&[
                (
                    "items",
                    ScriptValue::Array(vec![
                        fields(&[
                            ("value", ScriptValue::Int(6)),
                            ("weight", ScriptValue::Int(5)),
                            ("label", text("x")),
                        ]),
                        fields(&[
                            ("value", ScriptValue::Int(7)),
                            ("weight", ScriptValue::Int(8)),
                            ("label", text("x")),
                        ]),
                    ]),
                ),
                ("count", ScriptValue::Int(2)),
            ])),
            "{encoding}: an array of structs, each by name"
        );
    }
}

// ─── Edits ──────────────────────────────────────────────────────────────────

/// `TYPES` with `weight` renamed `mass`, given another default.
const RENAMED: &str = r#"struct Loot { int value; int mass = 3; string label = "x"; }
                         struct Pack { Loot[] items; int count; }"#;

const RENAMED_BEHAVIOR: &str = r#"behavior Guard {
                                      Loot loot = Loot { value: 1 };
                                      Pack pack = Pack { items: [Loot { value: 2 }], count: 1 };
                                      int total = 0;
                                      on Sum() { total = loot.value * 100 + loot.mass * 10 + loot.label.Length + pack.items[0].value * 1000; }
                                  }"#;

/// The guard's `loot` as the renamed struct reads the bumped one.
fn renamed_loot() -> ScriptValue {
    fields(&[
        ("value", ScriptValue::Int(9)),
        ("mass", ScriptValue::Int(3)),
        ("label", text("bbcd")),
    ])
}

/// **A saved struct matches its fields by name.** The script renamed `weight`
/// to `mass` since the save: `value` and `label` keep what the game gave them,
/// `mass` takes its declared default, and the saved `weight` is dropped — as a
/// load of the save into the edited script, through every encoding, and as a
/// hot reload of the running one.
#[test]
fn a_saved_struct_matches_fields_by_name() {
    let edited = format!("{RENAMED}\n{RENAMED_BEHAVIOR}");
    let (_, saved) = bumped(&guard());

    for (encoding, carry) in every_encoding() {
        let mut revived = runtime_of(&edited);
        lane_frame(&view_of(0.0, Some(carry(&saved))), &mut revived);
        assert_eq!(fault_of(&revived), None, "{encoding}: it did not fault");
        assert_eq!(
            field(&revived, "loot"),
            Some(renamed_loot()),
            "{encoding}: the survivors keep their values, the renamed field its default"
        );
        deliver(&mut revived, "Sum");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(4934)),
            "{encoding}: the loaded struct computes"
        );
    }

    let (mut runtime, _) = bumped(&guard());
    runtime.reload(MODULE, build(&edited));
    frames(&mut runtime, None, &[0.0]);
    assert_eq!(fault_of(&runtime), None, "the reload did not fault");
    assert_eq!(
        field(&runtime, "loot"),
        Some(renamed_loot()),
        "a hot reload matches by name too"
    );
}

/// **A field the struct gained since the save** takes its type's zero when it
/// has no default. One whose type has no zero — a struct — cannot be filled:
/// the saved value does not fit, and the behavior field takes its own
/// default.
#[test]
fn a_saved_struct_missing_a_new_field_takes_its_zero_or_does_not_fit() {
    let (_, saved) = bumped(&guard());

    let with_a_zero = r#"struct Loot { int value; int weight = 5; string label = "x"; int bonus; }
                         struct Pack { Loot[] items; int count; }
                         behavior Guard { Loot loot = Loot { value: 1 }; }"#;
    let mut revived = runtime_of(with_a_zero);
    lane_frame(&view_of(0.0, Some(saved.clone())), &mut revived);
    assert_eq!(fault_of(&revived), None, "it did not fault");
    assert_eq!(
        field(&revived, "loot"),
        Some(fields(&[
            ("value", ScriptValue::Int(9)),
            ("weight", ScriptValue::Int(7)),
            ("label", text("bbcd")),
            ("bonus", ScriptValue::Int(0)),
        ])),
        "the new field at its type's zero, the rest as saved"
    );

    let without = r#"struct Tag { int id; }
                     struct Loot { int value; int weight = 5; string label = "x"; Tag tag; }
                     behavior Guard { Loot loot = Loot { value: 1, tag: Tag { id: 2 } }; }"#;
    let mut revived = runtime_of(without);
    lane_frame(&view_of(0.0, Some(saved)), &mut revived);
    assert_eq!(fault_of(&revived), None, "it did not fault");
    assert_eq!(
        field(&revived, "loot"),
        Some(fields(&[
            ("value", ScriptValue::Int(1)),
            ("weight", ScriptValue::Int(5)),
            ("label", text("x")),
            ("tag", fields(&[("id", ScriptValue::Int(2))])),
        ])),
        "the saved struct does not fit: the behavior field's own default"
    );
}

// ─── Suspended bodies ───────────────────────────────────────────────────────

/// A guard whose attack builds a struct, waits, then reads it.
const ATTACKER: &str = r#"struct Pair { int a; int b = 5; }
                          struct Loot { int value; string label = "x"; }
                          behavior Guard {
                              int total = 0;
                              async void Attack() {
                                  Pair p = Pair { a: 3 };
                                  Loot l = Loot { value: 1, label: "a" + StructFieldLaneTail() };
                                  await 1.0s;
                                  Pair noise = Pair { a: 70, b: 70 };
                                  total = p.a * 100 + p.b * 10 + l.label.Length;
                              }
                              on Spotted(int by) { Attack(); }
                          }"#;

/// **A save taken mid-`await` holds the struct.** The frozen register is the
/// struct by name, and whichever encoding carried the save, a fresh runtime
/// loading it finishes the attack with it.
#[test]
fn a_machine_saved_mid_await_keeps_its_structs() {
    let mut runtime = runtime_of(ATTACKER);
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    runtime.set_pending(events);
    let report = lane_frame(&view_of(0.0, None), &mut runtime);
    let snapshot = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the lane recorded the guard");
    let machine = &snapshot
        .pending
        .as_ref()
        .expect("the save caught the attack")
        .machine;
    let pair = FrozenValue::Struct {
        name: "Pair".to_owned(),
        fields: vec![
            ("a".to_owned(), FrozenValue::Int(3)),
            ("b".to_owned(), FrozenValue::Int(5)),
        ],
    };
    assert!(
        machine.registers.contains(&pair),
        "the struct is written by name: {:?}",
        machine.registers
    );

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(&loaded, &snapshot, "{encoding}: the save is carried whole");

        let mut revived = runtime_of(ATTACKER);
        frames(&mut revived, Some(loaded), &[0.0, 0.6, 0.6, 0.6]);
        assert_eq!(fault_of(&revived), None, "{encoding}: it did not fault");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(354)),
            "{encoding}: the attack finished with its structs"
        );
    }
}
