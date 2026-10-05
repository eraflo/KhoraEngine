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

//! `a ?? b` and the numeric casts: each gives the value it names, or stops.
//!
//! `??` falls back only when its left side is null, and evaluates its right
//! side only then — like `&&`. `(float)i` produces a float, `(int)f` an int
//! truncated toward zero, and a float no `int` holds faults rather than
//! saturating into a number nobody wrote.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Host};

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

/// Runs `machine` to completion on `host`, `slice` fuel at a time; returns its
/// value and the fuel it reported, or the fault that stopped it.
fn finish(
    mut machine: Machine,
    program: &Program,
    host: &mut Host,
    slice: u64,
) -> Result<(Value, u64), Fault> {
    let mut spent = 0;
    for _ in 0..100_000 {
        let (run, cost) = machine.run_counting(program, host, slice);
        spent += cost;
        match run {
            Run::Completed => return Ok((machine.result(), spent)),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end at a slice of {slice}");
}

/// Runs the free function `function` of `program`.
fn run(program: &Program, function: &str, args: &[Value]) -> Result<Value, Fault> {
    let machine = Machine::new(program, function, args)
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    finish(machine, program, &mut Host::new(), u64::MAX).map(|(value, _)| value)
}

/// The fuel one complete run of `function` costs.
fn cost_of(program: &Program, function: &str, args: &[Value]) -> u64 {
    let machine = Machine::new(program, function, args)
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    finish(machine, program, &mut Host::new(), u64::MAX)
        .unwrap_or_else(|fault| panic!("`{function}` faulted: {fault:?}"))
        .1
}

/// Runs `member` on a fresh instance of `behavior`, created as the engine
/// creates one: fields sized from the layout, declared defaults run.
fn run_member(program: &Program, behavior: &str, member: &str) -> Result<Value, Fault> {
    let layout = program
        .layout(behavior)
        .unwrap_or_else(|| panic!("`{behavior}` is a behavior of the program"));
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("a field initialiser");
    finish(init, program, &mut host, u64::MAX)?;

    let function = resolve_member(program, behavior, member, &host)
        .unwrap_or_else(|| panic!("`{behavior}` has a member `{member}`"));
    let machine = Machine::new(program, &function, &[]).expect("the member exists");
    finish(machine, program, &mut host, u64::MAX).map(|(value, _)| value)
}

// ─── `??` ───────────────────────────────────────────────────────────────────

/// A present left side is the result. The second form's fallback divides by
/// zero: evaluated eagerly it faults, so only a lazy `??` returns the 3.
#[test]
fn coalesce_keeps_a_present_value() {
    let program = build(
        "fn int Plain() { int? s = 3; return s ?? 7; }
         fn int Guarded() { int? s = 3; int zero = 0; return s ?? 1 / zero; }",
    );

    assert_eq!(run(&program, "Plain", &[]), Ok(Value::Int(3)));
    assert_eq!(run(&program, "Guarded", &[]), Ok(Value::Int(3)));
}

/// **Lazy, like `&&`.** The fallback writes a field each time it runs: with a
/// present left side it never runs (`calls` stays 0); with null it runs once.
#[test]
fn coalesce_does_not_evaluate_the_right_side_when_the_left_is_present() {
    let program = build(
        "behavior Probe {
             int calls = 0;
             int Fallback() { calls += 1; return 7; }
             int Present() {
                 int? s = 3;
                 int got = s ?? Fallback();
                 return calls * 100 + got;
             }
             int Absent() {
                 int? s = null;
                 int got = s ?? Fallback();
                 return calls * 100 + got;
             }
         }",
    );

    assert_eq!(
        run_member(&program, "Probe", "Present"),
        Ok(Value::Int(3)),
        "the fallback ran although the left side was present"
    );
    assert_eq!(
        run_member(&program, "Probe", "Absent"),
        Ok(Value::Int(107)),
        "the fallback runs exactly once when the left side is null"
    );
}

/// `s ?? 1` with `s: float?` is a `float` — the checker says so — whichever
/// side supplied it, so it divides as a float: `2.5 / 2` and `1 / 2` are 1.25
/// and 0.5, not a fault on a float read as an int, nor an integer `0`.
#[test]
fn coalesce_takes_the_type_of_its_optional() {
    let program = build(
        "fn float Present() { float? s = 2.5; return (s ?? 1) / 2; }
         fn float Absent() { float? s = null; return (s ?? 1) / 2; }",
    );

    assert_eq!(run(&program, "Present", &[]), Ok(Value::Float(1.25)));
    assert_eq!(run(&program, "Absent", &[]), Ok(Value::Float(0.5)));
}

// ─── `(int)` ────────────────────────────────────────────────────────────────

/// `(int)` truncates toward zero, on both sides of it.
#[test]
fn cast_truncates_toward_zero() {
    let program = build(
        "fn int Negative() { return (int)-2.75; }
         fn int Positive() { return (int)2.75; }
         fn int SmallNegative() { return (int)-0.5; }
         fn int Param(float x) { return (int)x; }",
    );

    assert_eq!(run(&program, "Negative", &[]), Ok(Value::Int(-2)));
    assert_eq!(run(&program, "Positive", &[]), Ok(Value::Int(2)));
    assert_eq!(run(&program, "SmallNegative", &[]), Ok(Value::Int(0)));
    assert_eq!(
        run(&program, "Param", &[Value::Float(-7.9)]),
        Ok(Value::Int(-7))
    );
}

/// A float no `int` holds — infinite, NaN, or past the `i64` range — faults
/// with `InvalidCast` instead of saturating into a number nobody wrote.
#[test]
fn cast_of_a_non_finite_float_faults() {
    let program = build(
        "fn int Infinite() { return (int)(1.0 / 0.0); }
         fn int NegativeInfinite() { return (int)(-1.0 / 0.0); }
         fn int NotANumber() { return (int)(0.0 / 0.0); }
         fn int TooLarge() { float big = 1000000000000.0 * 100000000000.0; return (int)big; }",
    );

    for function in ["Infinite", "NegativeInfinite", "NotANumber", "TooLarge"] {
        let got = run(&program, function, &[]);
        assert!(
            matches!(got, Err(Fault::InvalidCast { .. })),
            "`{function}`: expected an InvalidCast fault, got {got:?}"
        );
    }
}

/// The edge of the range. `-2^63` is `i64::MIN`, an int. `2^63` is one past
/// `i64::MAX` — and also what `i64::MAX as f32` rounds to, so a range check
/// written against that constant lets it through to a silent saturation.
#[test]
fn cast_accepts_the_whole_int_range_and_nothing_past_it() {
    let program = build(
        "fn int Lowest() { return (int)-9223372036854775808.0; }
         fn int PastHighest() { return (int)9223372036854775808.0; }",
    );

    assert_eq!(run(&program, "Lowest", &[]), Ok(Value::Int(i64::MIN)));
    let got = run(&program, "PastHighest", &[]);
    assert!(
        matches!(got, Err(Fault::InvalidCast { .. })),
        "2^63 is outside `i64`: expected an InvalidCast fault, got {got:?}"
    );
}

/// What `(int)` produces is an int the integer instructions accept — today
/// `(int)2.75 + 1` hands a float to `AddInt` and faults.
#[test]
fn cast_then_int_arithmetic_works() {
    let program = build(
        "fn int Sum() { return (int)2.75 + 1; }
         fn int Remainder() { return (int)7.5 % 4; }",
    );

    assert_eq!(run(&program, "Sum", &[]), Ok(Value::Int(3)));
    assert_eq!(run(&program, "Remainder", &[]), Ok(Value::Int(3)));
}

// ─── `(float)` ──────────────────────────────────────────────────────────────

/// `(float)` produces a float value, not the int it was given.
#[test]
fn cast_of_an_int_to_float_is_a_float() {
    let program = build(
        "fn float Local() { int i = 3; return (float)i; }
         fn float Negative() { return (float)-4; }",
    );

    assert_eq!(run(&program, "Local", &[]), Ok(Value::Float(3.0)));
    assert_eq!(run(&program, "Negative", &[]), Ok(Value::Float(-4.0)));
}

// ─── What a cast costs and is ───────────────────────────────────────────────

/// A cast is one instruction and costs one fuel (RULES §7b): the same function
/// with and without it differs by exactly that.
#[test]
fn a_cast_costs_one_fuel() {
    let program = build(
        "fn int ToInt(float x) { return (int)x; }
         fn float SameFloat(float x) { return x; }
         fn float ToFloat(int x) { return (float)x; }
         fn int SameInt(int x) { return x; }",
    );

    assert_eq!(
        cost_of(&program, "ToInt", &[Value::Float(2.5)]),
        cost_of(&program, "SameFloat", &[Value::Float(2.5)]) + 1
    );
    assert_eq!(
        cost_of(&program, "ToFloat", &[Value::Int(2)]),
        cost_of(&program, "SameInt", &[Value::Int(2)]) + 1
    );
}

/// A cast is code: adding one to a function changes its fingerprint, so a hot
/// reload sees the edit.
#[test]
fn a_cast_changes_its_function_fingerprint() {
    let without = build("fn float F(float x) { return x; }");
    let with = build("fn int F(float x) { return (int)x; }");
    let widened = build("fn float F(int x) { return (float)x; }");
    let plain = build("fn int F(int x) { return x; }");

    let fingerprint = |program: &Program| program.function("F").expect("F").fingerprint;
    assert_ne!(fingerprint(&without), fingerprint(&with), "(int)");
    assert_ne!(fingerprint(&plain), fingerprint(&widened), "(float)");
}
