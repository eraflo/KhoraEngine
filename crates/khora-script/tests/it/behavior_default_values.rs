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

//! What a fresh instance, and a state just entered, hold — read back the way
//! a game reads it, by running the behavior's code against the store its
//! layout builds — and what entering a state costs.

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptValue, WorldCommand};
use khora_script::arena::Persisted;
use khora_script::dispatch::resolve_member;
use khora_script::vm::Value;
use khora_script::{compile_module, Diagnostic, Host, Machine, MemoryLoader, Program, Run};

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

fn outcome(main: &str) -> (Option<Program>, Vec<Diagnostic>) {
    let result = compile_module(&MemoryLoader::new().with("main.erg", main), "main.erg");
    let errors = result
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect();
    (result.program, errors)
}

fn build(main: &str) -> Program {
    let (program, errors) = outcome(main);
    assert!(
        errors.is_empty(),
        "should compile:\n{main}\n{:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    program.expect("a program with no errors")
}

/// Runs `members` of `Guard` in turn on one fresh instance: what each run
/// ended with and the fuel it spent, and the host afterwards.
fn run_members(program: &Program, members: &[&str]) -> (Vec<(Run, u64)>, Host) {
    let mut host = Host::new()
        .with_fields(fresh_store(program, "Guard"))
        .for_entity(SUBJECT);
    let mut runs = Vec::new();
    for member in members {
        let resolved = resolve_member(program, "Guard", member, &host).expect("the member");
        let mut machine = Machine::new(program, &resolved, &[]).expect("a machine");
        runs.push(machine.run_counting(program, &mut host, u64::MAX));
    }
    (runs, host)
}

fn slot(program: &Program, host: &Host, name: &str) -> Option<Persisted> {
    let at = program.layout("Guard")?.slot_of(name)?;
    host.fields.get(at).cloned()
}

fn registered(program: &Program, field: &str) -> ScriptValue {
    program
        .declared
        .iter()
        .find(|declared| declared.name == "Guard")
        .and_then(|declared| declared.fields.iter().find(|known| known.name == field))
        .map(|known| known.default.clone())
        .expect("the field is declared")
}

/// **A field of struct type written with no default starts at a struct its
/// code can read** — or is refused. A struct whose own field has neither a
/// default nor a zero (a `Vec3`, another struct, an `Entity`) has no
/// complete value to start at.
///
/// Observed: each compiles, registers `null` as the field's default, seeds a
/// fresh instance with `null`, and the first `inner.n` faults with
/// `NotAnObject { found: "null" }`.
#[test]
fn a_struct_field_with_no_default_starts_at_a_struct_its_code_can_read() {
    for source in [
        "struct Inner { Vec3 v; int n = 2; }
         behavior Guard { Inner inner; int got = 0; void Read() { got = inner.n; } }",
        "struct Inner { int n = 2; }
         struct Outer { Inner i; }
         behavior Guard { Outer inner; int got = 0; void Read() { got = inner.i.n; } }",
        "struct Inner { Entity e; int n = 2; }
         behavior Guard { Inner inner; int got = 0; void Read() { got = inner.n; } }",
    ] {
        let (program, errors) = outcome(source);
        let Some(program) = program.filter(|_| errors.is_empty()) else {
            continue;
        };
        assert_ne!(
            registered(&program, "inner"),
            ScriptValue::Null,
            "a non-optional struct field registers `null`:\n{source}"
        );
        let (runs, host) = run_members(&program, &["Read"]);
        assert_eq!(runs[0].0, Run::Completed, "{source}");
        assert_eq!(
            slot(&program, &host, "got"),
            Some(Persisted::Scalar(Value::Int(2))),
            "{source}"
        );
    }
}

/// **A state datum of struct type written with no default holds a struct
/// once the state is entered** — the first state, on a fresh instance — or
/// is refused.
///
/// Observed: it compiles, and reading `seen.n` faults with `NotAnObject {
/// found: "null" }`.
#[test]
fn a_struct_state_datum_with_no_default_holds_a_struct_its_code_can_read() {
    let source = "struct Inner { Vec3 v; int n = 2; }
         behavior Guard {
             int got = 0;
             state Idle { Inner seen; void Read() { got = seen.n; } }
         }";
    let (program, errors) = outcome(source);
    let Some(program) = program.filter(|_| errors.is_empty()) else {
        return;
    };
    let (runs, host) = run_members(&program, &["Read"]);
    assert_eq!(runs[0].0, Run::Completed);
    assert_eq!(
        slot(&program, &host, "got"),
        Some(Persisted::Scalar(Value::Int(2)))
    );
}

/// **A `float` field whose default is written `3` starts at the float its
/// component registers**: the registered default is the runtime default.
///
/// Observed: the component registers `Float(3.0)`; the fresh store holds
/// `Int(3)`.
#[test]
fn an_int_written_float_field_default_is_the_float_its_component_registers() {
    let program = build("behavior Guard { float speed = 3; float[] marks = [1, 2]; }");
    let layout = program.layout("Guard").expect("`Guard`");
    let store = layout.fresh_store();
    assert_eq!(registered(&program, "speed"), ScriptValue::Float(3.0));
    assert_eq!(
        store.get(layout.slot_of("speed").expect("`speed`")),
        Some(&Persisted::Scalar(Value::Float(3.0))),
        "a fresh instance's `speed`"
    );
}

/// **A `float` state datum written `1` is a float**: written into another
/// component's `float` field, it arrives as `1.0`.
///
/// Observed: `this.Set(Body { mass: t })` queues `mass: Int(1)`.
#[test]
fn an_int_written_float_state_datum_is_written_out_as_a_float() {
    let program = build(
        "component Body { float mass = 0.5; }
         behavior Guard {
             state Idle {
                 float t = 1;
                 void Go() { this.Set(Body { mass: t }); }
             }
         }",
    );
    let (runs, host) = run_members(&program, &["Go"]);
    assert_eq!(runs[0].0, Run::Completed);
    assert_eq!(
        host.commands.as_slice().to_vec(),
        vec![WorldCommand::SetComponent {
            entity: SUBJECT,
            component: khora_core::script::ComponentName::new("Body"),
            value: ScriptValue::Struct(vec![("mass".to_owned(), ScriptValue::Float(1.0))]),
        }]
    );
}

/// **Entering a state pays for the data it writes**, as building that data
/// in code does: a five-hundred-element array a state's datum starts at is not
/// one unit of work.
///
/// Observed: `become Big` and `become Empty` both spend 3 fuel.
#[test]
fn entering_a_state_pays_for_the_data_it_writes() {
    let elements = vec!["1"; 500].join(", ");
    let program = build(&format!(
        "behavior Guard {{
             state Idle {{ void Small() {{ become Empty; }} void Large() {{ become Big; }} }}
             state Empty {{ }}
             state Big {{ int[] xs = [{elements}]; }}
         }}"
    ));
    let (small, _) = run_members(&program, &["Small"]);
    let (large, _) = run_members(&program, &["Large"]);
    assert!(
        large[0].1 >= small[0].1 + 500,
        "entering `Big` spent {}, entering `Empty` {}",
        large[0].1,
        small[0].1
    );
}
