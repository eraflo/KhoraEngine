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

mod commands;
mod cost;
pub mod display;
mod fault;
mod frames;
mod freeze;
mod held;
pub mod instruction;
mod objects;
pub mod program;
mod register_file;
pub mod resume;
mod rows;
pub mod site;
mod step;
pub mod value;

pub use fault::Fault;
pub use instruction::{Instruction, Reg, WriteMode};
pub use program::{
    BehaviorLayout, DeclaredComponent, DeclaredField, DeclaredKind, EventSignature, Function,
    Patch, Program, QueryShape, Schedule, StateLayout, StructLayout, TimerKind, TimerLayout,
};
pub use resume::{resume, Abandoned, ResumeTier};
pub use site::{Site, SiteKind, SiteLocal};
pub use value::{resolve_str, ObjRef, StrError, StrRef, Value};

use serde::{Deserialize, Serialize};

use crate::native::Host;
use frames::{Frame, MAX_FRAMES};
use step::Step;

/// Why a program stopped before finishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suspension {
    /// The fuel allowance ran out. Resume when the budget allows.
    OutOfFuel,
    /// The program yielded. Resume when whatever it waits on resolves.
    Awaiting,
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
    /// What the outermost function was called with.
    ///
    /// Kept apart from its registers because a parameter is a local the body
    /// may reassign: when an edit leaves no place to resume at, the body is run
    /// again from its entry, and it has to be handed what it was handed then.
    #[serde(default)]
    arguments: Vec<Value>,
    /// The text and arrays the registers and arguments referenced when the
    /// machine last suspended, each named by a [`StrRef::Held`] or an
    /// [`ObjRef::Held`]. Empty while it runs.
    #[serde(default)]
    held: Vec<crate::arena::Owned>,
    /// Where in the arena each held value was copied from — see
    /// [`held::Origins`].
    #[serde(skip)]
    origins: held::Origins,
    /// The `ref` rows the current run has written and not yet sent — always
    /// empty between runs.
    #[serde(skip)]
    rows: rows::PendingRows,
}

impl Machine {
    /// A machine ready to run `function` with `args`.
    ///
    /// Returns `None` when the function does not exist or the wrong number of
    /// arguments was supplied — a caller error, not a script fault.
    pub fn new(program: &Program, function: &str, args: &[Value]) -> Option<Self> {
        Self::at(program, program.index_of(function)?, args)
    }

    /// [`new`](Self::new), for the function at `index` — what a caller that
    /// resolved it once already holds.
    pub fn at(program: &Program, index: usize, args: &[Value]) -> Option<Self> {
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
            arguments: args.to_vec(),
            held: Vec::new(),
            origins: held::Origins::default(),
            rows: Default::default(),
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

    /// The function the outermost frame runs, by index: the body this machine
    /// is part-way through.
    pub fn outermost(&self) -> Option<usize> {
        self.frames.first().map(|frame| frame.function)
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
        let mut overdraft = 0;
        // The text it held while suspended, back where running code reads it.
        if !self.finished {
            if let Err(fault) = self.rehydrate(&mut host.arena, program) {
                return (self.fault(fault), 0);
            }
        }
        let outcome = self.run_inner(program, host, &mut remaining, &mut overdraft);
        // However the run stopped, what it wrote leaves with it.
        self.send_rows(host, None);
        // And out again: the arena it built text in is emptied before the
        // machine next runs, and may be saved before then.
        if matches!(outcome, Run::Suspended(_)) {
            self.evacuate(&host.arena, program);
        }
        (outcome, fuel.saturating_sub(remaining) + overdraft)
    }

    /// The run loop. `remaining` is left holding what was not spent;
    /// `overdraft` what the run spent beyond `fuel`, running on to the next
    /// safepoint once the slice was used up.
    fn run_inner(
        &mut self,
        program: &Program,
        host: &mut Host,
        remaining: &mut u64,
        overdraft: &mut u64,
    ) -> Run {
        if self.finished {
            return Run::Completed;
        }
        // A run starts where a suspension left it — a safepoint or just past an
        // `await` — or at a function's entry: a place it may stop at.
        let mut at_safepoint = true;
        // Whether this run has executed nothing yet: a sized operation it starts
        // at runs whatever it costs, so every run makes progress.
        let mut first = true;
        // Where this run started, in which frame: a sized operation that stops
        // a run at an earlier place it can walk back to runs anyway when the
        // run started there, so a run that resumes there makes progress.
        // Once: the operation the run resumed for, not every later one that
        // walks back to the same place.
        let mut entry = Some(self.entry_point());

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
                    Some(()) => {
                        at_safepoint = true;
                        continue;
                    }
                    None => {
                        self.finished = true;
                        return Run::Completed;
                    }
                }
            };

            // Checked only at a safepoint, so a machine stopped for fuel always
            // stands at a place the compiler named: a statement's start, a
            // loop's head, a function's entry, or just past a call's return —
            // never with half an expression in its registers. A run with any
            // fuel at all passes the place it starts from, so every non-zero
            // slice makes progress.
            if matches!(instruction, Instruction::Safepoint) {
                at_safepoint = true;
            }
            if at_safepoint && *remaining == 0 {
                return Run::Suspended(Suspension::OutOfFuel);
            }

            // A native is charged what it declares rather than the flat
            // instruction cost: a raycast is not a `Move`, and billing it as
            // one would let a behavior spend a frame inside a single call while
            // the counter reported it had barely started.
            let cost = match instruction {
                Instruction::NativeCall { function, .. } => match host.natives.at(function) {
                    Some(native) => native.cost,
                    None => return self.fault(Fault::UnknownNative { index: function }),
                },
                _ => match self.sized_cost(&instruction, program, host) {
                    // A sized operation is its own checkpoint: it does not
                    // start when the fuel left cannot pay for it — unless it is
                    // the first thing this run does, so every run progresses.
                    Some(cost) => {
                        // Whether it is paid for or not, the first sized
                        // operation spends the pass the run started with.
                        let pass = entry.take();
                        if cost > *remaining && !first {
                            let back = instruction.stops_at().unwrap_or(self.program_counter);
                            let here = (self.frames.len(), self.function_index(), back);
                            if pass != Some(here) {
                                self.program_counter = back;
                                return Run::Suspended(Suspension::OutOfFuel);
                            }
                        }
                        cost
                    }
                    None => instruction.cost(),
                },
            };
            // Past the budget, the run goes on to the next safepoint and the
            // rest is charged as an overdraft. Never a loop: every back edge
            // lands on a safepoint, so the stretch is bounded by the code
            // between two of them.
            if *remaining < cost {
                *overdraft += cost - *remaining;
                *remaining = 0;
            } else {
                *remaining -= cost;
            }
            at_safepoint = false;
            // A safepoint does no work: the run's first instruction is the
            // first that costs something.
            if cost > 0 {
                first = false;
            }
            let from = self.program_counter;

            match self.step(&instruction, program, host, function.code.len()) {
                Ok(Step::Next) => self.program_counter += 1,
                // A call lands on its callee's entry. A jump backwards lands on
                // a loop's head — a safepoint the compiler marked, and one
                // anyway for code nobody compiled, so no loop can spin past
                // its budget.
                Ok(Step::Jumped) => {
                    at_safepoint = matches!(instruction, Instruction::Call { .. })
                        || self.program_counter <= from;
                }
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
                // Back in the caller, just past the call.
                Ok(Step::Returned) => {
                    if self.frames.is_empty() {
                        self.finished = true;
                        return Run::Completed;
                    }
                    at_safepoint = true;
                }
                Err(fault) => return self.fault(fault),
            }
        }
    }

    fn fault(&mut self, fault: Fault) -> Run {
        self.finished = true;
        Run::Faulted(fault)
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
}

#[cfg(test)]
mod tests;
