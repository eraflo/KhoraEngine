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

//! Compiler tests.
//!
//! These run the compiled program rather than inspecting the instructions.
//! Asserting on a specific opcode sequence would pin the compiler's current
//! choices in place and break on every improvement; asserting on the *answer*
//! tests what the language promises.
//!
//! The exception is the suspension invariant, which is about the shape of the
//! generated code and is therefore checked by running the same program under
//! every possible fuel budget.

use super::compile;
use crate::lexer::lex;
use crate::native::Host;
use crate::parser::parse;
use crate::types::check;
use crate::vm::{Machine, Program, Run, Suspension, Value};

/// Compiles a source string, insisting it lexes, parses and type-checks first.
fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(!lexed.has_errors(), "lex errors: {:?}", lexed.diagnostics);

    let parsed = parse(lexed.tokens);
    assert!(
        !parsed.has_errors(),
        "parse errors: {:?}",
        parsed
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checked = check(&parsed.module);
    assert!(
        !checked.has_errors(),
        "type errors: {:?}",
        checked
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let compiled = compile(&parsed.module);
    assert!(
        !compiled.has_errors(),
        "compile errors: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    compiled.program
}

/// Compiles and runs `entry`, returning what it produced.
fn run(source: &str, entry: &str) -> Value {
    let program = build(source);
    let mut machine = Machine::new(&program, entry, &[]).expect("the entry point exists");
    match machine.run(&program, &mut Host::new(), 100_000) {
        Run::Completed => machine.result(),
        other => panic!("expected completion, got {other:?}"),
    }
}

#[test]
fn arithmetic_produces_the_right_answer() {
    assert_eq!(run("fn int F() { return 1 + 2 * 3; }", "F"), Value::Int(7));
    assert_eq!(
        run("fn int F() { return (1 + 2) * 3; }", "F"),
        Value::Int(9)
    );
    assert_eq!(run("fn int F() { return 7 % 3; }", "F"), Value::Int(1));
    assert_eq!(run("fn int F() { return -5 + 2; }", "F"), Value::Int(-3));
}

/// Integer and float arithmetic compile to different instructions, so both are
/// checked — an int path that silently used the float one would round.
#[test]
fn integer_and_float_arithmetic_stay_apart() {
    assert_eq!(run("fn int F() { return 7 / 2; }", "F"), Value::Int(3));
    assert_eq!(
        run("fn float F() { return 7.0 / 2.0; }", "F"),
        Value::Float(3.5)
    );
}

/// A mixed pair compiles to the float form, matching the one implicit widening
/// the language allows.
#[test]
fn mixing_an_int_with_a_float_gives_a_float() {
    assert_eq!(
        run("fn float F() { return 1 + 0.5; }", "F"),
        Value::Float(1.5)
    );
}

/// Durations are floats at run time — the checker has already proved the units
/// agree, so the arithmetic is ordinary.
#[test]
fn durations_compute_in_seconds() {
    assert_eq!(
        run("fn Duration F() { return 2s + 500ms; }", "F"),
        Value::Float(2.5)
    );
}

#[test]
fn comparisons_and_negation_work() {
    assert_eq!(run("fn bool F() { return 1 < 2; }", "F"), Value::Bool(true));
    assert_eq!(
        run("fn bool F() { return 2 <= 2; }", "F"),
        Value::Bool(true)
    );
    assert_eq!(run("fn bool F() { return 3 > 2; }", "F"), Value::Bool(true));
    assert_eq!(
        run("fn bool F() { return 2 >= 3; }", "F"),
        Value::Bool(false)
    );
    assert_eq!(
        run("fn bool F() { return 1 == 1; }", "F"),
        Value::Bool(true)
    );
    assert_eq!(
        run("fn bool F() { return 1 != 1; }", "F"),
        Value::Bool(false)
    );
    assert_eq!(
        run("fn bool F() { return !false; }", "F"),
        Value::Bool(true)
    );
}

/// Short-circuiting is not an instruction but a branch, so it gets its own
/// test: `&&` must not evaluate its right operand when the left is false.
#[test]
fn logical_operators_short_circuit() {
    assert_eq!(
        run("fn bool F() { return false && true; }", "F"),
        Value::Bool(false)
    );
    assert_eq!(
        run("fn bool F() { return true || false; }", "F"),
        Value::Bool(true)
    );
    assert_eq!(
        run("fn bool F() { return true && false; }", "F"),
        Value::Bool(false)
    );
    assert_eq!(
        run("fn bool F() { return false || false; }", "F"),
        Value::Bool(false)
    );
}

#[test]
fn variables_and_assignment_work() {
    assert_eq!(
        run("fn int F() { int x = 1; x = x + 4; return x; }", "F"),
        Value::Int(5)
    );
    assert_eq!(
        run("fn int F() { var x = 10; x -= 3; return x; }", "F"),
        Value::Int(7)
    );
}

#[test]
fn branches_take_the_right_path() {
    let source = "fn int F() { if (1 < 2) { return 10; } else { return 20; } }";
    assert_eq!(run(source, "F"), Value::Int(10));

    let other = "fn int F() { if (2 < 1) { return 10; } else { return 20; } }";
    assert_eq!(run(other, "F"), Value::Int(20));
}

#[test]
fn a_while_loop_accumulates() {
    let source = "fn int F() {
        int total = 0;
        int i = 0;
        while (i < 5) { total = total + i; i = i + 1; }
        return total;
    }";
    assert_eq!(run(source, "F"), Value::Int(10));
}

#[test]
fn a_for_loop_accumulates() {
    let source = "fn int F() {
        int total = 0;
        for (int i = 0; i < 5; i = i + 1) { total = total + i; }
        return total;
    }";
    assert_eq!(run(source, "F"), Value::Int(10));
}

#[test]
fn functions_call_each_other() {
    let source = "fn int Double(int n) { return n * 2; }
                  fn int F() { return Double(21); }";
    assert_eq!(run(source, "F"), Value::Int(42));
}

/// Arguments have to reach the callee in the right order and the right slots,
/// which several-argument calls are the only way to catch.
#[test]
fn arguments_arrive_in_order() {
    let source = "fn int Sub(int a, int b) { return a - b; }
                  fn int F() { return Sub(10, 3); }";
    assert_eq!(run(source, "F"), Value::Int(7));

    let nested = "fn int Sub(int a, int b) { return a - b; }
                  fn int F() { return Sub(Sub(10, 3), 2); }";
    assert_eq!(run(nested, "F"), Value::Int(5));
}

/// A call forward must compile as readily as a call back, or declaration order
/// would decide what works.
#[test]
fn a_function_can_call_one_declared_later() {
    let source = "fn int F() { return Later(); }
                  fn int Later() { return 9; }";
    assert_eq!(run(source, "F"), Value::Int(9));
}

#[test]
fn recursion_terminates() {
    let source = "fn int Fact(int n) {
                      if (n <= 1) { return 1; }
                      return n * Fact(n - 1);
                  }
                  fn int F() { return Fact(5); }";
    assert_eq!(run(source, "F"), Value::Int(120));
}

#[test]
fn the_ternary_picks_a_branch() {
    assert_eq!(
        run("fn int F() { return 1 < 2 ? 10 : 20; }", "F"),
        Value::Int(10)
    );
}

/// A function with no `return` yields nothing rather than whatever was left in
/// a register.
#[test]
fn falling_off_the_end_returns_nothing() {
    assert_eq!(run("fn void F() { int x = 1; }", "F"), Value::Unit);
}

/// **The invariant, end to end.** A compiled program interrupted at every
/// possible point must give the answer it gives uninterrupted.
///
/// This is the property the whole design rests on, checked here against real
/// generated code rather than a hand-written instruction sequence.
#[test]
fn a_compiled_program_survives_interruption_anywhere() {
    let program = build(
        "fn int Double(int n) { return n * 2; }
         fn int F() {
             int total = 0;
             for (int i = 0; i < 5; i = i + 1) { total = total + Double(i); }
             return total;
         }",
    );

    let mut reference = Machine::new(&program, "F", &[]).expect("entry exists");
    assert_eq!(
        reference.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    let expected = reference.result();
    assert_eq!(expected, Value::Int(20));

    for slice in 1..=60u64 {
        let mut machine = Machine::new(&program, "F", &[]).expect("entry exists");
        // One host across the whole run — see the note in the VM's own
        // resumption test.
        let mut host = Host::new();
        let mut rounds = 0;
        loop {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    rounds += 1;
                    assert!(rounds < 5000, "slice {slice} is not progressing");
                }
                other => panic!("slice {slice}: {other:?}"),
            }
        }
        assert_eq!(
            machine.result(),
            expected,
            "a slice of {slice} diverged from the uninterrupted run"
        );
    }
}

/// An unbounded loop must suspend rather than hang the frame. That is what lets
/// the engine notice a behavior that never finishes instead of freezing behind
/// it — and it falls out of charging fuel for the back-edge.
#[test]
fn an_infinite_loop_suspends_rather_than_hanging() {
    let program = build("fn void F() { while (true) { } }");
    let mut machine = Machine::new(&program, "F", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), 1000),
        Run::Suspended(Suspension::OutOfFuel)
    );
    assert!(!machine.is_finished(), "it can still be resumed");
}

/// Frames must stay small: every live register is state copied on each
/// suspension, and suspensions are routine rather than exceptional.
#[test]
fn a_long_function_does_not_grow_its_frame_per_statement() {
    let program = build(
        "fn int F() {
             int a = 1 + 2;
             int b = 3 + 4;
             int c = 5 + 6;
             int d = 7 + 8;
             int e = 9 + 10;
             return a + b + c + d + e;
         }",
    );
    let function = program.function("F").expect("F was compiled");
    assert!(
        function.registers <= 12,
        "five locals should not need {} registers — temporaries are not being released",
        function.registers
    );
}

// ─── One name, one function ─────────────────────────────────────────────────

/// Compiles without insisting it succeeds, for the cases where the point is
/// that it does not.
fn errors(source: &str) -> Vec<String> {
    let parsed = parse(lex(source).tokens);
    compile(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

/// **The one an author meets without meaning to.** A member's kind is not part
/// of its compiled name, so `void Update()` and `on Update()` are one name. A
/// direct call would reach the last and a dispatch the first, which is a program
/// whose behavior depends on how it was entered.
#[test]
fn a_method_and_a_handler_of_the_same_name_are_refused() {
    let found = errors(
        "behavior Guard {
             void Update(float dt) { }
             on Update(float dt) { }
         }",
    );

    assert!(
        found.iter().any(|m| m.contains("`Guard.Update`")),
        "the collision should name the function: {found:?}"
    );
}

/// The same rule for two plain functions — the collision is about the name, not
/// about behaviors.
#[test]
fn a_free_function_declared_twice_is_refused() {
    let found = errors("fn int F() { return 1; } fn int F() { return 2; }");

    assert!(
        found.iter().any(|m| m.contains("`F`")),
        "expected a duplicate report: {found:?}"
    );
}

/// A state scopes the name, which is the whole point of writing a handler
/// inside one: `on Lost` while chasing is not `on Lost` while patrolling.
#[test]
fn the_same_handler_in_two_states_is_not_a_collision() {
    let found = errors(
        "behavior Guard {
             state Patrol { on Lost(int by) { } }
             state Chase  { on Lost(int by) { } }
         }",
    );

    assert!(found.is_empty(), "states scope their members: {found:?}");
}

/// And a state's handler may shadow the behavior's own — that pair is what
/// makes the fallback in `resolve_handler` mean something.
#[test]
fn a_state_handler_may_share_a_name_with_the_behaviors_own() {
    let found = errors(
        "behavior Guard {
             on Damaged(int by) { }
             state Chase { on Damaged(int by) { } }
         }",
    );

    assert!(found.is_empty(), "the two are distinct names: {found:?}");
}
