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

//! Fuel runs out only at a safepoint.
//!
//! A function's entry, a statement's start, a loop's head, and the return of a
//! call are the only places a run may stop for fuel. Between two of them it
//! runs and charges, past its budget if it must; that overdraft is bounded by
//! the program's costliest stretch, so the DCC's budget still holds.

use khora_core::script::PendingBody;
use khora_script::vm::{SiteKind, Suspension};
use khora_script::{check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Value};

/// A call costing far more than an instruction, so a stretch holding it is
/// the program's costliest.
#[ergon_fn(name = "SafepointWall", cost = 40)]
fn safepoint_wall() -> i64 {
    1
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

/// Loops, calls, long statements, and a native in the middle of one.
const BUSY: &str = "fn int Sum(int a, int b) { int s = a + b; return s; }
                    fn int F() {
                        int total = 0;
                        for (int i = 0; i < 4; i = i + 1) {
                            total = total + Sum(i, i * 2) * 3 - Sum(1, 2) + SafepointWall();
                        }
                        int k = 0;
                        while (k < 3) {
                            k = k + 1;
                            total = total + k * k + k;
                        }
                        return total;
                    }";

/// What `F` returns, and what it costs, run in one go.
fn uninterrupted(program: &Program) -> (Value, u64) {
    let mut machine = Machine::new(program, "F", &[]).expect("F exists");
    let (run, cost) = machine.run_counting(program, &mut Host::new(), u64::MAX);
    assert_eq!(run, Run::Completed);
    (machine.result(), cost)
}

/// **Where a run stops for fuel.** For every slice from one to the whole
/// run's cost, every suspension leaves the innermost frame at a site of its
/// function that is an entry, a statement's start, a loop's head or a call's
/// return — never mid-expression — and the result is the uninterrupted one.
#[test]
fn fuel_runs_out_only_at_safepoints() {
    let program = build(BUSY);
    let (expected, cost) = uninterrupted(&program);

    for slice in 1..=cost {
        let mut machine = Machine::new(&program, "F", &[]).expect("F exists");
        let mut host = Host::new();
        let mut cuts = 0;
        loop {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    cuts += 1;
                    assert!(cuts < 10_000, "slice {slice} is not making progress");

                    let frozen = machine
                        .freeze(&program, PendingBody::Update)
                        .unwrap_or_else(|| panic!("slice {slice}: cut {cuts} did not freeze"));
                    let innermost = frozen.frames.last().expect("a frame");
                    let function = program
                        .function(&innermost.function)
                        .expect("the frame names a function of the program");
                    let pc = machine.program_counter();
                    let site = function.sites.iter().find(|site| site.pc as usize == pc);
                    assert!(
                        site.is_some_and(|site| matches!(
                            site.kind,
                            SiteKind::Entry
                                | SiteKind::Statement
                                | SiteKind::LoopHead
                                | SiteKind::Return { .. }
                        )),
                        "slice {slice}, cut {cuts}: `{}` stopped at pc {pc}, which is no \
                         safepoint: {:?}",
                        function.name,
                        function.sites
                    );
                }
                other => panic!("slice {slice}: unexpected {other:?}"),
            }
        }
        assert_eq!(machine.result(), expected, "a slice of {slice} diverged");
    }
}

/// **The DCC stays bounded.** However finely the run is sliced, no run spends
/// more than its fuel plus the program's costliest stretch between two
/// safepoints — and that stretch counts the native at its declared cost.
#[test]
fn a_run_never_overdraws_more_than_the_programs_longest_stretch() {
    let program = build(BUSY);
    let (_, cost) = uninterrupted(&program);
    let longest = program.max_overdraft();
    assert!(
        longest >= 40,
        "the stretch holding the native costs at least the native: {longest}"
    );
    assert!(
        longest < cost,
        "a stretch is acyclic, so it is not the whole looping run: {longest} of {cost}"
    );

    for slice in 1..=cost {
        let mut machine = Machine::new(&program, "F", &[]).expect("F exists");
        let mut host = Host::new();
        let mut total = 0;
        loop {
            let (run, spent) = machine.run_counting(&program, &mut host, slice);
            total += spent;
            assert!(
                spent <= slice + longest,
                "a slice of {slice} spent {spent}, past its fuel plus the longest stretch \
                 ({longest})"
            );
            match run {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {}
                other => panic!("slice {slice}: unexpected {other:?}"),
            }
        }
        assert_eq!(total, cost, "a slice of {slice} changed what the run costs");
    }
}

/// A run handed fuel makes progress even when its first stretch costs more
/// than all of it: the stretch is paid as an overdraft, and the run stops at
/// the next safepoint rather than at the place it started.
#[test]
fn a_slice_smaller_than_a_stretch_still_makes_progress() {
    let program = build(
        "fn int F() {
             int a = SafepointWall();
             int b = SafepointWall();
             return a + b;
         }",
    );
    let mut machine = Machine::new(&program, "F", &[]).expect("F exists");
    let mut host = Host::new();

    let (run, spent) = machine.run_counting(&program, &mut host, 1);
    assert_eq!(run, Run::Suspended(Suspension::OutOfFuel));
    assert!(spent >= 40, "the first stretch was paid in full: {spent}");
    let function = program.function("F").expect("F exists");
    let at = function
        .sites
        .iter()
        .find(|site| site.pc as usize == machine.program_counter());
    assert!(
        at.is_some_and(|site| site.kind == SiteKind::Statement),
        "stopped at the second statement's start: {:?}",
        function.sites
    );

    let mut rounds = 0;
    while machine.run(&program, &mut host, 1) != Run::Completed {
        rounds += 1;
        assert!(rounds < 10, "each run makes a statement's progress");
    }
    assert_eq!(machine.result(), Value::Int(2));
}

/// **A loop nobody compiled still stops.** A hand-built program has no
/// `Safepoint` at its loop's head, and a deserialized one may not either; a
/// backward jump is a place the machine checks its fuel all the same, so a
/// loop of a million turns handed a hundred units of fuel stops within one
/// turn of it rather than running to the end.
#[test]
fn a_hand_built_loop_stops_at_its_backward_jump() {
    use khora_script::{Function, Instruction};

    // r0 = 0; r1 = limit; r2 = 1; loop: r3 = r0 < r1; if !r3 goto end;
    // r0 += r2; goto loop; end: return r0
    let program = Program {
        functions: vec![Function {
            name: "Count".to_owned(),
            arity: 0,
            registers: 4,
            code: vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(0),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Int(1_000_000),
                },
                Instruction::LoadConst {
                    dst: 2,
                    value: Value::Int(1),
                },
                Instruction::Less {
                    dst: 3,
                    lhs: 0,
                    rhs: 1,
                },
                Instruction::JumpIfNot { cond: 3, target: 7 },
                Instruction::AddInt {
                    dst: 0,
                    lhs: 0,
                    rhs: 2,
                },
                Instruction::Jump { target: 3 },
                Instruction::Return { src: 0 },
            ],
            fingerprint: 0,
            sites: Vec::new(),
        }],
        ..Program::default()
    };

    let mut machine = Machine::new(&program, "Count", &[]).expect("Count exists");
    let (run, spent) = machine.run_counting(&program, &mut Host::new(), 100);

    assert_eq!(run, Run::Suspended(Suspension::OutOfFuel));
    assert!(
        spent <= 100 + 4,
        "stopped within one turn of the loop past its fuel: {spent}"
    );
    assert_eq!(
        machine.program_counter(),
        3,
        "at the loop's head, where the jump landed"
    );
}
