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

//! An operation on an array pays for its size.
//!
//! Building a thousand-element literal, copying a large array, loading one
//! from a field: each does work proportional to the elements it touches, and
//! is charged that much fuel. Otherwise `[0, 0, …]` with ten thousand elements
//! would cost one unit and the frame budget the language exists to respect
//! would be a fiction. A copy (or a field load) that the fuel left in a run
//! cannot pay for waits for the next run, unless it is the run's first step.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Machine, Program, Run, Suspension, Value};
use khora_script::{check, compile, lex, parse, Host};

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
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// `count` integer literals `0, 1, …`, comma-separated.
fn ints(count: usize) -> String {
    (0..count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What one whole run of `machine` spends, and how it ended.
fn spend_whole(machine: &mut Machine, program: &Program, host: &mut Host) -> (Run, u64) {
    machine.run_counting(program, host, u64::MAX)
}

/// The fuel `function` spends, run whole.
fn cost_of(program: &Program, function: &str) -> u64 {
    let mut machine = Machine::new(program, function, &[]).expect("the function exists");
    let (run, spent) = spend_whole(&mut machine, program, &mut Host::new());
    assert_eq!(run, Run::Completed, "`{function}` completes");
    spent
}

/// Runs `machine` `slice` fuel at a time, a fresh frame between runs, until it
/// completes; every run's spend, and the result.
fn sliced(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    slice: u64,
) -> (Vec<u64>, Value) {
    let mut spends = Vec::new();
    for _ in 0..100_000 {
        let (run, spent) = machine.run_counting(program, host, slice);
        spends.push(spent);
        match run {
            Run::Completed => return (spends, machine.result()),
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
            Run::Faulted(fault) => panic!("faulted at slice {slice}: {fault:?}"),
        }
    }
    panic!("no end at slice {slice}");
}

/// A thousand elements, then a statement after them to stop at.
fn big_literal() -> String {
    format!(
        "fn int F() {{
             int[] xs = [{}];
             int n = xs.Length;
             return xs[999] + n;
         }}",
        ints(1000)
    )
}

// ─── Building ───────────────────────────────────────────────────────────────

/// **A 1 000-element literal is not one unit of fuel.** Handed ten, the run
/// that builds it spends at least the thousand elements it built and stops at
/// the next safepoint; resumed ten at a time, the body completes with every
/// element in place. The compiler's bound on what a run can spend past its
/// fuel counts the literal.
#[test]
fn building_a_big_array_costs_fuel() {
    let program = build(&big_literal());

    let mut machine = Machine::new(&program, "F", &[]).expect("the function exists");
    let mut host = Host::new();
    let (run, spent) = machine.run_counting(&program, &mut host, 10);
    assert_eq!(
        run,
        Run::Suspended(Suspension::OutOfFuel),
        "ten fuel does not build a thousand elements and finish"
    );
    assert!(spent >= 1000, "the run building it spent only {spent}");

    let mut machine = Machine::new(&program, "F", &[]).expect("the function exists");
    let (spends, result) = sliced(&mut machine, &program, &mut Host::new(), 10);
    assert_eq!(result, Value::Int(1999), "the last element and the length");
    assert!(
        spends.iter().sum::<u64>() >= 1000,
        "the whole body spent {spends:?}"
    );

    assert!(
        program.max_overdraft() >= 1000,
        "the overdraft bound ({}) does not count the literal",
        program.max_overdraft()
    );
}

// ─── Copying ────────────────────────────────────────────────────────────────

/// **A copy costs what it copies.** One more `int[] c = a;` of a 1 000-element
/// array costs at least a thousand more fuel; of a two-element array, a
/// handful.
#[test]
fn copy_cost_scales_with_size() {
    let program = |count: usize, copies: usize| {
        let extra: String = (0..copies).map(|n| format!("int[] c{n} = a; ")).collect();
        build(&format!(
            "fn int F() {{ int[] a = [{}]; {extra} return a.Length; }}",
            ints(count)
        ))
    };

    let big_one = cost_of(&program(1000, 1), "F");
    let big_two = cost_of(&program(1000, 2), "F");
    let small_one = cost_of(&program(2, 1), "F");
    let small_two = cost_of(&program(2, 2), "F");

    assert!(
        big_two >= big_one + 1000,
        "a copy of 1 000 elements cost {} fuel",
        big_two.saturating_sub(big_one)
    );
    assert!(
        small_two <= small_one + 20,
        "a copy of 2 elements cost {} fuel",
        small_two.saturating_sub(small_one)
    );
}

/// **A copy the run cannot pay for waits for the next run.** The run that
/// starts after the literal has five fuel and a thousand-element copy ahead
/// of it: it stops before the copy, having spent no more than its five,
/// instead of running the copy on credit. The next run starts with the copy and
/// pays for it, and the body completes.
#[test]
fn a_copy_waits_for_fuel_it_can_pay_for() {
    let program = build(&format!(
        "fn int F() {{
             int[] a = [{}];
             int n = 1;
             int[] b = a;
             return b.Length + n;
         }}",
        ints(1000)
    ));

    let mut machine = Machine::new(&program, "F", &[]).expect("the function exists");
    let mut host = Host::new();
    let (first, _) = machine.run_counting(&program, &mut host, 5);
    assert_eq!(
        first,
        Run::Suspended(Suspension::OutOfFuel),
        "the premise: the literal's run stops after it"
    );
    host.end_frame();

    let (second, spent) = machine.run_counting(&program, &mut host, 5);
    assert_eq!(second, Run::Suspended(Suspension::OutOfFuel));
    assert!(
        spent <= 5,
        "the run with the copy ahead spent {spent} of its 5: the copy ran on credit"
    );
    host.end_frame();

    let (spends, result) = sliced(&mut machine, &program, &mut host, 5);
    assert_eq!(result, Value::Int(1001), "after {spends:?}");
}

/// **A field load is a copy too.** Reading a thousand-element field into a
/// local, with four fuel left in the run, waits for the next run; that run
/// starts with the load and pays for it whole.
#[test]
fn loading_a_big_field_waits_for_fuel_it_can_pay_for() {
    let program = build(&format!(
        "behavior Guard {{
             int[] big = [{}];
             int Count() {{
                 int n = 1;
                 int[] mine = big;
                 return mine.Length + n;
             }}
         }}",
        ints(1000)
    ));
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(&program, &init_name("Guard"), &[]).expect("an initialiser");
    assert_eq!(
        spend_whole(&mut init, &program, &mut host).0,
        Run::Completed
    );
    host.end_frame();

    let count = resolve_member(&program, "Guard", "Count", &host).expect("`Count`");
    let mut machine = Machine::new(&program, &count, &[]).expect("the member exists");
    let (first, spent) = machine.run_counting(&program, &mut host, 5);
    assert_eq!(first, Run::Suspended(Suspension::OutOfFuel));
    assert!(
        spent <= 5,
        "the run with the load ahead spent {spent} of its 5: the load ran on credit"
    );
    host.end_frame();

    let (second, spent) = machine.run_counting(&program, &mut host, 5);
    assert!(
        spent >= 1000,
        "the run starting at the load pays for it whole: spent {spent} ({second:?})"
    );
    let (spends, result) = sliced(&mut machine, &program, &mut host, 5);
    assert_eq!(result, Value::Int(1001), "after {spends:?}");
}
