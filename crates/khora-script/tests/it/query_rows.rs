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

//! Which rows a sweep reaches, and how a body leaves it.
//!
//! The rows are taken when the sweep starts and each is checked again when it
//! is reached: a row an earlier row's body took out of the query — a bound
//! component removed, a `without` added — is skipped. `break`, `continue` and
//! `return` leave a query as they leave any loop, the innermost one first. A
//! query run again — inside another loop — takes its rows again.

use std::collections::HashMap;
use std::sync::Arc;

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue,
};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE};

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
component Dead { float since; }
";

fn build(main: &str) -> Program {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
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

fn struct_of(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

fn projected(
    name: &str,
    holders: &[(u32, ScriptValue)],
    default: ScriptValue,
) -> ProjectedComponent {
    let rows = holders.iter().map(|(_, row)| row.clone()).collect();
    let index: HashMap<EntityId, (u32, u32)> = holders
        .iter()
        .enumerate()
        .map(|(row, (index, _))| {
            (
                EntityId {
                    index: *index,
                    generation: 1,
                },
                (0, row as u32),
            )
        })
        .collect();
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(rows))],
        index,
        Some(default),
    )
}

/// Entities 10, 20, … 70 hold `Health` with `current` 1 … 7 and `Regen`;
/// entity 40 is `Dead`, so `without Dead` sweeps 1, 2, 3, 5, 6, 7.
fn projection() -> ComponentProjection {
    let health = |current: i64| {
        struct_of(&[
            ("current", ScriptValue::Int(current)),
            ("max", ScriptValue::Int(10)),
        ])
    };
    let regen = struct_of(&[("rate", ScriptValue::Float(1.0))]);
    let dead = struct_of(&[("since", ScriptValue::Float(0.0))]);
    let holders: Vec<(u32, ScriptValue)> = [7, 3, 1, 6, 4, 2, 5]
        .into_iter()
        .map(|n| (n * 10, health(i64::from(n))))
        .collect();
    let regens: Vec<(u32, ScriptValue)> = [5, 1, 7, 2, 6, 3, 4]
        .into_iter()
        .map(|n| (n * 10, regen.clone()))
        .collect();
    ComponentProjection::new(vec![
        projected("Health", &holders, health(100)),
        projected("Regen", &regens, regen.clone()),
        projected("Dead", &[(40, dead.clone())], dead),
    ])
}

/// Runs `Census.<function>` whole on a fresh instance, returning its result.
fn run(program: &Program, function: &str) -> Value {
    let mut host = Host::new()
        .with_fields(fresh_store(program, "Census"))
        .for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection()));
    let name = format!("Census.{function}");
    let mut machine = Machine::new(program, &name, &[]).expect("the function exists");
    assert_eq!(
        machine.run(program, &mut host, u64::MAX),
        Run::Completed,
        "`{name}` completes"
    );
    machine.result()
}

/// **A row an earlier row's body took out of the query is skipped.** At the
/// first row, the body removes `Health` from entity 50 and gives entity 60
/// `Dead`: both were taken as rows, neither still matches when reached — and
/// the rows around them are visited as before.
#[test]
fn a_row_an_earlier_row_took_out_of_the_query_is_skipped() {
    let program = build(
        "behavior Census {
             int Sweep() {
                 int total = 0;
                 foreach (Entity e, Health h) without Dead {
                     if (h.current == 1) {
                         foreach (Entity f, Health k) {
                             if (k.current == 5) { f.Remove(Health); }
                             if (k.current == 6) { f.Add(Dead { since: 0.0 }); }
                         }
                     }
                     total = total * 10 + h.current;
                 }
                 return total;
             }
         }",
    );

    assert_eq!(run(&program, "Sweep"), Value::Int(1237));
}

/// **`break`, `continue` and `return` leave a query as they leave a loop**:
/// `continue` goes on to the next row, `break` ends the sweep, and inside a
/// nested query both are the inner sweep's — `return` leaves every sweep.
#[test]
fn break_continue_and_return_leave_a_query_as_they_leave_a_loop() {
    let program = build(
        "behavior Census {
             int Flat() {
                 int total = 0;
                 foreach (Entity e, Health h) {
                     if (h.current == 2) { continue; }
                     if (h.current == 5) { break; }
                     total = total * 10 + h.current;
                 }
                 return total;
             }
             int Inner() {
                 int total = 0;
                 foreach (Health h) {
                     foreach (Health k) {
                         if (k.current == 3) { break; }
                         if (k.current == 1) { continue; }
                         total += 1;
                     }
                     total = total * 10;
                 }
                 return total;
             }
             int Out() {
                 foreach (Health h) without Dead {
                     foreach (Health k, Regen r) {
                         if (h.current == 2 && k.current == 3) {
                             return h.current * 10 + k.current;
                         }
                     }
                 }
                 return -1;
             }
         }",
    );

    assert_eq!(
        run(&program, "Flat"),
        Value::Int(134),
        "1, 3, 4, then 5 breaks"
    );
    // Each outer row: the inner sweep skips 1, counts 2, stops at 3.
    assert_eq!(run(&program, "Inner"), Value::Int(11_111_110));
    assert_eq!(run(&program, "Out"), Value::Int(23));
}

/// **A query run again takes its rows again.** Inside a `while`, the second
/// and third sweeps see the `Dead` the first one added to entity 10.
#[test]
fn a_query_in_a_loop_takes_its_rows_again_each_time() {
    let program = build(
        "behavior Census {
             int Sweeps() {
                 int total = 0;
                 int pass = 0;
                 while (pass < 3) {
                     int seen = 0;
                     foreach (Entity e, Health h) without Dead {
                         seen += 1;
                         if (pass == 0 && h.current == 1) { e.Add(Dead { since: 1.0 }); }
                     }
                     total = total * 10 + seen;
                     pass += 1;
                 }
                 return total;
             }
         }",
    );

    assert_eq!(run(&program, "Sweeps"), Value::Int(655));
}
