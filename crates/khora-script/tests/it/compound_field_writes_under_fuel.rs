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

//! A compound write through a path rooted at a behavior field — `xs[i] += v`
//! — reads the element, then walks the path again to write it. With nothing
//! between the two that can suspend (no call, no `await`), the statement must
//! not be cut between its read and its write: a cut lets another body write
//! the element, and the write then puts back a value computed from the old
//! one.

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

/// **A compound write to a field's element does not erase a write made while
/// it waited for fuel.** `Bump` is `xs[0] += 1` on a fifty-element field — no
/// call, nothing to await; `Poke` writes `xs[0] = 5` in the frame after each
/// stop. A scalar `n += 1` cannot be cut between its read and its write; this
/// one, when it is, must still land after `Poke` rather than over it: once
/// `Bump` is done, `xs[0]` is `6` (Poke then Bump) — never `1`, which is the
/// value `Bump` computed from the `0` it read before it stopped, written back
/// over Poke's `5`.
#[test]
fn a_compound_write_to_a_field_element_keeps_a_write_made_while_it_waited() {
    let program = build(&format!(
        "behavior Guard {{
             int[] xs = [{}];
             void Bump() {{ int k = 0; xs[0] += 1; }}
             void Poke() {{ xs[0] = 5; }}
             int At(int i) {{ return xs[i]; }}
         }}",
        ints(50)
    ));
    let mut wrong = Vec::new();
    for slice in 1..=200u64 {
        let mut host = instance(&program);
        let mut bump = machine_for(&program, "Bump", &[], &host);
        let mut poked = false;
        loop {
            match bump.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(_) => {
                    host.end_frame();
                    if !poked {
                        poked = true;
                        let mut poke = machine_for(&program, "Poke", &[], &host);
                        finish(&mut poke, &program, &mut host).expect("the other write runs");
                        host.end_frame();
                    }
                }
                Run::Faulted(fault) => panic!("slice {slice}: faulted: {fault:?}"),
            }
        }
        if !poked {
            continue;
        }
        host.end_frame();
        let mut at = machine_for(&program, "At", &[Value::Int(0)], &host);
        let read = finish(&mut at, &program, &mut host);
        if read != Ok(Value::Int(6)) {
            wrong.push(format!("slice {slice}: xs[0] = {read:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the other body's write was erased:\n{}",
        wrong.join("\n")
    );
}
