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

//! The call stack: one [`Frame`] per active call, and the moves between
//! frames — a call, a return, a jump within a function.

use serde::{Deserialize, Serialize};

use super::{Fault, Machine, Program, Reg, Step, Value};

/// How deep calls may nest before the machine gives up.
///
/// A script that recurses without a base case would otherwise grow the register
/// file until the process died. Stopping at a limit turns an engine crash into
/// one disabled behavior.
pub(super) const MAX_FRAMES: usize = 64;

/// One active call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Frame {
    /// Index of the function running.
    pub(super) function: usize,
    /// Where this frame's registers begin in the flat file.
    pub(super) base: usize,
    /// Instruction to resume at in the *caller*.
    pub(super) return_pc: usize,
    /// Absolute register the result goes to in the caller.
    pub(super) result: usize,
}

impl Machine {
    /// Where a run starts: the frame depth, the function, the instruction.
    pub(super) fn entry_point(&self) -> (usize, usize, usize) {
        (
            self.frames.len(),
            self.function_index(),
            self.program_counter,
        )
    }

    /// The function the innermost frame runs.
    pub(super) fn function_index(&self) -> usize {
        self.frames
            .last()
            .map_or(usize::MAX, |frame| frame.function)
    }

    /// Pushes a frame for `function`, whose arguments already sit at `base..`
    /// in the caller's window, and moves to its entry.
    pub(super) fn call(
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
        self.registers[callee_base + argc as usize..needed].fill(Value::Unit);

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
    pub(super) fn pop_frame(&mut self, value: Value) -> Option<()> {
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
    pub(super) fn jump_to(&mut self, target: usize, code_len: usize) -> Result<(), Fault> {
        if target > code_len {
            return Err(Fault::BadJump { target });
        }
        self.program_counter = target;
        Ok(())
    }
}
