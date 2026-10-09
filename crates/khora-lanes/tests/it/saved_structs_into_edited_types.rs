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

//! A saved struct loaded into a script that edited the struct since.
//!
//! A behavior field's saved value is carried by name and dropped when its
//! declared type can no longer hold it (`bridge::fits`): the checker never
//! lets an author put text in an `int`, so a load must not either. A struct's
//! own fields are carried the same way — by name — so the same holds one level
//! down: a saved field whose type changed is not loaded as the old kind, and a
//! field the save lacks is filled whole, as a literal leaving it out would be.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::bridge::from_persisted;
use khora_script::vm::Program;
use khora_script::{check, compile, lex, parse};

const MODULE: &str = "saved_structs_into_edited_types.erg";

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

fn view_of(observed: Option<ScriptSnapshot>) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: 0.016,
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

/// A runtime running `source`, which loaded `saved` in its first frame and
/// ran one more.
fn loaded(source: &str, saved: ScriptSnapshot) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    for observed in [Some(saved), None] {
        let lane = BudgetedScriptLane::new();
        let mut deck = OutputDeck::new();
        let view = view_of(observed);
        let mut ctx = LaneContext::new();
        ctx.insert_ref(&view);
        ctx.insert(Fuel(u64::MAX));
        ctx.insert_slot(&mut runtime);
        ctx.insert_slot(&mut deck);
        lane.execute(&mut ctx).expect("the lane runs");
    }
    runtime
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

/// **A saved struct field whose type changed is not loaded as the old kind.**
/// The save holds `loot.value` as the `int` it was; the script now declares
/// it `string`. Loaded into the `string` field, the `int` would be read as
/// text by every line that trusts the checker — so it must not arrive: the
/// field takes its default (or the whole struct does not fit and `loot`
/// takes its own), and the guard reading `loot.value.Length` does not fault.
#[test]
fn a_saved_struct_field_whose_type_changed_is_not_loaded_as_the_old_kind() {
    let saved = ScriptSnapshot::default().with_field(
        "loot",
        fields(&[
            ("value", ScriptValue::Int(9)),
            ("weight", ScriptValue::Int(7)),
        ]),
    );
    let edited = r#"struct Loot { string value = "gold"; int weight = 5; }
                    behavior Guard {
                        Loot loot = Loot { };
                        int length = -1;
                        void Update(float dt) { length = loot.value.Length; }
                    }"#;

    let runtime = loaded(edited, saved);
    let value = match field(&runtime, "loot") {
        Some(ScriptValue::Struct(named)) => named
            .into_iter()
            .find(|(name, _)| name == "value")
            .map(|(_, value)| value),
        other => panic!("`loot` holds a struct: {other:?}"),
    };
    assert!(
        matches!(value, Some(ScriptValue::Str(_))),
        "`loot.value` is declared `string` and holds {value:?}"
    );
    assert_eq!(
        fault_of(&runtime),
        None,
        "reading it as text does not fault"
    );
    assert_eq!(field(&runtime, "length"), Some(ScriptValue::Int(4)));
}

/// **A struct field the save lacks is filled whole.** `Loot` gained `tag`,
/// declared with a default that writes only some of `Tag`'s fields. A literal
/// leaving `tag` out builds every `Tag` field — the written one and the one
/// at `Tag`'s own default — and a save lacking `tag` must load as that same
/// struct, not as a `Tag` holding only the fields its default happened to
/// spell.
#[test]
fn a_struct_field_the_save_lacks_is_filled_whole() {
    let saved =
        ScriptSnapshot::default().with_field("loot", fields(&[("value", ScriptValue::Int(9))]));
    let edited = r#"struct Tag { int id = 4; int rank; }
                    struct Loot { int value; Tag tag = Tag { rank: 2 }; }
                    behavior Guard { Loot loot = Loot { value: 1 }; }"#;

    let runtime = loaded(edited, saved);
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(
        field(&runtime, "loot"),
        Some(fields(&[
            ("value", ScriptValue::Int(9)),
            (
                "tag",
                fields(&[("id", ScriptValue::Int(4)), ("rank", ScriptValue::Int(2))])
            ),
        ])),
        "`tag` holds every field of `Tag`, as `Loot {{ value: 9 }}` would build it"
    );
}

/// **A saved scalar does not load into a field now declared a struct.** The
/// save holds `loot` as the `int` it was declared; the script now declares it
/// `Loot`. An `int` is told apart from a struct by its kind alone, so it does
/// not fit: `loot` takes its declared default, and reading `loot.value` does
/// not fault.
#[test]
fn a_saved_scalar_does_not_load_into_a_field_now_declared_a_struct() {
    let saved = ScriptSnapshot::default().with_field("loot", ScriptValue::Int(5));
    let edited = r#"struct Loot { int value; }
                    behavior Guard {
                        Loot loot = Loot { value: 1 };
                        int seen = -1;
                        void Update(float dt) { seen = loot.value; }
                    }"#;

    let runtime = loaded(edited, saved);
    assert_eq!(
        field(&runtime, "loot"),
        Some(fields(&[("value", ScriptValue::Int(1))])),
        "the saved `int` does not fit `Loot`: the field's own default"
    );
    assert_eq!(
        fault_of(&runtime),
        None,
        "reading `loot.value` does not fault"
    );
    assert_eq!(field(&runtime, "seen"), Some(ScriptValue::Int(1)));
}
