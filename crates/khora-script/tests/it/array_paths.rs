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

//! A write through a path lands once, in the array the path names when the
//! write happens.
//!
//! `xs[i] = F()` where `F` suspends: the right-hand side runs first, then the
//! path is walked and written — so nothing the write goes through was held
//! across the suspension. A path rooted at a behavior field is the field
//! loaded, written and stored back: loaded *before* the suspension, that store
//! would put back a stale copy and erase whatever another body wrote to the
//! field in the meantime.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::{Object, PersistentStore};
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

/// Puts other objects at the first indices of the arena.
fn occupy(host: &mut Host) {
    for n in 0..4 {
        host.arena
            .alloc(Object::Array(vec![Value::Int(-1); n + 1]))
            .expect("the arena has room");
        host.arena
            .alloc(Object::Str("other".to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other objects.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

/// Runs to the end, a frame per suspension.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => next_frame(host),
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

fn instance(program: &Program) -> Host {
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(program, &init_name("Guard"), &[]).expect("a field initialiser");
    finish(&mut init, program, &mut host).expect("the initialiser runs");
    host
}

fn machine_for(program: &Program, member: &str, args: &[Value], host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, args).expect("the member exists")
}

fn call(program: &Program, member: &str, args: &[Value], host: &mut Host) -> Result<Value, Fault> {
    let mut machine = machine_for(program, member, args, host);
    finish(&mut machine, program, host)
}

/// The guard's `xs`, element by element.
fn elements(program: &Program, host: &mut Host) -> Vec<Result<Value, Fault>> {
    (0..3)
        .map(|i| call(program, "At", &[Value::Int(i)], host))
        .collect()
}

const GUARD: &str = r#"behavior Guard {
                           int[] xs = [1, 2, 3];
                           async int Slow() { await 1.0s; return 7; }
                           async void Put() { int i = 1; xs[i] = Slow(); }
                           async void Bump() { xs[1] += Slow(); }
                           void Poke() { xs[0] = 5; }
                           int At(int i) { return xs[i]; }

                           async int Local() {
                               int[] ys = [1, 2, 3];
                               int i = 1;
                               ys[i] = Slow();
                               return ys[0] * 100 + ys[1] * 10 + ys[2];
                           }
                           async int LocalAdd() {
                               int[] ys = [1, 2, 3];
                               ys[1] += Slow();
                               return ys[0] * 100 + ys[1] * 10 + ys[2];
                           }
                           async int Nested() {
                               int[][] grid = [[1, 2], [3]];
                               grid[0][1] = Slow();
                               return grid[0][0] * 100 + grid[0][1] * 10 + grid[1][0];
                           }
                       }"#;

/// **The write lands once, in the array the program reads afterwards.** On a
/// local, a compound write, and a nested path — each with its right-hand side
/// suspended across a frame whose arena was emptied and refilled.
#[test]
fn a_write_through_a_path_lands_once() {
    let program = build(GUARD);
    let cases = [
        ("Local", Value::Int(173)),
        ("LocalAdd", Value::Int(193)),
        ("Nested", Value::Int(173)),
    ];
    let mut wrong = Vec::new();
    for (member, wanted) in cases {
        let mut host = instance(&program);
        let mut machine = machine_for(&program, member, &[], &host);
        assert_eq!(
            machine.run(&program, &mut host, u64::MAX),
            Run::Suspended(Suspension::Awaiting),
            "`{member}`: the premise, stopped inside the right-hand side"
        );
        next_frame(&mut host);
        let got = finish(&mut machine, &program, &mut host);
        if got != Ok(wanted) {
            wrong.push(format!("`{member}`: expected {wanted:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A path rooted at a field is walked when the write happens.** While
/// `xs[1] = Slow()` waits, another body writes `xs[0]`. When the first resumes
/// and writes `xs[1]`, both writes are in the field: the field was not loaded
/// before the wait and stored back stale after it.
#[test]
fn a_field_written_during_the_right_hand_side_keeps_that_write() {
    let program = build(GUARD);
    for (member, second) in [("Put", 7), ("Bump", 9)] {
        let mut host = instance(&program);
        let mut machine = machine_for(&program, member, &[], &host);
        assert_eq!(
            machine.run(&program, &mut host, u64::MAX),
            Run::Suspended(Suspension::Awaiting),
            "`{member}`: the premise, stopped inside the right-hand side"
        );
        next_frame(&mut host);
        call(&program, "Poke", &[], &mut host).expect("the other write runs");
        next_frame(&mut host);

        finish(&mut machine, &program, &mut host).expect("the write finishes");
        next_frame(&mut host);
        assert_eq!(
            elements(&program, &mut host),
            [Ok(Value::Int(5)), Ok(Value::Int(second)), Ok(Value::Int(3))],
            "`{member}`: the other body's write survives, and the path's lands"
        );
    }
}

/// Every fuel slice from 1 to 60, the arena emptied between slices: the local
/// writes land exactly as they do run whole.
#[test]
fn a_write_through_a_path_lands_once_at_every_fuel_slice() {
    let program = build(GUARD);
    let mut wrong = Vec::new();
    for (member, wanted) in [
        ("Local", Value::Int(173)),
        ("LocalAdd", Value::Int(193)),
        ("Nested", Value::Int(173)),
    ] {
        for slice in 1..=60u64 {
            let mut host = instance(&program);
            let mut machine = machine_for(&program, member, &[], &host);
            let mut got = Err("never finished".to_owned());
            for _ in 0..100_000 {
                match machine.run(&program, &mut host, slice) {
                    Run::Suspended(_) => next_frame(&mut host),
                    Run::Completed => {
                        got = Ok(machine.result());
                        break;
                    }
                    Run::Faulted(fault) => {
                        got = Err(format!("{fault:?}"));
                        break;
                    }
                }
            }
            if got != Ok(wanted) {
                wrong.push(format!("`{member}` at slice {slice}: {got:?}"));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
