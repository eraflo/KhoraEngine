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

//! Every place a body entering a state with an array stops at is a named
//! site.
//!
//! `become S(xs)` exports its array argument, and a state entered with an
//! array default stores it: both are charged their size, and a body short of
//! fuel may stop before or around them. A save taken at a stop writes down the
//! site the frame stands at; an edit of the code then finds the site again and
//! rebuilds the frame. A stop with no site cannot be rebuilt — the body is run
//! again from its entry, and whatever it did before the stop happens twice.
//! Whether the entry itself is a place to stop is the VM's choice; a stop
//! with no name is never acceptable.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{resume, Fault, Machine, Program, ResumeTier, Run, Value};
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

fn machine_for(program: &Program, member: &str, host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, &[]).expect("the member exists")
}

/// A guard entering a state with an array: as an argument, and as the
/// state's declared default.
fn guard(n: i64) -> String {
    format!(
        "behavior Guard {{
             state Patrol {{
                 void Spot() {{ int[] route = [{route}]; int n = {n}; become Chase(route); }}
                 void Ready() {{ int n = {n}; become Wait(); }}
             }}
             state Chase(int[] path) {{ int First() {{ return path.Length; }} }}
             state Wait {{ int[] path = [{route}]; int First() {{ return path.Length; }} }}
         }}",
        route = ints(500)
    )
}

/// Runs `member` of a fresh instance `slice` fuel at a time and stops it at
/// its `stop`-th suspension: the machine and its host there, or `None` when it
/// completes first.
fn stopped_at(program: &Program, member: &str, slice: u64, stop: usize) -> Option<(Machine, Host)> {
    let mut host = instance(program);
    let mut machine = machine_for(program, member, &host);
    for seen in 1..=stop {
        match machine.run(program, &mut host, slice) {
            Run::Completed => return None,
            Run::Suspended(_) if seen == stop => return Some((machine, host)),
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
            Run::Faulted(fault) => panic!("`{member}` faulted at slice {slice}: {fault:?}"),
        }
    }
    None
}

/// **Every stop around entering a state with an array names its site, and a
/// stop at the entry rebuilds there.** `Spot` (`become Chase(route)`) and
/// `Ready` (a state whose default is an array) run at every slice from 1 to
/// 40 and a few larger ones; each stop is saved. Every saved frame names its
/// site. A stop inside the `become` statement is resumed in code whose
/// `int n` changed: the frame is rebuilt at that site — never restarted for
/// want of one — and the state then holds its 500 elements. A body that never
/// stops at the entry satisfies this trivially.
#[test]
fn a_body_stopped_while_entering_a_state_with_an_array_names_its_site() {
    let original = build(&guard(1));
    let edited = build(&guard(2));
    let mut wrong = Vec::new();
    for member in ["Spot", "Ready"] {
        for slice in (1..=40u64).chain([100, 400, 600]) {
            for stop in 1.. {
                let Some((machine, mut host)) = stopped_at(&original, member, slice, stop) else {
                    break;
                };
                let frozen = machine
                    .freeze(&original, PendingBody::Sequence)
                    .expect("a machine of this program freezes");
                let site = frozen.frames.last().expect("a frame").site.clone();
                if site.is_empty() {
                    wrong.push(format!(
                        "`{member}` slice {slice} stop {stop}: at pc {} names no site",
                        frozen.program_counter
                    ));
                    continue;
                }
                if !site.starts_with("become.") {
                    continue;
                }
                match resume(&frozen, original.fingerprint(), &edited) {
                    Ok((mut resumed, tier)) => {
                        if tier != ResumeTier::Rebuilt {
                            wrong.push(format!(
                                "`{member}` slice {slice} stop {stop} at `{site}`: resumed {tier:?}, not rebuilt"
                            ));
                        }
                        host.awaiting = None;
                        host.end_frame();
                        finish(&mut resumed, &edited, &mut host).expect("the body finishes");
                        host.end_frame();
                        let mut first = machine_for(&edited, "First", &host);
                        let length = finish(&mut first, &edited, &mut host);
                        if length != Ok(Value::Int(500)) {
                            wrong.push(format!(
                                "`{member}` slice {slice} stop {stop}: the state holds {length:?} elements"
                            ));
                        }
                    }
                    Err(abandoned) => wrong.push(format!(
                        "`{member}` slice {slice} stop {stop} at `{site}`: abandoned {abandoned:?}"
                    )),
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{}",
        wrong.join(
            "
"
        )
    );
}
