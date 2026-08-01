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

//! The instruction set.
//!
//! Register-based: operands name registers, results name a destination. The
//! deciding argument is not density but suspension — the live state of a
//! register machine is a program counter and a flat register file, whereas a
//! stack machine would also have to capture the operand stack mid-expression.
//!
//! # One instruction per typed operation
//!
//! `AddInt` and `AddFloat` are separate. The checker has already proved which
//! applies, so making the VM re-inspect its operands at every arithmetic step
//! would pay for a decision that was settled at compile time.
//!
//! Comparisons are the exception: `Less` and friends accept either numeric
//! shape, because emitting four variants for a rarely hot operation buys less
//! than it costs in instruction-set surface.

use serde::{Deserialize, Serialize};

use super::value::Value;

/// A register index within the current call frame.
pub type Reg = u8;

/// One instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instruction {
    /// `dst = value`
    LoadConst {
        /// Destination.
        dst: Reg,
        /// The constant.
        value: Value,
    },
    /// `dst = src`
    Move {
        /// Destination.
        dst: Reg,
        /// Source.
        src: Reg,
    },

    /// `dst = lhs + rhs`, integers.
    AddInt {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs - rhs`, integers.
    SubInt {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs * rhs`, integers.
    MulInt {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs / rhs`, integers. Division by zero faults.
    DivInt {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs % rhs`, integers. Modulo by zero faults.
    RemInt {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = -src`, integers.
    NegInt {
        /// Destination.
        dst: Reg,
        /// Operand.
        src: Reg,
    },

    /// `dst = lhs + rhs`, floats.
    AddFloat {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs - rhs`, floats.
    SubFloat {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs * rhs`, floats.
    MulFloat {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs / rhs`, floats.
    ///
    /// Does **not** fault on a zero divisor: IEEE gives an infinity, and
    /// gameplay code dividing by a zero delta is better served by a number that
    /// propagates than by a disabled behavior.
    DivFloat {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = -src`, floats.
    NegFloat {
        /// Destination.
        dst: Reg,
        /// Operand.
        src: Reg,
    },

    /// `dst = lhs == rhs`. Works on any two values of the same shape.
    Eq {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs < rhs`, numeric.
    Less {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = lhs <= rhs`, numeric.
    LessEq {
        /// Destination.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },
    /// `dst = !src`
    Not {
        /// Destination.
        dst: Reg,
        /// Operand.
        src: Reg,
    },
    /// `dst = src is null`
    IsNull {
        /// Destination.
        dst: Reg,
        /// Operand.
        src: Reg,
    },

    /// Unconditional jump.
    Jump {
        /// Instruction index.
        target: usize,
    },
    /// Jump when `cond` is false — the shape a compiled `if` and `while` need.
    JumpIfNot {
        /// Condition register.
        cond: Reg,
        /// Instruction index.
        target: usize,
    },

    /// Calls `function`, with arguments already placed in `base..base + argc`.
    ///
    /// Laying the arguments out contiguously lets the callee's frame start at
    /// `base` with no copying: its parameters are already where it expects
    /// them.
    Call {
        /// Index into the program's function table.
        function: usize,
        /// First argument register, and the callee's frame base.
        base: Reg,
        /// Argument count.
        argc: u8,
        /// Where the result goes in the caller's frame.
        dst: Reg,
    },
    /// Returns `src` from the current function.
    Return {
        /// The value, or a register holding `Unit` for a void return.
        src: Reg,
    },

    /// Suspends voluntarily. Stands in for `await` until it is wired up.
    Yield,
    /// Stops the program.
    Halt,
}

impl Instruction {
    /// What running this costs from the fuel budget.
    ///
    /// Uniform today. It is a method rather than a constant because a call and
    /// a move plainly do not cost the same, and the shape should be here when
    /// measurement says so — not retrofitted through every call site.
    pub fn cost(&self) -> u64 {
        1
    }
}
