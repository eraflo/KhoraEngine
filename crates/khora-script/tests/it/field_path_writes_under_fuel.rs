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

//! A write through a path rooted at a behavior field never puts back a stale
//! copy of the field — whatever slice of fuel the body runs in.
//!
//! `xs[i] = 7` on a field loads the field (a copy), writes the element into
//! the copy, and stores the copy back. If the body can stop between the load
//! and the store — the store of an array is a sized operation, which waits for
//! fuel it can pay for — another body may write the field meanwhile, and the
//! store then erases that write with the copy loaded before it.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
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

/// Runs to the end with all the fuel it wants, a fresh frame per suspension.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
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
    host.end_frame();
    host
}

fn machine_for(program: &Program, member: &str, args: &[Value], host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, args).expect("the member exists")
}

/// **Another body's write survives a path write cut for fuel.** `Put` writes
/// `xs[1]` of a fifty-element field, a slice of fuel at a time; whenever it
/// stops, `Poke` writes `xs[0]` in the next frame. Once `Put` is done, the
/// field holds both writes — at every slice. A cut between the field's load
/// and its store puts back the copy loaded before `Poke` ran, and `xs[0]` is
/// `0` again.
#[test]
fn a_field_written_while_a_path_write_waits_for_fuel_keeps_that_write() {
    let program = build(&format!(
        "behavior Guard {{
             int[] xs = [{}];
             void Put() {{ int i = 1; xs[i] = 7; }}
             void Poke() {{ xs[0] = 5; }}
             int At(int i) {{ return xs[i]; }}
         }}",
        ints(50)
    ));
    let mut wrong = Vec::new();
    for slice in 1..=120u64 {
        let mut host = instance(&program);
        let mut set = machine_for(&program, "Put", &[], &host);
        let mut cut = false;
        loop {
            match set.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(_) => {
                    cut = true;
                    host.end_frame();
                    let mut poke = machine_for(&program, "Poke", &[], &host);
                    finish(&mut poke, &program, &mut host).expect("the other write runs");
                    host.end_frame();
                }
                Run::Faulted(fault) => panic!("slice {slice}: faulted: {fault:?}"),
            }
        }
        if !cut {
            continue;
        }
        host.end_frame();
        let read: Vec<_> = (0..2)
            .map(|i| {
                let mut at = machine_for(&program, "At", &[Value::Int(i)], &host);
                finish(&mut at, &program, &mut host)
            })
            .collect();
        if read != [Ok(Value::Int(5)), Ok(Value::Int(7))] {
            wrong.push(format!("slice {slice}: xs[0], xs[1] = {read:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the other body's write was erased:\n{}",
        wrong.join("\n")
    );
}
