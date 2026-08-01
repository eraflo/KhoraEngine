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

//! The interruptible virtual machine.
//!
//! A **register machine**, not a stack machine. Fewer instructions per
//! operation and better locality are welcome, but the deciding factor is that
//! its state is trivial to freeze: a program counter and a flat register file.
//! A stack machine would need the operand stack captured mid-expression too,
//! and the whole design rests on being able to stop anywhere and continue.
//!
//! # The contract
//!
//! Running a program with a fuel budget either finishes it, or stops and hands
//! back a machine that can be resumed. **Resuming must be indistinguishable
//! from never having stopped** — that is the invariant the tests at the bottom
//! of this file exist to defend, and it is checked at every possible
//! interruption point, not at a sampled few.
//!
//! # Fuel
//!
//! Every instruction costs fuel. The DCC's frame budget becomes a fuel
//! allowance, so a script that would overrun its share simply stops mid-way and
//! resumes next frame. The observable behaviour never changes — only *when* the
//! work happens, which is exactly the engine's rule of adapting the HOW and
//! never the WHAT.

use serde::{Deserialize, Serialize};

/// A runtime value.
///
/// Deliberately narrow for now: the phase-0 prototype exists to prove
/// suspension, and a wider value set would not make that proof any stronger.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// 64-bit signed integer.
    Int(i64),
    /// 32-bit float — gameplay does not need more.
    Float(f32),
    /// Boolean.
    Bool(bool),
}

impl Value {
    /// The integer inside, or `None` if this is not an `Int`.
    fn as_int(self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(n),
            _ => None,
        }
    }

    /// The boolean inside, or `None` if this is not a `Bool`.
    fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// Short type name, for fault messages.
    fn type_name(self) -> &'static str {
        match self {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Bool(_) => "bool",
        }
    }
}

/// One instruction. Register operands are indices into the register file.
///
/// Kept to what the phase-0 proof needs — arithmetic, comparison, branching and
/// an explicit yield. The full instruction set arrives with the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instruction {
    /// `dst = value`
    LoadConst {
        /// Destination register.
        dst: u8,
        /// The constant to load.
        value: Value,
    },
    /// `dst = src`
    Move {
        /// Destination register.
        dst: u8,
        /// Source register.
        src: u8,
    },
    /// `dst = lhs + rhs`, integers.
    AddInt {
        /// Destination register.
        dst: u8,
        /// Left operand register.
        lhs: u8,
        /// Right operand register.
        rhs: u8,
    },
    /// `dst = lhs - rhs`, integers.
    SubInt {
        /// Destination register.
        dst: u8,
        /// Left operand register.
        lhs: u8,
        /// Right operand register.
        rhs: u8,
    },
    /// `dst = lhs < rhs`, integers.
    LessInt {
        /// Destination register.
        dst: u8,
        /// Left operand register.
        lhs: u8,
        /// Right operand register.
        rhs: u8,
    },
    /// Unconditional jump.
    Jump {
        /// Instruction index to continue at.
        target: usize,
    },
    /// Jump when `cond` holds `false`. The shape a compiled `while` needs.
    JumpIfNot {
        /// Register holding the condition.
        cond: u8,
        /// Instruction index to continue at.
        target: usize,
    },
    /// Suspend voluntarily. Stands in for `await` until the compiler exists.
    Yield,
    /// Stop. Running past the last instruction halts too; this makes it explicit.
    Halt,
}

// `Suspension`, `Fault` and `Run` are answers to "what happened during this
// call" — they are consumed immediately and never stored. Only [`Machine`]
// crosses a frame boundary or a scene save, so only it derives `Serialize`.
// Keeping the split visible stops the serialisable surface from creeping wider
// than it has to be.

/// Why a program stopped before finishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suspension {
    /// The fuel allowance ran out. Resume when the budget allows.
    OutOfFuel,
    /// The program yielded. Resume when whatever it waits on resolves.
    Awaiting,
}

/// A program that cannot continue. Faults never panic: a broken script must not
/// take the engine down with it, so the machine stops and reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// A register index outside the register file.
    BadRegister {
        /// The offending index.
        index: u8,
    },
    /// A jump outside the program.
    BadJump {
        /// The offending instruction index.
        target: usize,
    },
    /// An operand of the wrong type.
    TypeMismatch {
        /// What the instruction required.
        expected: &'static str,
        /// What it found.
        found: &'static str,
    },
}

/// The outcome of one [`Machine::run`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// The program reached its end or a `Halt`.
    Completed,
    /// The program stopped and can be resumed.
    Suspended(Suspension),
    /// The program is broken and will not be resumed.
    Faulted(Fault),
}

/// The machine: a register file and a program counter.
///
/// Everything that must survive an interruption lives here, and nothing else
/// does. That is what makes the state serialisable — and serialisable state is
/// what lets a scene be saved mid-`await`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    registers: Vec<Value>,
    /// Index of the *next* instruction to run. Suspending is nothing more than
    /// returning while this points at the instruction not yet executed.
    program_counter: usize,
    /// Set once the program has halted or faulted, so a resume cannot restart a
    /// finished program.
    finished: bool,
}

impl Machine {
    /// A machine with `registers` slots, all zeroed, ready to run from the top.
    pub fn new(registers: usize) -> Self {
        Self {
            registers: vec![Value::Int(0); registers],
            program_counter: 0,
            finished: false,
        }
    }

    /// Reads a register, for tests and for the engine to collect results.
    pub fn register(&self, index: u8) -> Option<Value> {
        self.registers.get(index as usize).copied()
    }

    /// Whether the program has run to completion (or faulted).
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The instruction that will run next. Useful for debugging a suspension.
    pub fn program_counter(&self) -> usize {
        self.program_counter
    }

    /// Runs `program` until it finishes, faults, yields, or burns `fuel`
    /// instructions.
    ///
    /// Call it again to resume — the machine holds everything needed and does
    /// not care how long it was left alone, or whether it was serialised in
    /// between.
    pub fn run(&mut self, program: &[Instruction], fuel: u64) -> Run {
        if self.finished {
            return Run::Completed;
        }

        let mut remaining = fuel;

        loop {
            // Falling off the end is a normal way to finish, not a fault: a
            // compiler emitting straight-line code should not need a trailing
            // `Halt` to be correct.
            let Some(instruction) = program.get(self.program_counter) else {
                self.finished = true;
                return Run::Completed;
            };

            // Checked before the instruction, so a suspension always lands on
            // an instruction boundary. Landing mid-instruction would mean
            // capturing partial results, which is precisely what the register
            // machine was chosen to avoid.
            if remaining == 0 {
                return Run::Suspended(Suspension::OutOfFuel);
            }
            remaining -= 1;

            match self.step(instruction, program.len()) {
                Ok(Step::Next) => self.program_counter += 1,
                Ok(Step::Jumped) => {}
                Ok(Step::Halt) => {
                    self.finished = true;
                    return Run::Completed;
                }
                Ok(Step::Yield) => {
                    // Past the yield, so resuming continues rather than
                    // yielding again forever.
                    self.program_counter += 1;
                    return Run::Suspended(Suspension::Awaiting);
                }
                Err(fault) => {
                    self.finished = true;
                    return Run::Faulted(fault);
                }
            }
        }
    }

    /// Executes one instruction and says how the program counter should move.
    fn step(&mut self, instruction: &Instruction, program_len: usize) -> Result<Step, Fault> {
        match *instruction {
            Instruction::LoadConst { dst, value } => {
                self.write(dst, value)?;
                Ok(Step::Next)
            }
            Instruction::Move { dst, src } => {
                let value = self.read(src)?;
                self.write(dst, value)?;
                Ok(Step::Next)
            }
            Instruction::AddInt { dst, lhs, rhs } => {
                let (a, b) = self.read_int_pair(lhs, rhs)?;
                // Wrapping rather than panicking: a script that overflows is
                // wrong, but it must not be able to abort the frame.
                self.write(dst, Value::Int(a.wrapping_add(b)))?;
                Ok(Step::Next)
            }
            Instruction::SubInt { dst, lhs, rhs } => {
                let (a, b) = self.read_int_pair(lhs, rhs)?;
                self.write(dst, Value::Int(a.wrapping_sub(b)))?;
                Ok(Step::Next)
            }
            Instruction::LessInt { dst, lhs, rhs } => {
                let (a, b) = self.read_int_pair(lhs, rhs)?;
                self.write(dst, Value::Bool(a < b))?;
                Ok(Step::Next)
            }
            Instruction::Jump { target } => {
                self.jump_to(target, program_len)?;
                Ok(Step::Jumped)
            }
            Instruction::JumpIfNot { cond, target } => {
                let value = self.read(cond)?;
                let taken = value.as_bool().ok_or(Fault::TypeMismatch {
                    expected: "bool",
                    found: value.type_name(),
                })?;
                if taken {
                    Ok(Step::Next)
                } else {
                    self.jump_to(target, program_len)?;
                    Ok(Step::Jumped)
                }
            }
            Instruction::Yield => Ok(Step::Yield),
            Instruction::Halt => Ok(Step::Halt),
        }
    }

    /// Moves the program counter, rejecting a target outside the program.
    ///
    /// `program_len` is a valid target: jumping one past the end is how a loop
    /// exits, and the next `run` iteration turns it into `Completed`.
    fn jump_to(&mut self, target: usize, program_len: usize) -> Result<(), Fault> {
        if target > program_len {
            return Err(Fault::BadJump { target });
        }
        self.program_counter = target;
        Ok(())
    }

    fn read(&self, index: u8) -> Result<Value, Fault> {
        self.registers
            .get(index as usize)
            .copied()
            .ok_or(Fault::BadRegister { index })
    }

    fn write(&mut self, index: u8, value: Value) -> Result<(), Fault> {
        let slot = self
            .registers
            .get_mut(index as usize)
            .ok_or(Fault::BadRegister { index })?;
        *slot = value;
        Ok(())
    }

    /// Reads two registers that both have to be integers — the shape every
    /// arithmetic instruction needs.
    fn read_int_pair(&self, lhs: u8, rhs: u8) -> Result<(i64, i64), Fault> {
        let a = self.read(lhs)?;
        let b = self.read(rhs)?;
        let a = a.as_int().ok_or(Fault::TypeMismatch {
            expected: "int",
            found: a.type_name(),
        })?;
        let b = b.as_int().ok_or(Fault::TypeMismatch {
            expected: "int",
            found: b.type_name(),
        })?;
        Ok((a, b))
    }
}

/// How the program counter moves after an instruction.
enum Step {
    /// Advance one.
    Next,
    /// The instruction already set it.
    Jumped,
    /// Stop for good.
    Halt,
    /// Stop, resumable.
    Yield,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registers: r0 = counter, r1 = limit, r2 = step, r3 = scratch.
    ///
    /// ```text
    /// r0 = 0; r1 = limit; r2 = 1
    /// loop:  r3 = r0 < r1
    ///        if !r3 goto end
    ///        r0 = r0 + r2
    ///        goto loop
    /// end:   halt
    /// ```
    fn counting_program(limit: i64) -> Vec<Instruction> {
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
            Instruction::LessInt {
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
            Instruction::Halt,
        ]
    }

    /// Runs to completion with unlimited fuel, returning the counter.
    fn run_uninterrupted(program: &[Instruction]) -> Value {
        let mut machine = Machine::new(4);
        assert_eq!(machine.run(program, u64::MAX), Run::Completed);
        machine.register(0).expect("r0 exists")
    }

    #[test]
    fn counts_to_the_limit() {
        assert_eq!(run_uninterrupted(&counting_program(10)), Value::Int(10));
    }

    /// **The invariant of the whole design.** For every fuel allowance from 1
    /// upwards, running the program in repeated slices must land on exactly the
    /// result of running it in one go.
    ///
    /// Not a sampled check: a slice size of 1 interrupts at *every* instruction
    /// boundary the program has, so any state the machine forgot to carry would
    /// show up here.
    #[test]
    fn interrupting_anywhere_changes_nothing() {
        let program = counting_program(10);
        let expected = run_uninterrupted(&program);

        for slice in 1..=40u64 {
            let mut machine = Machine::new(4);
            let mut rounds = 0;

            loop {
                match machine.run(&program, slice) {
                    Run::Completed => break,
                    Run::Suspended(Suspension::OutOfFuel) => {
                        rounds += 1;
                        assert!(rounds < 1000, "slice {slice} is not making progress");
                    }
                    other => panic!("slice {slice}: unexpected {other:?}"),
                }
            }

            assert_eq!(
                machine.register(0).expect("r0 exists"),
                expected,
                "slice of {slice} instructions diverged from the uninterrupted run"
            );
        }
    }

    /// A suspended machine has to survive leaving the process. This is what
    /// makes saving a scene mid-`await` possible, so it is proven now rather
    /// than assumed and discovered later.
    #[test]
    fn a_suspended_machine_survives_serialization() {
        let program = counting_program(10);

        let mut machine = Machine::new(4);
        assert_eq!(
            machine.run(&program, 5),
            Run::Suspended(Suspension::OutOfFuel),
            "5 instructions is not enough to finish"
        );

        let json = serde_json::to_string(&machine).expect("machine serialises");
        let mut revived: Machine = serde_json::from_str(&json).expect("machine deserialises");

        assert_eq!(revived, machine, "the round trip must be lossless");

        while revived.run(&program, 4) != Run::Completed {}
        assert_eq!(revived.register(0), Some(Value::Int(10)));
    }

    /// `Yield` stands in for `await`: it stops the machine and the next call
    /// carries on *past* it, rather than hitting the same yield forever.
    #[test]
    fn yield_suspends_once_and_moves_on() {
        let program = vec![
            Instruction::LoadConst {
                dst: 0,
                value: Value::Int(7),
            },
            Instruction::Yield,
            Instruction::LoadConst {
                dst: 1,
                value: Value::Int(9),
            },
        ];

        let mut machine = Machine::new(2);
        assert_eq!(
            machine.run(&program, u64::MAX),
            Run::Suspended(Suspension::Awaiting)
        );
        assert_eq!(machine.register(0), Some(Value::Int(7)));
        assert_eq!(
            machine.register(1),
            Some(Value::Int(0)),
            "execution stopped at the yield"
        );

        assert_eq!(machine.run(&program, u64::MAX), Run::Completed);
        assert_eq!(machine.register(1), Some(Value::Int(9)));
    }

    /// Running off the end finishes cleanly — a compiler emitting straight-line
    /// code should not need a trailing `Halt` to be correct.
    #[test]
    fn falling_off_the_end_completes() {
        let program = vec![Instruction::LoadConst {
            dst: 0,
            value: Value::Int(1),
        }];
        let mut machine = Machine::new(1);
        assert_eq!(machine.run(&program, u64::MAX), Run::Completed);
        assert!(machine.is_finished());
    }

    /// A finished machine stays finished. Resuming one must not silently
    /// restart the program from the top.
    #[test]
    fn a_finished_machine_does_not_restart() {
        let program = counting_program(3);
        let mut machine = Machine::new(4);
        assert_eq!(machine.run(&program, u64::MAX), Run::Completed);

        assert_eq!(machine.run(&program, u64::MAX), Run::Completed);
        assert_eq!(
            machine.register(0),
            Some(Value::Int(3)),
            "the counter must not have been recomputed"
        );
    }

    /// Faults stop the machine and report; they never panic. A broken script
    /// must not be able to take the frame down.
    #[test]
    fn a_bad_register_faults_without_panicking() {
        let program = vec![Instruction::LoadConst {
            dst: 9,
            value: Value::Int(1),
        }];
        let mut machine = Machine::new(2);
        assert_eq!(
            machine.run(&program, u64::MAX),
            Run::Faulted(Fault::BadRegister { index: 9 })
        );
        assert!(machine.is_finished(), "a faulted machine is not resumed");
    }

    #[test]
    fn a_jump_past_the_program_faults() {
        let program = vec![Instruction::Jump { target: 99 }];
        let mut machine = Machine::new(1);
        assert_eq!(
            machine.run(&program, u64::MAX),
            Run::Faulted(Fault::BadJump { target: 99 })
        );
    }

    /// Jumping exactly one past the last instruction is how a loop exits, so it
    /// must complete rather than fault on an off-by-one.
    #[test]
    fn a_jump_to_the_end_completes() {
        let program = vec![Instruction::Jump { target: 1 }];
        let mut machine = Machine::new(1);
        assert_eq!(machine.run(&program, u64::MAX), Run::Completed);
    }

    #[test]
    fn adding_a_bool_faults_on_the_type() {
        let program = vec![
            Instruction::LoadConst {
                dst: 0,
                value: Value::Bool(true),
            },
            Instruction::AddInt {
                dst: 1,
                lhs: 0,
                rhs: 0,
            },
        ];
        let mut machine = Machine::new(2);
        assert_eq!(
            machine.run(&program, u64::MAX),
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
        let mut machine = Machine::new(4);
        assert_eq!(
            machine.run(&program, 0),
            Run::Suspended(Suspension::OutOfFuel)
        );
        assert_eq!(machine.program_counter(), 0);
    }
}
