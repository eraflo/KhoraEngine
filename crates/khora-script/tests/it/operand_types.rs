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

//! What the compiler knows of an operand's type is what the checker proved.
//!
//! Operators, casts and `??` are lowered by the operand's type: `a + b` on two
//! `Vec3` is the native `"Vec3 + Vec3"`, `(int)f` is `FloatToInt`, `s ?? 1` on a
//! `float?` is a float. Wherever the compiler knows less than the checker —
//! a behavior member called by its bare name, a `var` holding an optional —
//! the lowering picks the wrong operation, and the program computes a value
//! nobody wrote or faults on one it should have accepted.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::{FrozenMachine, PendingBody};
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{resume, Fault, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

/// The entity a behavior's instance belongs to.
const SUBJECT: EntityId = EntityId {
    index: 1,
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

/// Runs `machine` to the end, every `await` treated as elapsed.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("still running after 1000 resumes");
}

/// A fresh instance of `behavior`: fields sized from the layout, defaults run.
fn instance(program: &Program, behavior: &str) -> Host {
    let layout = program
        .layout(behavior)
        .unwrap_or_else(|| panic!("`{behavior}` is a behavior of the program"));
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("a field initialiser");
    finish(init, program, &mut host).expect("the defaults run");
    host
}

/// Runs `member` on a fresh instance of `behavior`.
fn run_member(program: &Program, behavior: &str, member: &str) -> Result<Value, Fault> {
    let mut host = instance(program, behavior);
    let function = resolve_member(program, behavior, member, &host)
        .unwrap_or_else(|| panic!("`{behavior}` has a member `{member}`"));
    let machine = Machine::new(program, &function, &[]).expect("the member exists");
    finish(machine, program, &mut host)
}

// ─── A member's result, called by its bare name ─────────────────────────────

/// Inside a behavior, `Dir()` calls the sibling member `Dir` — and the checker
/// types it `Vec3`. The operator on it must be `Vec3 + Vec3`, and `-Dir()`,
/// `Dir() * 2` the vector forms, exactly as on a local holding the same value.
///
/// Observed: `Dir() + Vec3(0, 1, 0)` returns `Dir()` unchanged (the lowering
/// cannot find `"? + Vec3"` and moves the left operand into the result);
/// `-Dir()` and `Dir() * 2` fault with `TypeMismatch { expected: "int", found:
/// "Vec3" }`.
#[test]
fn an_operator_on_a_sibling_members_vector_result_is_the_vector_operation() {
    let program = build(
        "behavior Probe {
             Vec3 Dir() { return Vec3(1.0, 0.0, 0.0); }
             Vec3 Sum() { return Dir() + Vec3(0.0, 1.0, 0.0); }
             Vec3 Negated() { return -Dir(); }
             Vec3 Scaled() { return Dir() * 2; }
         }",
    );

    let got = [
        run_member(&program, "Probe", "Sum"),
        run_member(&program, "Probe", "Negated"),
        run_member(&program, "Probe", "Scaled"),
    ];
    assert_eq!(
        got,
        [
            Ok(Value::Vec3(Vec3::new(1.0, 1.0, 0.0))),
            Ok(Value::Vec3(Vec3::new(-1.0, 0.0, 0.0))),
            Ok(Value::Vec3(Vec3::new(2.0, 0.0, 0.0))),
        ],
        "(Sum, Negated, Scaled)"
    );
}

/// `(int)Half()` on a sibling member returning `float` converts, as it does on
/// a local: the `int` member returns `2`, and `+ 1` on it is integer addition.
/// `MaybeHalf() ?? 1` on a sibling's `float?` is a float and divides as one.
///
/// Observed: `(int)Half()` returns `Float(2.5)` from an `int` member;
/// `(int)Half() + 1` and `(MaybeHalf() ?? 1) / 2` fault with `TypeMismatch {
/// expected: "int", found: "float" }`.
#[test]
fn a_cast_or_coalesce_of_a_sibling_members_result_converts() {
    let program = build(
        "behavior Probe {
             float Half() { return 2.5; }
             float? MaybeHalf() { return 2.5; }
             int Truncated() { return (int)Half(); }
             int TruncatedPlusOne() { return (int)Half() + 1; }
             float Halved() { return (MaybeHalf() ?? 1) / 2; }
         }",
    );

    let got = [
        run_member(&program, "Probe", "Truncated"),
        run_member(&program, "Probe", "TruncatedPlusOne"),
        run_member(&program, "Probe", "Halved"),
    ];
    assert_eq!(
        got,
        [Ok(Value::Int(2)), Ok(Value::Int(3)), Ok(Value::Float(1.25))],
        "(Truncated, TruncatedPlusOne, Halved)"
    );
}

// ─── A `var` holding an optional, across an edit ────────────────────────────

/// `Guard` with `members`.
fn guard(members: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int landed = 0;
             {members}
         }}"
    ))
}

/// `var b = maybe` with `maybe: int?` is an `int?` local, and a resumed frame
/// knows it as one. An edit that makes `b` an `int` (`int b = 7;`) cannot carry
/// its `null` across: the body restarts and lands `8` — or, rebuilt, never
/// hands a `null` to integer arithmetic.
///
/// Observed: the site records `b` as `int` (the shape of `int?` is now `int`,
/// and a `var` is recorded by its shape), so the frame is `Rebuilt` with `b =
/// null` in an `int` local, and `b + 1` faults with `TypeMismatch { expected:
/// "int", found: "null" }`.
#[test]
fn a_var_holding_an_optional_is_not_carried_into_a_non_optional_local() {
    let original = guard(
        "async void Attack() {
             int? maybe = null;
             var b = maybe;
             await 1.0s;
             landed = b ?? 41;
         }",
    );
    let edited = guard(
        "async void Attack() {
             int? maybe = null;
             int b = 7;
             await 1.0s;
             landed = b + 1;
         }",
    );

    let mut host = instance(&original, "Guard");
    let mut machine = Machine::new(&original, "Guard.Attack", &[]).expect("the member exists");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    host.awaiting = None;
    let frozen: FrozenMachine = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("a machine of this program freezes");

    let (resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(host.fields.clone())
        .for_entity(SUBJECT);
    let outcome = finish(resumed, &edited, &mut host);
    let slot = edited
        .layout("Guard")
        .and_then(|layout| layout.slot_of("landed"))
        .expect("`landed` is a field");
    let landed = match host.fields.get(slot) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    };

    assert!(
        outcome.is_ok(),
        "resumed as {tier:?}, the edited body faulted: {outcome:?}"
    );
    assert_ne!(tier, ResumeTier::Exact, "the premise: the program changed");
    assert_eq!(landed, Some(8), "resumed as {tier:?}");
}
