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
//! # The contract
//!
//! Running with a fuel budget either finishes, or stops and hands back a
//! machine that can be resumed. **Resuming must be indistinguishable from never
//! having stopped** — the invariant the tests defend at every possible
//! interruption point, not at a sampled few.
//!
//! # Fuel
//!
//! Every instruction costs fuel. The DCC's frame budget becomes a fuel
//! allowance, so a script that would overrun its share stops mid-way and
//! resumes next frame. Observable behaviour never changes — only *when* the
//! work happens, which is the engine's rule of adapting the HOW and never the
//! WHAT.
//!
//! # Calls
//!
//! Frames are explicit rather than recursive Rust calls. A native recursion
//! would put the interpreter's own stack in the middle of the machine state,
//! and that state has to be freezable — mid-call, mid-`await`, into a saved
//! scene. Everything that must survive lives in [`Machine`], and nothing else
//! does.

pub mod instruction;
pub mod program;
pub mod value;

#[cfg(test)]
mod tests;

pub use instruction::{Instruction, Reg};
pub use program::{BehaviorLayout, Function, Program, StateLayout, TimerKind, TimerLayout};
pub use value::{resolve_str, StrError, StrRef, Value};

use serde::{Deserialize, Serialize};

use crate::arena::{Object, Persisted};
use crate::native::Host;

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
    /// A register index outside the current frame.
    BadRegister {
        /// The offending index.
        index: Reg,
    },
    /// A jump outside the function.
    BadJump {
        /// The offending instruction index.
        target: usize,
    },
    /// A call to a function index that does not exist.
    BadFunction {
        /// The offending index.
        index: usize,
    },
    /// An operand of the wrong type.
    TypeMismatch {
        /// What the instruction required.
        expected: &'static str,
        /// What it found.
        found: &'static str,
    },
    /// Integer division or modulo by zero.
    DivideByZero,
    /// The call stack grew past its limit.
    StackOverflow,
    /// A call to an engine function the host does not expose.
    ///
    /// Means the program was compiled against a different registry than the one
    /// running it — the indices no longer line up, so continuing would call
    /// whatever now sits at that slot.
    UnknownNative {
        /// The offending index.
        index: usize,
    },
    /// A string reference that no longer resolves.
    ///
    /// A constant index outside the program, or arena text from an earlier
    /// frame — the second is the case the arena's generation counter exists to
    /// catch, surfacing here rather than reading whatever landed at that index.
    BadString,
    /// A frame allocated more text than the arena holds.
    ArenaFull,
    /// An engine function refused.
    NativeFailed {
        /// Which one.
        name: &'static str,
        /// What it said.
        message: String,
    },
}

/// The outcome of one [`Machine::run`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// The program reached its end, a `Halt`, or returned from the outermost
    /// frame.
    Completed,
    /// The program stopped and can be resumed.
    Suspended(Suspension),
    /// The program is broken and will not be resumed.
    Faulted(Fault),
}

/// How deep calls may nest before the machine gives up.
///
/// A script that recurses without a base case would otherwise grow the register
/// file until the process died. Stopping at a limit turns an engine crash into
/// one disabled behavior.
const MAX_FRAMES: usize = 64;

/// One active call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Frame {
    /// Index of the function running.
    function: usize,
    /// Where this frame's registers begin in the flat file.
    base: usize,
    /// Instruction to resume at in the *caller*.
    return_pc: usize,
    /// Absolute register the result goes to in the caller.
    result: usize,
}

/// The machine.
///
/// Everything that must survive an interruption lives here, and nothing else
/// does. Serialisable state is what lets a scene be saved mid-`await`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// A flat register file; each frame owns a window into it.
    registers: Vec<Value>,
    /// The call stack, outermost first.
    frames: Vec<Frame>,
    /// Index of the *next* instruction in the innermost frame's function.
    /// Suspending is nothing more than returning while this points at work not
    /// yet done.
    program_counter: usize,
    /// Set once the program has halted or faulted, so a resume cannot restart
    /// a finished program.
    finished: bool,
}

impl Machine {
    /// A machine ready to run `function` with `args`.
    ///
    /// Returns `None` when the function does not exist or the wrong number of
    /// arguments was supplied — a caller error, not a script fault.
    pub fn new(program: &Program, function: &str, args: &[Value]) -> Option<Self> {
        let index = program.index_of(function)?;
        let target = program.functions.get(index)?;
        if args.len() != target.arity {
            return None;
        }

        let mut registers = vec![Value::Unit; target.registers.max(args.len())];
        registers[..args.len()].copy_from_slice(args);

        Some(Self {
            registers,
            frames: vec![Frame {
                function: index,
                base: 0,
                return_pc: 0,
                result: 0,
            }],
            program_counter: 0,
            finished: false,
        })
    }

    /// Reads a register in the innermost frame.
    pub fn register(&self, index: Reg) -> Option<Value> {
        let base = self.frames.last()?.base;
        self.registers.get(base + index as usize).copied()
    }

    /// The value the outermost frame returned, once finished.
    pub fn result(&self) -> Value {
        self.registers.first().copied().unwrap_or(Value::Unit)
    }

    /// Whether the program has run to completion (or faulted).
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The instruction that will run next. Useful for debugging a suspension.
    pub fn program_counter(&self) -> usize {
        self.program_counter
    }

    /// How deep the call stack currently is.
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Runs until the program finishes, faults, yields, or burns `fuel`.
    ///
    /// Call again to resume — the machine holds everything needed and does not
    /// care how long it was left alone, or whether it was serialised in
    /// between.
    ///
    /// `host` is what the program can reach outside itself. It is borrowed for
    /// the run rather than held by the machine, because the machine is what
    /// gets serialised with the scene and the host is what belongs to the
    /// frame — a machine that owned its host could not be saved.
    pub fn run(&mut self, program: &Program, host: &mut Host, fuel: u64) -> Run {
        self.run_counting(program, host, fuel).0
    }

    /// Runs, and reports how much fuel was actually spent.
    ///
    /// A caller sharing one budget across many programs needs the second number:
    /// without it, it can only count *calls*, and a call is not a unit of work —
    /// one handler is a dozen instructions and another is a thousand. Counting
    /// calls would hand the same slice to both and call that a budget.
    pub fn run_counting(&mut self, program: &Program, host: &mut Host, fuel: u64) -> (Run, u64) {
        let mut remaining = fuel;
        let outcome = self.run_inner(program, host, &mut remaining);
        (outcome, fuel.saturating_sub(remaining))
    }

    /// The run loop. `remaining` is left holding what was not spent.
    fn run_inner(&mut self, program: &Program, host: &mut Host, remaining: &mut u64) -> Run {
        if self.finished {
            return Run::Completed;
        }

        loop {
            let Some(frame) = self.frames.last() else {
                self.finished = true;
                return Run::Completed;
            };
            let Some(function) = program.functions.get(frame.function) else {
                return self.fault(Fault::BadFunction {
                    index: frame.function,
                });
            };

            // Falling off the end returns Unit, so a compiler emitting
            // straight-line code needs no trailing `Return` to be correct.
            let Some(instruction) = function.code.get(self.program_counter).cloned() else {
                match self.pop_frame(Value::Unit) {
                    Some(()) => continue,
                    None => {
                        self.finished = true;
                        return Run::Completed;
                    }
                }
            };

            // Checked before the instruction, so a suspension always lands on
            // an instruction boundary. Landing mid-instruction would mean
            // capturing partial results, which is what the register machine
            // was chosen to avoid.
            //
            // A native is charged what it declares rather than the flat
            // instruction cost: a raycast is not a `Move`, and billing it as
            // one would let a behavior spend a frame inside a single call while
            // the counter reported it had barely started.
            let cost = match instruction {
                Instruction::NativeCall { function, .. } => match host.natives.at(function) {
                    Some(native) => native.cost,
                    None => return self.fault(Fault::UnknownNative { index: function }),
                },
                _ => instruction.cost(),
            };
            if *remaining < cost {
                return Run::Suspended(Suspension::OutOfFuel);
            }
            *remaining -= cost;

            match self.step(&instruction, program, host, function.code.len()) {
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
                Ok(Step::Returned) => {
                    if self.frames.is_empty() {
                        self.finished = true;
                        return Run::Completed;
                    }
                }
                Err(fault) => return self.fault(fault),
            }
        }
    }

    fn fault(&mut self, fault: Fault) -> Run {
        self.finished = true;
        Run::Faulted(fault)
    }

    /// Executes one instruction.
    fn step(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
        code_len: usize,
    ) -> Result<Step, Fault> {
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

            Instruction::AddInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_add),
            Instruction::SubInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_sub),
            Instruction::MulInt { dst, lhs, rhs } => self.int_op(dst, lhs, rhs, i64::wrapping_mul),
            Instruction::DivInt { dst, lhs, rhs } => {
                self.checked_int_op(dst, lhs, rhs, |a, b| (b != 0).then(|| a.wrapping_div(b)))
            }
            Instruction::RemInt { dst, lhs, rhs } => {
                self.checked_int_op(dst, lhs, rhs, |a, b| (b != 0).then(|| a.wrapping_rem(b)))
            }
            Instruction::NegInt { dst, src } => {
                let value = self.int(src)?;
                self.write(dst, Value::Int(value.wrapping_neg()))?;
                Ok(Step::Next)
            }

            Instruction::AddFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a + b),
            Instruction::SubFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a - b),
            Instruction::MulFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a * b),
            Instruction::DivFloat { dst, lhs, rhs } => self.float_op(dst, lhs, rhs, |a, b| a / b),
            Instruction::NegFloat { dst, src } => {
                let value = self.float(src)?;
                self.write(dst, Value::Float(-value))?;
                Ok(Step::Next)
            }

            Instruction::Eq { dst, lhs, rhs } => {
                let a = self.read(lhs)?;
                let b = self.read(rhs)?;
                // Comparing an int against a float compares their numeric
                // value; the checker only allows it where that is meant.
                let equal = match (a, b) {
                    (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => {
                        x as f32 == y
                    }
                    // Two strings are equal when their *text* is. Comparing the
                    // references would answer no for a literal and a computed
                    // string spelling the same thing, which is the one case a
                    // script most often means to compare.
                    (Value::Str(_), Value::Str(_)) => {
                        self.resolve_str(a, program, host)? == self.resolve_str(b, program, host)?
                    }
                    _ => a == b,
                };
                self.write(dst, Value::Bool(equal))?;
                Ok(Step::Next)
            }
            Instruction::Less { dst, lhs, rhs } => self.compare(dst, lhs, rhs, true),
            Instruction::LessEq { dst, lhs, rhs } => self.compare(dst, lhs, rhs, false),
            Instruction::Not { dst, src } => {
                let value = self.read(src)?;
                let flag = value.as_bool().ok_or(Fault::TypeMismatch {
                    expected: "bool",
                    found: value.type_name(),
                })?;
                self.write(dst, Value::Bool(!flag))?;
                Ok(Step::Next)
            }
            Instruction::IsNull { dst, src } => {
                let value = self.read(src)?;
                self.write(dst, Value::Bool(value.is_null()))?;
                Ok(Step::Next)
            }

            Instruction::Jump { target } => {
                self.jump_to(target, code_len)?;
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
                    self.jump_to(target, code_len)?;
                    Ok(Step::Jumped)
                }
            }

            Instruction::Call {
                function,
                base,
                argc,
                dst,
            } => self.call(program, function, base, argc, dst),
            Instruction::Return { src } => {
                let value = self.read(src)?;
                // Whether a caller took the value or the program ended is
                // decided by `run`, which checks whether any frame is left.
                self.pop_frame(value);
                Ok(Step::Returned)
            }

            Instruction::Become {
                state,
                base,
                argc,
                state_slot,
                data_slot,
            } => {
                // The arguments first, while the old state's data is still
                // there: an argument may have been read out of it, and writing
                // the discriminant early would only matter if this could fail
                // part-way, which it cannot.
                for offset in 0..argc {
                    let value = self.read(base + offset)?;
                    host.fields.set(
                        data_slot as usize + offset as usize,
                        Persisted::Scalar(value),
                    );
                }
                host.fields.set(
                    state_slot as usize,
                    Persisted::Scalar(Value::Int(state as i64)),
                );
                Ok(Step::Next)
            }

            Instruction::LoadField { dst, slot } => {
                let value = match host.fields.get(slot as usize) {
                    Some(Persisted::Scalar(value)) => *value,
                    // Text kept in a field is stored by value, so reading it
                    // brings a copy into the frame arena — where everything the
                    // running code can name lives.
                    Some(Persisted::Owned(object)) => {
                        let copy = object.clone();
                        let reference = host.arena.alloc(copy).map_err(|_| Fault::ArenaFull)?;
                        Value::Str(StrRef::Arena(reference))
                    }
                    // A slot the instance does not have yet: a script that
                    // gained a field since this entity was saved. Unset rather
                    // than a fault — hot-reload is meant to survive that.
                    None => Value::Unit,
                };
                self.write(dst, value)?;
                Ok(Step::Next)
            }
            Instruction::StoreField { slot, src } => {
                let value = self.read(src)?;
                let stored = match value {
                    // Stored by value, not by reference: an arena handle would
                    // be stale by the next frame, and a field is exactly what
                    // has to outlive one.
                    Value::Str(_) => {
                        let text = self.resolve_str(value, program, host)?.to_owned();
                        Persisted::Owned(Object::Str(text))
                    }
                    other => Persisted::Scalar(other),
                };
                host.fields.set(slot as usize, stored);
                Ok(Step::Next)
            }

            Instruction::LoadStr { dst, index } => {
                if program.string(index).is_none() {
                    return Err(Fault::BadString);
                }
                self.write(dst, Value::Str(StrRef::Const(index)))?;
                Ok(Step::Next)
            }
            Instruction::Concat { dst, lhs, rhs } => {
                let left = self.read(lhs)?;
                let right = self.read(rhs)?;
                let joined = format!(
                    "{}{}",
                    self.resolve_str(left, program, host)?,
                    self.resolve_str(right, program, host)?
                );
                let reference = host
                    .arena
                    .alloc(crate::arena::Object::Str(joined))
                    .map_err(|_| Fault::ArenaFull)?;
                self.write(dst, Value::Str(StrRef::Arena(reference)))?;
                Ok(Step::Next)
            }

            Instruction::NativeCall {
                function,
                base,
                argc,
                dst,
            } => self.native_call(program, host, function, base, argc, dst),

            Instruction::Await { seconds, dst } => {
                let value = self.read(seconds)?;
                let wait = value.as_float().ok_or(Fault::TypeMismatch {
                    expected: "Duration",
                    found: value.type_name(),
                })?;
                // Written before suspending, so resuming finds it already there
                // and does not have to know an `await` was in progress.
                self.write(dst, Value::Unit)?;
                // The caller reads this to decide when to come back. Left on the
                // host rather than returned, because the suspension itself is
                // the same one the budget produces — only the reason differs.
                host.awaiting = Some(wait.max(0.0));
                Ok(Step::Yield)
            }

            Instruction::Yield => Ok(Step::Yield),
            Instruction::Halt => Ok(Step::Halt),
        }
    }

    /// The text a string value stands for.
    ///
    /// Needs both the program and the arena because a string can live in
    /// either, and the value itself carries only which. Borrows both for the
    /// call rather than copying the text, so comparing two strings costs no
    /// allocation.
    pub fn resolve_str<'a>(
        &self,
        value: Value,
        program: &'a Program,
        host: &'a Host,
    ) -> Result<&'a str, Fault> {
        value::resolve_str(value, &program.strings, &host.arena).map_err(|error| match error {
            StrError::NotAString(found) => Fault::TypeMismatch {
                expected: "string",
                found,
            },
            StrError::NotInProgram | StrError::Gone => Fault::BadString,
        })
    }

    /// Runs an engine function and writes its result back.
    ///
    /// No frame is pushed: a native has no bytecode to give registers to, and
    /// nothing to return into. That is also why it is the one place a run
    /// cannot be suspended part-way — the call either completes or faults.
    fn native_call(
        &mut self,
        program: &Program,
        host: &mut Host,
        function: usize,
        base: Reg,
        argc: u8,
        dst: Reg,
    ) -> Result<Step, Fault> {
        let native = host
            .natives
            .at(function)
            .ok_or(Fault::UnknownNative { index: function })?;

        // Copied out rather than borrowed: the native takes the host mutably,
        // and the arguments live in the machine's register file, so holding a
        // slice across the call would borrow both at once.
        let mut args = Vec::with_capacity(argc as usize);
        for offset in 0..argc {
            args.push(self.read(base + offset)?);
        }

        let mut context = host.context(&program.strings);
        let result = (native.call)(&mut context, &args).map_err(|error| Fault::NativeFailed {
            name: native.name,
            message: error.message,
        })?;

        self.write(dst, result)?;
        Ok(Step::Next)
    }

    fn call(
        &mut self,
        program: &Program,
        function: usize,
        base: Reg,
        argc: u8,
        dst: Reg,
    ) -> Result<Step, Fault> {
        let target = program
            .functions
            .get(function)
            .ok_or(Fault::BadFunction { index: function })?;

        if self.frames.len() >= MAX_FRAMES {
            return Err(Fault::StackOverflow);
        }

        let caller_base = self.frames.last().map(|f| f.base).unwrap_or(0);
        let callee_base = caller_base + base as usize;
        let result = caller_base + dst as usize;

        // The arguments already sit at `callee_base..`, so the callee's frame
        // starts there and its parameters need no copying.
        let needed = callee_base + target.registers.max(argc as usize);
        if self.registers.len() < needed {
            self.registers.resize(needed, Value::Unit);
        }
        // Registers past the arguments belong to the callee and must not carry
        // whatever the caller left there.
        for slot in callee_base + argc as usize..needed {
            self.registers[slot] = Value::Unit;
        }

        self.frames.push(Frame {
            function,
            base: callee_base,
            return_pc: self.program_counter + 1,
            result,
        });
        self.program_counter = 0;
        Ok(Step::Jumped)
    }

    /// Pops the innermost frame, delivering `value` to its caller.
    ///
    /// Returns `None` when the outermost frame returned, which ends the program.
    fn pop_frame(&mut self, value: Value) -> Option<()> {
        let frame = self.frames.pop()?;
        if self.frames.is_empty() {
            // The outermost result goes to register 0, where `result` reads it.
            if let Some(slot) = self.registers.first_mut() {
                *slot = value;
            }
            return None;
        }
        if let Some(slot) = self.registers.get_mut(frame.result) {
            *slot = value;
        }
        self.program_counter = frame.return_pc;
        Some(())
    }

    /// Moves the program counter, rejecting a target outside the function.
    ///
    /// `code_len` is a valid target: jumping one past the end is how a loop
    /// exits, and the next iteration turns it into a return.
    fn jump_to(&mut self, target: usize, code_len: usize) -> Result<(), Fault> {
        if target > code_len {
            return Err(Fault::BadJump { target });
        }
        self.program_counter = target;
        Ok(())
    }

    // ── Register access ───────────────────────────────

    fn slot(&self, index: Reg) -> Result<usize, Fault> {
        let base = self.frames.last().map(|f| f.base).unwrap_or(0);
        let slot = base + index as usize;
        if slot >= self.registers.len() {
            return Err(Fault::BadRegister { index });
        }
        Ok(slot)
    }

    fn read(&self, index: Reg) -> Result<Value, Fault> {
        Ok(self.registers[self.slot(index)?])
    }

    fn write(&mut self, index: Reg, value: Value) -> Result<(), Fault> {
        let slot = self.slot(index)?;
        self.registers[slot] = value;
        Ok(())
    }

    fn int(&self, index: Reg) -> Result<i64, Fault> {
        let value = self.read(index)?;
        value.as_int().ok_or(Fault::TypeMismatch {
            expected: "int",
            found: value.type_name(),
        })
    }

    fn float(&self, index: Reg) -> Result<f32, Fault> {
        let value = self.read(index)?;
        value.as_float().ok_or(Fault::TypeMismatch {
            expected: "float",
            found: value.type_name(),
        })
    }

    fn int_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(i64, i64) -> i64,
    ) -> Result<Step, Fault> {
        let a = self.int(lhs)?;
        let b = self.int(rhs)?;
        self.write(dst, Value::Int(op(a, b)))?;
        Ok(Step::Next)
    }

    /// An integer operation that can refuse its operands — division by zero.
    fn checked_int_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(i64, i64) -> Option<i64>,
    ) -> Result<Step, Fault> {
        let a = self.int(lhs)?;
        let b = self.int(rhs)?;
        let value = op(a, b).ok_or(Fault::DivideByZero)?;
        self.write(dst, Value::Int(value))?;
        Ok(Step::Next)
    }

    fn float_op(
        &mut self,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        op: fn(f32, f32) -> f32,
    ) -> Result<Step, Fault> {
        let a = self.float(lhs)?;
        let b = self.float(rhs)?;
        self.write(dst, Value::Float(op(a, b)))?;
        Ok(Step::Next)
    }

    /// `lhs < rhs` when `strict`, `lhs <= rhs` otherwise.
    ///
    /// Two integers compare as integers: routing them through `f32` would lose
    /// precision above 2^24, and a comparison that goes wrong on large counters
    /// is the kind of bug nobody looks for.
    fn compare(&mut self, dst: Reg, lhs: Reg, rhs: Reg, strict: bool) -> Result<Step, Fault> {
        let a = self.read(lhs)?;
        let b = self.read(rhs)?;

        let result = match (a.as_int(), b.as_int()) {
            (Some(x), Some(y)) => {
                if strict {
                    x < y
                } else {
                    x <= y
                }
            }
            _ => {
                let x = a.as_float().ok_or(Fault::TypeMismatch {
                    expected: "a number",
                    found: a.type_name(),
                })?;
                let y = b.as_float().ok_or(Fault::TypeMismatch {
                    expected: "a number",
                    found: b.type_name(),
                })?;
                if strict {
                    x < y
                } else {
                    x <= y
                }
            }
        };

        self.write(dst, Value::Bool(result))?;
        Ok(Step::Next)
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
    /// A frame was popped.
    Returned,
}
