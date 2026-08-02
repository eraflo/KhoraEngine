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

//! Virtual machine tests.
//!
//! The invariant everything else rests on has its own test: interrupting at
//! *every* instruction boundary must give the result of never interrupting.
//! Now that calls exist, it is checked across a call too — a suspension in the
//! middle of a call stack is exactly what a frame budget produces.

use super::*;
use crate::native::Host;

fn function(name: &str, arity: usize, registers: usize, code: Vec<Instruction>) -> Function {
    Function {
        name: name.to_owned(),
        arity,
        registers,
        code,
    }
}

/// ```text
/// r0 = 0; r1 = limit; r2 = 1
/// loop:  r3 = r0 < r1
///        if !r3 goto end
///        r0 = r0 + r2
///        goto loop
/// end:   return r0
/// ```
fn counting_program(limit: i64) -> Program {
    Program {
        functions: vec![function(
            "Count",
            0,
            4,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(0),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Int(limit),
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
        )],
    }
}

/// `Double(n) = n * 2`, and `Main() = Double(21)`.
fn calling_program() -> Program {
    Program {
        functions: vec![
            function(
                "Main",
                0,
                3,
                vec![
                    Instruction::LoadConst {
                        dst: 1,
                        value: Value::Int(21),
                    },
                    Instruction::Call {
                        function: 1,
                        base: 1,
                        argc: 1,
                        dst: 0,
                    },
                    Instruction::Return { src: 0 },
                ],
            ),
            function(
                "Double",
                1,
                3,
                vec![
                    Instruction::LoadConst {
                        dst: 1,
                        value: Value::Int(2),
                    },
                    Instruction::MulInt {
                        dst: 2,
                        lhs: 0,
                        rhs: 1,
                    },
                    Instruction::Return { src: 2 },
                ],
            ),
        ],
    }
}

fn run_to_completion(program: &Program, entry: &str) -> Value {
    let mut machine = Machine::new(program, entry, &[]).expect("the entry point exists");
    assert_eq!(
        machine.run(program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    machine.result()
}

#[test]
fn counts_to_the_limit() {
    assert_eq!(
        run_to_completion(&counting_program(10), "Count"),
        Value::Int(10)
    );
}

/// **The invariant of the whole design.** For every fuel allowance from one
/// upwards, running in repeated slices must land exactly where running in one
/// go does.
///
/// A slice of one interrupts at every instruction boundary the program has, so
/// any state the machine forgot to carry shows up here.
#[test]
fn interrupting_anywhere_changes_nothing() {
    let program = counting_program(10);
    let expected = run_to_completion(&program, "Count");

    for slice in 1..=40u64 {
        let mut machine = Machine::new(&program, "Count", &[]).expect("entry exists");
        // One host for the whole run, not one per slice: the host holds the
        // frame's arena and queued effects, and a fresh one between two pieces
        // of the same execution would discard what the first piece produced.
        let mut host = Host::new();
        let mut rounds = 0;

        loop {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    rounds += 1;
                    assert!(rounds < 1000, "slice {slice} is not making progress");
                }
                other => panic!("slice {slice}: unexpected {other:?}"),
            }
        }

        assert_eq!(
            machine.result(),
            expected,
            "a slice of {slice} diverged from the uninterrupted run"
        );
    }
}

/// The same invariant across a call boundary. A frame budget routinely runs out
/// mid-call, so the frame stack has to survive it.
#[test]
fn interrupting_inside_a_call_changes_nothing() {
    let program = calling_program();
    let expected = run_to_completion(&program, "Main");
    assert_eq!(expected, Value::Int(42));

    for slice in 1..=12u64 {
        let mut machine = Machine::new(&program, "Main", &[]).expect("entry exists");
        let mut host = Host::new();
        while machine.run(&program, &mut host, slice) != Run::Completed {}
        assert_eq!(
            machine.result(),
            expected,
            "a slice of {slice} diverged across the call"
        );
    }
}

/// A suspended machine has to survive leaving the process — that is what makes
/// saving a scene mid-`await` possible, so it is proven rather than assumed.
#[test]
fn a_suspended_machine_survives_serialization() {
    let program = calling_program();

    let mut machine = Machine::new(&program, "Main", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), 2),
        Run::Suspended(Suspension::OutOfFuel),
        "two instructions is not enough to finish"
    );

    let json = serde_json::to_string(&machine).expect("machine serialises");
    let mut revived: Machine = serde_json::from_str(&json).expect("machine deserialises");
    assert_eq!(revived, machine, "the round trip must be lossless");

    let mut host = Host::new();
    while revived.run(&program, &mut host, 3) != Run::Completed {}
    assert_eq!(revived.result(), Value::Int(42));
}

#[test]
fn arguments_arrive_in_the_first_registers() {
    let program = calling_program();
    let mut machine = Machine::new(&program, "Double", &[Value::Int(5)]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    assert_eq!(machine.result(), Value::Int(10));
}

/// Calling with the wrong number of arguments is a caller mistake, caught
/// before anything runs rather than becoming a fault mid-frame.
#[test]
fn the_wrong_argument_count_is_refused_up_front() {
    let program = calling_program();
    assert!(Machine::new(&program, "Double", &[]).is_none());
    assert!(Machine::new(&program, "Missing", &[]).is_none());
}

#[test]
fn float_arithmetic_works_and_stays_float() {
    let program = Program {
        functions: vec![function(
            "Half",
            0,
            3,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Float(3.0),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Float(2.0),
                },
                Instruction::DivFloat {
                    dst: 2,
                    lhs: 0,
                    rhs: 1,
                },
                Instruction::Return { src: 2 },
            ],
        )],
    };
    assert_eq!(run_to_completion(&program, "Half"), Value::Float(1.5));
}

/// Integer division by zero has no answer, so it faults. Float division does
/// not: IEEE gives an infinity, and gameplay dividing by a zero delta is better
/// served by a number that propagates than by a disabled behavior.
#[test]
fn integer_division_by_zero_faults_but_float_does_not() {
    let int_program = Program {
        functions: vec![function(
            "Bad",
            0,
            3,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(1),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Int(0),
                },
                Instruction::DivInt {
                    dst: 2,
                    lhs: 0,
                    rhs: 1,
                },
            ],
        )],
    };
    let mut machine = Machine::new(&int_program, "Bad", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&int_program, &mut Host::new(), u64::MAX),
        Run::Faulted(Fault::DivideByZero)
    );

    let float_program = Program {
        functions: vec![function(
            "Inf",
            0,
            3,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Float(1.0),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Float(0.0),
                },
                Instruction::DivFloat {
                    dst: 2,
                    lhs: 0,
                    rhs: 1,
                },
                Instruction::Return { src: 2 },
            ],
        )],
    };
    let Value::Float(result) = run_to_completion(&float_program, "Inf") else {
        panic!("expected a float");
    };
    assert!(result.is_infinite());
}

/// Integers compare as integers. Routing them through `f32` would lose
/// precision above 2^24, and a comparison that goes wrong on a large counter is
/// the kind of bug nobody thinks to look for.
#[test]
fn large_integers_compare_exactly() {
    let a = (1i64 << 53) + 1;
    let program = Program {
        functions: vec![function(
            "Compare",
            0,
            3,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(1i64 << 53),
                },
                Instruction::LoadConst {
                    dst: 1,
                    value: Value::Int(a),
                },
                Instruction::Less {
                    dst: 2,
                    lhs: 0,
                    rhs: 1,
                },
                Instruction::Return { src: 2 },
            ],
        )],
    };
    assert_eq!(
        run_to_completion(&program, "Compare"),
        Value::Bool(true),
        "as f32 both would round to the same value"
    );
}

/// Runaway recursion must stop at a limit rather than growing the register file
/// until the process dies: an engine crash becomes one disabled behavior.
#[test]
fn unbounded_recursion_faults_instead_of_exhausting_memory() {
    let program = Program {
        functions: vec![function(
            "Forever",
            0,
            2,
            vec![
                Instruction::Call {
                    function: 0,
                    base: 1,
                    argc: 0,
                    dst: 0,
                },
                Instruction::Return { src: 0 },
            ],
        )],
    };
    let mut machine = Machine::new(&program, "Forever", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Faulted(Fault::StackOverflow)
    );
}

#[test]
fn yield_suspends_once_and_moves_on() {
    let program = Program {
        functions: vec![function(
            "Pause",
            0,
            2,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(7),
                },
                Instruction::Yield,
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Int(9),
                },
                Instruction::Return { src: 0 },
            ],
        )],
    };

    let mut machine = Machine::new(&program, "Pause", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    assert_eq!(machine.register(0), Some(Value::Int(7)));

    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    assert_eq!(machine.result(), Value::Int(9));
}

/// Running off the end returns nothing rather than faulting: a compiler
/// emitting straight-line code should not need a trailing `Return`.
#[test]
fn falling_off_the_end_completes() {
    let program = Program {
        functions: vec![function(
            "Empty",
            0,
            1,
            vec![Instruction::LoadConst {
                dst: 0,
                value: Value::Int(1),
            }],
        )],
    };
    let mut machine = Machine::new(&program, "Empty", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    assert!(machine.is_finished());
}

/// A finished machine stays finished; resuming must not silently restart it.
#[test]
fn a_finished_machine_does_not_restart() {
    let program = counting_program(3);
    let mut machine = Machine::new(&program, "Count", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    assert_eq!(
        machine.run(&program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    assert_eq!(machine.result(), Value::Int(3), "not recomputed");
}

#[test]
fn faults_report_instead_of_panicking() {
    let bad_register = Program {
        functions: vec![function(
            "Bad",
            0,
            2,
            vec![Instruction::Move { dst: 0, src: 200 }],
        )],
    };
    let mut machine = Machine::new(&bad_register, "Bad", &[]).expect("entry exists");
    assert!(matches!(
        machine.run(&bad_register, &mut Host::new(), u64::MAX),
        Run::Faulted(Fault::BadRegister { .. })
    ));
    assert!(machine.is_finished(), "a faulted machine is not resumed");

    let bad_jump = Program {
        functions: vec![function(
            "Bad",
            0,
            1,
            vec![Instruction::Jump { target: 99 }],
        )],
    };
    let mut machine = Machine::new(&bad_jump, "Bad", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&bad_jump, &mut Host::new(), u64::MAX),
        Run::Faulted(Fault::BadJump { target: 99 })
    );

    let bad_type = Program {
        functions: vec![function(
            "Bad",
            0,
            2,
            vec![
                Instruction::LoadConst {
                    dst: 0,
                    value: Value::Bool(true),
                },
                Instruction::AddInt {
                    dst: 1,
                    lhs: 0,
                    rhs: 0,
                },
            ],
        )],
    };
    let mut machine = Machine::new(&bad_type, "Bad", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&bad_type, &mut Host::new(), u64::MAX),
        Run::Faulted(Fault::TypeMismatch {
            expected: "int",
            found: "bool",
        })
    );
}

/// Zero fuel makes no progress but does not fault — the DCC may legitimately
/// grant nothing this frame, and the script simply waits its turn.
#[test]
fn zero_fuel_suspends_without_progress() {
    let program = counting_program(5);
    let mut machine = Machine::new(&program, "Count", &[]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), 0),
        Run::Suspended(Suspension::OutOfFuel)
    );
    assert_eq!(machine.program_counter(), 0);
}

/// A callee must not see whatever the caller left in the registers it is about
/// to use, or a stale value would masquerade as an initialised local.
#[test]
fn a_callee_starts_with_clean_registers() {
    let program = Program {
        functions: vec![
            function(
                "Main",
                0,
                4,
                vec![
                    // Dirty the registers the callee's frame will occupy.
                    Instruction::LoadConst {
                        dst: 2,
                        value: Value::Int(999),
                    },
                    Instruction::LoadConst {
                        dst: 3,
                        value: Value::Int(999),
                    },
                    Instruction::Call {
                        function: 1,
                        base: 2,
                        argc: 0,
                        dst: 0,
                    },
                    Instruction::Return { src: 0 },
                ],
            ),
            // Returns its own r1, which it never wrote.
            function("Peek", 0, 2, vec![Instruction::Return { src: 1 }]),
        ],
    };
    assert_eq!(run_to_completion(&program, "Main"), Value::Unit);
}
