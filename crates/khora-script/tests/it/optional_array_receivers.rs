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

//! The receiver of `xs?.Push(v)` and `xs?.RemoveAt(i)`.
//!
//! `?.` tests the receiver for `null` and changes it when it is there. The
//! receiver is one place: its index operands are evaluated once, and the
//! array the call changes is never a `null` the test let through — whatever
//! another body did to the place while this one waited, or ran out of fuel.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Suspension, Value};
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

fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => next_frame(host),
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
}

fn instance(program: &Program) -> Host {
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name("Guard"), &[]).expect("a field initialiser");
    finish(init, program, &mut host).expect("the initialiser runs");
    host.end_frame();
    host
}

fn machine_for(program: &Program, member: &str, host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, &[]).expect("the member exists")
}

fn call(program: &Program, member: &str, host: &mut Host) -> Result<Value, Fault> {
    let machine = machine_for(program, member, host);
    let result = finish(machine, program, host);
    next_frame(host);
    result
}

/// **An element receiver is evaluated once.** `grid[Next()]?.Push(5)` with
/// `grid = [[1], null]`: `Next()` returns 0 the first time it is called, so
/// the element tested is `grid[0]`, present, and the push lands there.
/// `Next()` runs once — evaluating it again for the change picks `grid[1]`,
/// the `null` the test never saw.
#[test]
fn an_element_receiver_of_a_question_dot_is_evaluated_once() {
    let program = build(
        "behavior Guard {
             int calls = 0;
             int Next() { calls += 1; return calls - 1; }
             int Run() {
                 int[]?[] grid = [[1], null];
                 grid[Next()]?.Push(5);
                 int[] first = grid[0] ?? [];
                 return calls * 100 + first.Length;
             }
         }",
    );
    let mut host = instance(&program);
    assert_eq!(
        call(&program, "Run", &mut host),
        Ok(Value::Int(102)),
        "`Next()` runs once, and `grid[0]` gains the 5"
    );
}

const FIELD: &str = "behavior Guard {
                         int[]? xs = [1];
                         async int Slow() { await 1.0s; return 7; }
                         async void Add() { xs?.Push(Slow()); }
                         void Push() { xs?.Push(2); }
                         void Clear() { xs = null; }
                         int Count() { int[] ys = xs ?? [-1, -1, -1]; return ys.Length; }
                     }";

/// **A field another body empties while a `?.Push` waits is not pushed
/// to.** `xs?.Push(Slow())` tests `xs`, present, then waits inside its
/// argument; meanwhile another body sets `xs` to `null`. When the push
/// resumes there is no array to change: it must not fault — and the `null`
/// written meanwhile is kept, as every write made meanwhile is.
#[test]
fn a_field_emptied_while_a_question_dot_push_waits_is_not_pushed_to() {
    let program = build(FIELD);
    let mut host = instance(&program);
    let mut machine = machine_for(&program, "Add", &host);
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the premise: `Add` stops at the await in its argument"
    );
    next_frame(&mut host);
    call(&program, "Clear", &mut host).expect("the other write runs");
    let resumed = finish(machine, &program, &mut host);
    next_frame(&mut host);
    assert!(
        resumed.is_ok(),
        "the push resumed onto the null another body wrote: {resumed:?}"
    );
    assert_eq!(
        call(&program, "Count", &mut host),
        Ok(Value::Int(3)),
        "the field is still the null written meanwhile"
    );
}

/// **A field emptied while a `?.Push` is out of fuel is not pushed to.**
/// `xs?.Push(2)` with nothing to wait on: a slice that pays for the test's
/// load of `xs` but not for the second load the change makes stops between
/// them. Another body sets `xs` to `null`; the resumed push must not fault.
#[test]
fn a_field_emptied_while_a_question_dot_push_is_out_of_fuel_is_not_pushed_to() {
    let program = build(FIELD);
    let mut faults = Vec::new();
    let mut stopped_between = false;
    for slice in 1..12 {
        let mut host = instance(&program);
        let mut machine = machine_for(&program, "Push", &host);
        // One slice, then the field emptied, then the rest.
        match machine.run(&program, &mut host, slice) {
            Run::Completed => continue,
            Run::Suspended(_) => {}
            Run::Faulted(fault) => panic!("slice {slice}: faulted before the clear: {fault:?}"),
        }
        stopped_between = true;
        next_frame(&mut host);
        call(&program, "Clear", &mut host).expect("the other write runs");
        if let Err(fault) = finish(machine, &program, &mut host) {
            faults.push(format!("slice {slice}: {fault:?}"));
        }
    }
    assert!(stopped_between, "the premise: some slice stops the push");
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
