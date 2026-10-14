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

//! A behavior is a component, and another script writes and reads it as one:
//! `other.Set(Guard { … })`, `other.Get(Guard)`, `other.Has(Guard)`.
//!
//! What the language promises before anything reaches a `World`: the command
//! a `Set` of a behavior's component queues — a patch of the fields written,
//! a number converted as a component field converts it — what a `Get` and a
//! `Has` read, and what the checker refuses: a behavior writing its own
//! component with `Set` (its fields are assigned), a field it does not have,
//! a value its field does not take. Its state companion stays unnameable
//! (`behavior_components.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue, WorldCommand,
};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

const OTHER: EntityId = EntityId {
    index: 9,
    generation: 1,
};

/// A guard whose `health` is a `float`, declared in its own module.
const GUARD: &str = "behavior Guard {
    float health = 10;
    int armor = 2;
}";

/// `main.erg` compiled beside `others`.
fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut loader = MemoryLoader::new().with("main.erg", main);
    for &(path, source) in others {
        loader = loader.with(path, source);
    }
    compile_module(&loader, "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build_with(main: &str, others: &[(&str, &str)]) -> Program {
    let result = compile_with(main, others);
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn build(main: &str) -> Program {
    build_with(main, &[])
}

/// The errors of `main`; panics when it compiles.
fn refusals(main: &str) -> Vec<Diagnostic> {
    let result = compile_with(main, &[]);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    result
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// The refusal of `main` saying `fragment`.
fn refused_saying(main: &str, fragment: &str) -> Diagnostic {
    let found = refusals(main);
    found
        .iter()
        .find(|d| said(d).contains(fragment))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragment:?}; got {:?}",
                found.iter().map(said).collect::<Vec<_>>()
            )
        })
}

/// What `diagnostic` underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    &source[diagnostic.span.start as usize..diagnostic.span.end as usize]
}

/// What `function` queued, run whole as the subject's.
fn queued(program: &Program, function: &str, args: &[Value]) -> Vec<WorldCommand> {
    let mut host = Host::new().for_entity(SUBJECT);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    host.commands.as_slice().to_vec()
}

fn patch(fields: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// A column of rows already converted.
struct Rows(Vec<ScriptValue>);

impl ColumnSnapshot for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn read(&self, row: usize) -> ScriptValue {
        self.0[row].clone()
    }
}

/// What the flow would project of `Guard`: `OTHER` runs it, with `health`
/// 4.5 and `armor` 7; `SUBJECT` does not.
fn projection() -> ComponentProjection {
    let row = patch(&[
        ("health", ScriptValue::Float(4.5)),
        ("armor", ScriptValue::Int(7)),
    ]);
    let index: HashMap<EntityId, (u32, u32)> = [(OTHER, (0, 0))].into_iter().collect();
    ComponentProjection::new(vec![ProjectedComponent::new(
        ComponentName::new("Guard"),
        vec![Box::new(Rows(vec![row]))],
        index,
        Some(patch(&[
            ("health", ScriptValue::Float(10.0)),
            ("armor", ScriptValue::Int(2)),
        ])),
    )])
}

/// Runs `function` whole over the projection, returning its result.
fn run(program: &Program, function: &str, args: &[Value]) -> Value {
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection()));
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    let outcome = machine.run(program, &mut host, u64::MAX);
    assert_eq!(outcome, Run::Completed, "`{function}` completes");
    machine.result()
}

// ─── Writing another behavior ───────────────────────────────────────────────

/// **A behavior's component is written like any component.** `Set` queues a
/// patch of the fields it names, under the behavior's name — and an `int`
/// written to a `float` field is sent as the `float` the field holds.
#[test]
fn a_set_of_a_behavior_component_queues_its_patch_with_ints_widened_to_floats() {
    let program = build(
        "behavior Guard { float health = 10; int armor = 2; }
         fn void Heal(Entity e) { e.Set(Guard { health: 3 }); }",
    );

    assert_eq!(
        queued(&program, "Heal", &[Value::Entity(OTHER)]),
        vec![WorldCommand::SetComponent {
            entity: OTHER,
            component: ComponentName::new("Guard"),
            value: patch(&[("health", ScriptValue::Float(3.0))]),
        }]
    );
}

/// Another behavior writes it too, through an entity it holds — and from
/// another module, where the behavior is imported.
#[test]
fn another_behavior_sets_a_behavior_component_through_an_entity() {
    let program = build(
        "behavior Guard { float health = 10; int armor = 2; }
         behavior Medic {
             void Treat(Entity patient) { patient.Set(Guard { armor: 5, health: 1.5 }); }
         }",
    );
    assert_eq!(
        queued(&program, "Medic.Treat", &[Value::Entity(OTHER)]),
        vec![WorldCommand::SetComponent {
            entity: OTHER,
            component: ComponentName::new("Guard"),
            value: patch(&[
                ("armor", ScriptValue::Int(5)),
                ("health", ScriptValue::Float(1.5)),
            ]),
        }]
    );

    let imported = build_with(
        "import \"ai/guard.erg\";
         fn void Heal(Entity e) { e.Set(Guard { health: 3 }); }",
        &[("ai/guard.erg", GUARD)],
    );
    assert_eq!(
        queued(&imported, "Heal", &[Value::Entity(OTHER)]),
        vec![WorldCommand::SetComponent {
            entity: OTHER,
            component: ComponentName::new("Guard"),
            value: patch(&[("health", ScriptValue::Float(3.0))]),
        }]
    );
}

/// A behavior sets *another* behavior on its own entity: only its own
/// component is written by assigning fields.
#[test]
fn a_behavior_sets_another_behavior_on_its_own_entity() {
    let program = build(
        "behavior Guard { float health = 10; int armor = 2; }
         behavior Medic {
             void Patch() { this.Set(Guard { armor: 9 }); }
         }",
    );

    assert_eq!(
        queued(&program, "Medic.Patch", &[]),
        vec![WorldCommand::SetComponent {
            entity: SUBJECT,
            component: ComponentName::new("Guard"),
            value: patch(&[("armor", ScriptValue::Int(9))]),
        }]
    );
}

// ─── Reading another behavior ───────────────────────────────────────────────

/// **`Get` reads a copy of the behavior's component, `Has` says whether the
/// entity runs it** — `null` and `false` for one that does not.
#[test]
fn get_and_has_read_a_behavior_component() {
    let program = build(
        "behavior Guard { float health = 10; int armor = 2; }
         fn float Health(Entity e) { return e.Get(Guard)?.health ?? -1.0; }
         fn int Armor(Entity e) {
             if (var g = e.Get(Guard)) { return g.armor; }
             return -1;
         }
         fn bool Guarded(Entity e) { return e.Has(Guard); }",
    );

    assert_eq!(
        run(&program, "Health", &[Value::Entity(OTHER)]),
        Value::Float(4.5)
    );
    assert_eq!(
        run(&program, "Armor", &[Value::Entity(OTHER)]),
        Value::Int(7)
    );
    assert_eq!(
        run(&program, "Guarded", &[Value::Entity(OTHER)]),
        Value::Bool(true)
    );
    assert_eq!(
        run(&program, "Health", &[Value::Entity(SUBJECT)]),
        Value::Float(-1.0),
        "an entity not running it reads null"
    );
    assert_eq!(
        run(&program, "Guarded", &[Value::Entity(SUBJECT)]),
        Value::Bool(false)
    );
}

/// What a program reads of a behavior's component is in its read set, so the
/// flow projects it; a behavior only written is not.
#[test]
fn a_program_reading_a_behavior_component_lists_it_in_its_reads() {
    let reading = build(
        "behavior Guard { float health = 10; }
         behavior Medic { float seen = 0; }
         fn bool Guarded(Entity e) { return e.Has(Guard); }
         fn void Mark(Entity e) { e.Set(Medic { seen: 1 }); }",
    );

    assert!(
        reading.reads.iter().any(|read| read == "Guard"),
        "`Guard` is read: {:?}",
        reading.reads
    );
    assert!(
        !reading.reads.iter().any(|read| read == "Medic"),
        "`Medic` is only written: {:?}",
        reading.reads
    );
}

/// A `Set` then a `Get` of a behavior's component in one run reads the write,
/// as it does for any component: the field written, the others as projected.
#[test]
fn a_get_after_a_set_of_a_behavior_component_reads_the_write() {
    let program = build(
        "behavior Guard { float health = 10; int armor = 2; }
         fn float Written(Entity e) {
             e.Set(Guard { health: 8 });
             return e.Get(Guard)?.health ?? -1.0;
         }
         fn int Untouched(Entity e) {
             e.Set(Guard { health: 8 });
             return e.Get(Guard)?.armor ?? -1;
         }",
    );

    assert_eq!(
        run(&program, "Written", &[Value::Entity(OTHER)]),
        Value::Float(8.0)
    );
    assert_eq!(
        run(&program, "Untouched", &[Value::Entity(OTHER)]),
        Value::Int(7)
    );
}

// ─── What the checker refuses ───────────────────────────────────────────────

/// **A behavior's own fields are assigned, not `Set`.** Writing its own
/// component through `this.Set` is refused at the component's name, with a
/// note saying to assign the field.
#[test]
fn a_behavior_setting_its_own_component_is_refused_pointing_to_field_assignment() {
    let main = "behavior Guard {
        float health = 10;
        void Hurt() { this.Set(Guard { health: 3 }); }
    }";
    let found = refusals(main);
    let refusal = found
        .iter()
        .find(|d| said(d).contains("Guard"))
        .unwrap_or_else(|| {
            panic!(
                "a refusal naming `Guard`; got {:?}",
                found.iter().map(said).collect::<Vec<_>>()
            )
        });
    let note = refusal.note.as_deref().unwrap_or_default();
    assert!(
        note.contains("assign"),
        "the note says to assign the field; got {:?}",
        said(refusal)
    );
    assert!(
        !said(refusal).contains("not a component a script writes"),
        "a behavior's component is one a script writes: {:?}",
        said(refusal)
    );
    let span = underlined(main, refusal);
    assert!(
        span.contains("Guard"),
        "the refusal underlines the write of `Guard`; got {span:?}"
    );
}

/// A field a behavior does not have is refused at the field, as a
/// component's is.
#[test]
fn a_set_of_a_field_a_behavior_does_not_have_is_refused() {
    refused_saying(
        "behavior Guard { float health = 10; }
         fn void F(Entity e) { e.Set(Guard { mana: 3 }); }",
        "`Guard` has no field `mana`",
    );
}

/// A value its field does not take is refused, as a component field refuses
/// it — whatever the behavior's name.
#[test]
fn a_set_of_a_value_a_behavior_field_does_not_take_is_refused() {
    let found = refusals(
        "behavior Guard { float health = 10; }
         fn void F(Entity e) { e.Set(Guard { health: true }); }",
    );
    assert!(
        found
            .iter()
            .all(|d| !said(d).contains("not a component a script writes")),
        "refused for the value, not the component: {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
    assert!(
        found.iter().any(|d| said(d).contains("bool")),
        "the refusal names the value it was given: {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
}
