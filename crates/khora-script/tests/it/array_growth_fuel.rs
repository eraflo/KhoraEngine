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

//! What growing and shrinking an array costs, and where a body doing it may
//! stop.
//!
//! `xs.RemoveAt(i)` shifts every element after `i` down by one: it is charged
//! that work — one, plus the elements it shifts — and, like a copy, a run
//! that cannot pay for it stops before it (unless it is the run's first
//! step), at a named site. `xs.Push(v)` costs one, plus the copy of an object
//! it is given. On a behavior field, the field is loaded, changed and written
//! back with no stop in between, so a write another body makes while this
//! one waits for fuel is never erased.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{resume, Fault, Machine, Program, ResumeTier, Run, Suspension, Value};
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

/// The fuel `function` spends, run whole.
fn cost_of(program: &Program, function: &str) -> u64 {
    let mut machine = Machine::new(program, function, &[]).expect("the function exists");
    let (run, spent) = machine.run_counting(program, &mut Host::new(), u64::MAX);
    assert_eq!(run, Run::Completed, "`{function}` completes");
    spent
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

/// Runs `machine` `slice` fuel at a time, a fresh frame between runs, until it
/// completes; the result.
fn sliced(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    slice: u64,
) -> Result<Value, Fault> {
    for _ in 0..100_000 {
        match machine.run(program, host, slice) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end at slice {slice}");
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

fn call(program: &Program, member: &str, args: &[Value], host: &mut Host) -> Result<Value, Fault> {
    let mut machine = machine_for(program, member, args, host);
    let result = finish(&mut machine, program, host);
    host.end_frame();
    result
}

// ─── Costs ──────────────────────────────────────────────────────────────────

/// **`RemoveAt` costs what it shifts.** From a thousand elements, removing
/// the first shifts 999 of them and costs 999 more than removing the last,
/// which shifts none; removing the middle one costs in between, by the
/// elements it shifts. Removing the last costs a handful.
#[test]
fn remove_at_costs_what_it_shifts() {
    let program = |statement: &str| {
        build(&format!(
            "fn int F() {{ int[] a = [{}]; {statement} return a.Length; }}",
            ints(1000)
        ))
    };
    let none = cost_of(&program("int k = 999;"), "F");
    let front = cost_of(&program("a.RemoveAt(0);"), "F");
    let middle = cost_of(&program("a.RemoveAt(500);"), "F");
    let back = cost_of(&program("a.RemoveAt(999);"), "F");

    assert!(
        front >= back + 999 && front <= back + 1010,
        "removing the first of 1 000 cost {} more than removing the last",
        front.saturating_sub(back)
    );
    assert!(
        middle >= back + 499 && middle <= back + 510,
        "removing the middle of 1 000 cost {} more than removing the last",
        middle.saturating_sub(back)
    );
    assert!(
        back <= none + 5,
        "removing the last element cost {} more than not removing it",
        back.saturating_sub(none)
    );
}

/// **`Push` costs one, plus the copy of an object it is given.** Pushing an
/// int onto a thousand-element array costs a handful — the array is not
/// copied. Pushing a thousand-element array onto another costs at least the
/// thousand elements copied; a two-element one, a handful.
#[test]
fn push_costs_one_and_the_copy_of_what_it_is_given() {
    let scalar = |statement: &str| {
        build(&format!(
            "fn int F() {{ int[] a = [{}]; {statement} return a.Length; }}",
            ints(1000)
        ))
    };
    let without = cost_of(&scalar("int k = 1;"), "F");
    let with = cost_of(&scalar("a.Push(1);"), "F");
    assert!(
        with <= without + 5,
        "pushing an int onto 1 000 elements cost {} more",
        with.saturating_sub(without)
    );

    let rows = |count: usize, statement: &str| {
        build(&format!(
            "fn int F() {{ int[] row = [{}]; int[][] grid = []; {statement} return grid.Length; }}",
            ints(count)
        ))
    };
    let big_without = cost_of(&rows(1000, "int k = 1;"), "F");
    let big_with = cost_of(&rows(1000, "grid.Push(row);"), "F");
    let small_without = cost_of(&rows(2, "int k = 1;"), "F");
    let small_with = cost_of(&rows(2, "grid.Push(row);"), "F");
    assert!(
        big_with >= big_without + 1000,
        "pushing a copy of 1 000 elements cost {} more",
        big_with.saturating_sub(big_without)
    );
    assert!(
        small_with <= small_without + 10,
        "pushing a copy of 2 elements cost {} more",
        small_with.saturating_sub(small_without)
    );
}

// ─── Stopping before a removal ──────────────────────────────────────────────

/// A thousand elements, a statement to spend the next run's first fuel on,
/// then a removal that shifts 999.
fn removal(n: i64) -> String {
    format!(
        "fn int F() {{
             int[] a = [{}];
             int n = 1;
             a.RemoveAt(0);
             return a.Length + n * {n};
         }}",
        ints(1000)
    )
}

/// **A removal the run cannot pay for waits for the next run, at a named
/// site.** The run that starts after the literal has five fuel and a removal
/// shifting 999 elements ahead of it: it stops before the removal, having
/// spent no more than its five. The frame stopped there names a `remove`
/// site, and an edit after it rebuilds the frame there — the removal then
/// runs once. Run to the end, the next run starts with the removal and pays
/// for it.
#[test]
fn a_remove_at_waits_for_fuel_it_can_pay_for() {
    let program = build(&removal(1));
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
        "the run with the removal ahead spent {spent} of its 5: the removal ran on credit"
    );
    host.end_frame();

    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    let site = frozen.frames.last().expect("a frame").site.clone();
    assert!(
        site.ends_with(":remove"),
        "the stop before the removal names its site: `{site}`"
    );

    let edited = build(&removal(10));
    let (mut resumed, tier) = resume(&frozen, program.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "rebuilt at `{site}`");
    assert_eq!(
        finish(&mut resumed, &edited, &mut Host::new()),
        Ok(Value::Int(1009)),
        "the rebuilt frame removes one element"
    );

    let (third, spent) = machine.run_counting(&program, &mut host, 5);
    assert!(
        spent >= 1000,
        "the run starting at the removal pays for it whole: spent {spent} ({third:?})"
    );
    assert_eq!(
        sliced(&mut machine, &program, &mut host, 5),
        Ok(Value::Int(1000))
    );
}

// ─── Fields under fuel ──────────────────────────────────────────────────────

/// A member, the field's length after it, and `(index, value)` pairs the
/// field must hold then.
type Expected = (&'static str, i64, [(i64, i64); 3]);

/// **Another body's write survives a push or a removal on a field cut for
/// fuel.** `Add` pushes onto a fifty-element field and `Drop` removes its
/// second element, a slice of fuel at a time; whenever one stops, `Poke`
/// writes `xs[0]` in the next frame. Once it is done, the field holds both
/// changes — at every slice. A stop between the field's load and its write
/// back puts back the copy loaded before `Poke` ran.
#[test]
fn a_field_written_while_a_push_or_a_removal_waits_keeps_that_write() {
    let program = build(&format!(
        "behavior Guard {{
             int[] xs = [{}];
             void Add() {{ int v = 7; xs.Push(v); }}
             void Drop() {{ int i = 1; xs.RemoveAt(i); }}
             void Poke() {{ xs[0] = 5; }}
             int Count() {{ return xs.Length; }}
             int At(int i) {{ return xs[i]; }}
         }}",
        ints(50)
    ));
    let cases: [Expected; 2] = [
        ("Add", 51, [(0, 5), (1, 1), (50, 7)]),
        ("Drop", 49, [(0, 5), (1, 2), (48, 49)]),
    ];
    let mut wrong = Vec::new();
    for (member, length, held) in cases {
        for slice in 1..=120u64 {
            let mut host = instance(&program);
            let mut body = machine_for(&program, member, &[], &host);
            let mut cut = false;
            loop {
                match body.run(&program, &mut host, slice) {
                    Run::Completed => break,
                    Run::Suspended(_) => {
                        cut = true;
                        host.end_frame();
                        call(&program, "Poke", &[], &mut host).expect("the other write runs");
                    }
                    Run::Faulted(fault) => panic!("`{member}` slice {slice}: {fault:?}"),
                }
            }
            if !cut {
                continue;
            }
            host.end_frame();
            let count = call(&program, "Count", &[], &mut host);
            let read: Vec<_> = held
                .iter()
                .map(|(i, _)| call(&program, "At", &[Value::Int(*i)], &mut host))
                .collect();
            let want: Vec<_> = held.iter().map(|(_, v)| Ok(Value::Int(*v))).collect();
            if count != Ok(Value::Int(length)) || read != want {
                wrong.push(format!(
                    "`{member}` slice {slice}: length {count:?}, elements {read:?}"
                ));
                break;
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "the other body's write was erased:\n{}",
        wrong.join("\n")
    );
}

// ─── Every slice ────────────────────────────────────────────────────────────

/// Pushes and removals on a local, an element, and a copy of the local pushed
/// onto the grid — then the same on fields.
const WORK: &str = "behavior Guard {
                        int[] fs = [FIELD];
                        int[][] fgrid = [[1]];
                        int Local() {
                            int[] xs = [FIELD];
                            xs.Push(1);
                            xs.RemoveAt(0);
                            int[][] grid = [[1]];
                            grid[0].Push(2);
                            grid.Push(xs);
                            grid[1].RemoveAt(3);
                            return xs.Length * 1000000 + grid[1].Length * 10000
                                + grid[1][3] * 100 + xs[3] * 10 + grid[0][1];
                        }
                        void Work() {
                            fs.Push(1);
                            fs.RemoveAt(0);
                            fgrid[0].Push(2);
                            fgrid.Push(fs);
                            fgrid[1].RemoveAt(3);
                        }
                        int Fields() {
                            return fs.Length * 1000000 + fgrid[1].Length * 10000
                                + fgrid[1][3] * 100 + fs[3] * 10 + fgrid[0][1];
                        }
                    }";

/// `[0, …, 29]`, pushed `1`, its first removed: thirty elements, `xs[3]` is
/// `4`. The grid's copy of it, its fourth removed: twenty-nine, `[3]` is `5`.
const WORKED: i64 = 30_290_542;

/// **At every slice from 1 to 60, every stop names a site and the body
/// computes what it computes run whole** — on locals, and on fields (read
/// back in a later frame).
#[test]
fn pushes_and_removals_survive_every_fuel_slice_and_name_every_stop() {
    let program = build(&WORK.replace("FIELD", &ints(30)));
    let mut wrong = Vec::new();
    for member in ["Local", "Work"] {
        for slice in 1..=60u64 {
            let mut host = instance(&program);
            let mut machine = machine_for(&program, member, &[], &host);
            let mut result = None;
            for stop in 1..100_000 {
                match machine.run(&program, &mut host, slice) {
                    Run::Completed => {
                        result = Some(machine.result());
                        break;
                    }
                    Run::Suspended(_) => {
                        let frozen = machine
                            .freeze(&program, PendingBody::Sequence)
                            .expect("a machine of this program freezes");
                        let site = &frozen.frames.last().expect("a frame").site;
                        if site.is_empty() {
                            wrong.push(format!(
                                "`{member}` slice {slice} stop {stop}: at pc {} names no site",
                                frozen.program_counter
                            ));
                        }
                        host.awaiting = None;
                        host.end_frame();
                    }
                    Run::Faulted(fault) => {
                        wrong.push(format!("`{member}` slice {slice}: faulted {fault:?}"));
                        break;
                    }
                }
            }
            let got = match member {
                "Local" => result,
                _ => {
                    host.end_frame();
                    call(&program, "Fields", &[], &mut host).ok()
                }
            };
            if got != Some(Value::Int(WORKED)) {
                wrong.push(format!("`{member}` slice {slice}: {got:?}"));
            }
        }
    }
    wrong.dedup();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
