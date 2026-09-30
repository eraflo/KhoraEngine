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

//! Attacks on the conformance suite and on the fuel overdraft.

use khora_script::vm::{Machine, Program, Run, Suspension, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// A call that costs more than a small slice.
#[ergon_fn(name = "BreakerCostly", cost = 50)]
fn breaker_costly() -> f32 {
    3.0
}

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

/// Runs `function` to completion `slice` fuel at a time; returns its value and
/// the total fuel `run_counting` reported.
fn sliced(program: &Program, function: &str, args: &[Value], slice: u64) -> (Value, u64) {
    let mut machine = Machine::new(program, function, args).expect("entry exists");
    let mut host = Host::new();
    let mut total = 0;
    for _ in 0..100_000 {
        let (run, cost) = machine.run_counting(program, &mut host, slice);
        total += cost;
        match run {
            Run::Completed => return (machine.result(), total),
            Run::Suspended(_) => {}
            Run::Faulted(fault) => panic!("faulted at a slice of {slice}: {fault:?}"),
        }
    }
    panic!("no end at a slice of {slice}");
}

// ─── The overdraft ──────────────────────────────────────────────────────────

/// Coverage: the overdraft is paid in full, reported in full, and is one
/// instruction — the next one waits for the next run.
#[test]
fn an_overdraft_is_one_instruction_reported_in_full() {
    let program = build("fn float Main() { return BreakerCostly(); }");
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    let mut host = Host::new();

    assert_eq!(
        machine.run_counting(&program, &mut host, 1),
        (Run::Suspended(Suspension::OutOfFuel), 50),
        "the call is paid past the slice, and the run reports what it cost"
    );
    assert_eq!(machine.program_counter(), 1, "one instruction, not two");
    assert_eq!(
        machine.run_counting(&program, &mut host, 1),
        (Run::Completed, 1)
    );
    assert_eq!(machine.result(), Value::Float(3.0));
}

/// Coverage: zero fuel is not a slice smaller than an instruction — it runs
/// nothing, even when the next instruction is the expensive one.
#[test]
fn zero_fuel_never_overdraws() {
    let program = build("fn float Main() { return BreakerCostly(); }");
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");

    assert_eq!(
        machine.run_counting(&program, &mut Host::new(), 0),
        (Run::Suspended(Suspension::OutOfFuel), 0)
    );
    assert_eq!(machine.program_counter(), 0);
}

/// Coverage: slicing changes when work happens, never how much it costs. The
/// sum `run_counting` reports over any slicing equals one unsliced run — the
/// number the script lane's budget accounting is built on.
#[test]
fn slicing_never_changes_what_a_run_costs() {
    let program = build(
        "fn float Main() {
             float total = 0.0;
             for (int i = 0; i < 4; i = i + 1) { total = total + BreakerCostly(); }
             return total;
         }",
    );
    let (whole, cost) = sliced(&program, "Main", &[], u64::MAX);
    assert_eq!(whole, Value::Float(12.0));

    for slice in 1..=120 {
        assert_eq!(
            sliced(&program, "Main", &[], slice),
            (whole, cost),
            "a slice of {slice}"
        );
    }
}

// ─── The suite's rows ───────────────────────────────────────────────────────

/// Coverage: the shipped rows "`&&` short-circuits" and "`||` short-circuits"
/// (`false && true`, `true || false`) give the same answer under eager
/// evaluation. Only a right-hand side that would fault tells the two apart.
#[test]
fn logical_operators_really_short_circuit() {
    let program = build(
        "fn bool And() { int zero = 0; return false && 1 / zero == 0; }
         fn bool Or() { int zero = 0; return true || 1 / zero == 0; }",
    );
    assert_eq!(sliced(&program, "And", &[], u64::MAX).0, Value::Bool(false));
    assert_eq!(sliced(&program, "Or", &[], u64::MAX).0, Value::Bool(true));
}
