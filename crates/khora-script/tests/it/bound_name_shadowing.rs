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

//! A name bound by `if (var …)`, `while (var …)` or a `match` arm, and a local
//! of the branch that shadows it, never take each other's value when a frame
//! is rebuilt.
//!
//! The checker lets the branch redeclare the bound name — the binding is a
//! scope of its own, the branch's block another. When an edit removes the
//! shadowing local, the frame suspended after it must come back with the
//! bound value where the edited code reads it, not the removed shadow's.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 6,
    generation: 1,
};

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

/// `Guard` with `Attack(int? a)` holding `body`.
fn guard(body: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int landed = 0;
             int left = 1;
             int? Next() {{
                 if (left == 0) {{ return null; }}
                 left -= 1;
                 return 5;
             }}
             async void Attack(int? a) {{
                 {body}
             }}
         }}"
    ))
}

fn finish(machine: &mut Machine, program: &Program, host: &mut Host) {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return,
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// `Guard.Attack(5)` of `original` stopped at its await, resumed into `edited`
/// and run to its end: `landed` afterwards, whichever tier took it back.
fn landed_after_resume(original: &Program, edited: &Program) -> Option<i64> {
    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, original, &mut host);
    let mut machine = Machine::new(original, "Guard.Attack", &[Value::Int(5)]).expect("Attack");
    assert_eq!(
        machine.run(original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(original, PendingBody::Sequence)
        .expect("a machine of its program freezes");

    let (mut machine, _tier) = resume(&frozen, original.fingerprint(), edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(host.fields.clone())
        .for_entity(SUBJECT);
    finish(&mut machine, edited, &mut host);

    let layout = edited.layout("Guard").expect("a layout");
    match host.fields.get(layout.slot_of("landed")?) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    }
}

/// **`if (var x = a)`.** The then-branch shadowed `x` with `x * 10` before
/// its await; the edit removes the shadow. The edited code reads the bound
/// `x`, 5 — not the removed shadow's 50.
#[test]
fn an_if_var_name_does_not_take_the_value_of_a_removed_shadow() {
    let original = guard(
        "if (var x = a) {
             int x = x * 10;
             await 1.0s;
             landed = x;
         }",
    );
    let edited = guard(
        "if (var x = a) {
             await 1.0s;
             landed = x;
         }",
    );

    assert_eq!(landed_after_resume(&original, &edited), Some(5));
}

/// **A `match` arm.** The same edit inside an `int t` arm.
#[test]
fn a_match_arm_name_does_not_take_the_value_of_a_removed_shadow() {
    let original = guard(
        "match (a) {
             int t => {
                 int t = t * 10;
                 await 1.0s;
                 landed = t;
             }
             null => { landed = -1; }
         }",
    );
    let edited = guard(
        "match (a) {
             int t => {
                 await 1.0s;
                 landed = t;
             }
             null => { landed = -1; }
         }",
    );

    assert_eq!(landed_after_resume(&original, &edited), Some(5));
}

/// **`while (var t = Next())`.** The same edit in the loop's body.
#[test]
fn a_while_var_name_does_not_take_the_value_of_a_removed_shadow() {
    let original = guard(
        "while (var t = Next()) {
             int t = t * 10;
             await 1.0s;
             landed = t;
         }",
    );
    let edited = guard(
        "while (var t = Next()) {
             await 1.0s;
             landed = t;
         }",
    );

    assert_eq!(landed_after_resume(&original, &edited), Some(5));
}
