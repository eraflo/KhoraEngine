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

    /// Enters a state, with its arguments already in `base..base + argc`.
    ///
    /// Writes which state the behavior is in, then the values it was entered
    /// with. It does **not** jump: a transition ends the current member's turn
    /// rather than redirecting it, and the next dispatch finds the new state.
    /// Jumping would mean the rest of the statement that said `become` ran
    /// inside a state it had just left.
    Become {
        /// The state's discriminant.
        state: u16,
        /// First argument register.
        base: Reg,
        /// Argument count.
        argc: u8,
        /// The slot holding which state the behavior is in.
        state_slot: u16,
        /// Where the state's own data begins.
        data_slot: u16,
    },

    /// Reads one of the running behavior's fields.
    ///
    /// Fields do not live in registers: a register file belongs to a call, and
    /// a field outlives every call made on the entity. They live in the
    /// behavior's persistent store, which is what a scene save writes out.
    LoadField {
        /// Where the value goes.
        dst: Reg,
        /// Which field, by declaration order.
        slot: u16,
    },
    /// Writes one of the running behavior's fields.
    StoreField {
        /// Which field, by declaration order.
        slot: u16,
        /// The value.
        src: Reg,
    },

    /// Loads a string literal from the program's constant table.
    ///
    /// Names the literal rather than copying it, which is what keeps a literal
    /// inside a loop free.
    LoadStr {
        /// Where it goes.
        dst: Reg,
        /// Index into [`Program::strings`](crate::vm::Program::strings).
        index: u32,
    },
    /// Joins two strings, allocating the result in the frame arena.
    ///
    /// The result is new text, so unlike a literal it has nowhere to live but
    /// the arena — and therefore lasts one frame, like everything else there.
    Concat {
        /// Where the joined text goes.
        dst: Reg,
        /// Left operand.
        lhs: Reg,
        /// Right operand.
        rhs: Reg,
    },

    /// Calls an engine function, with arguments in `base..base + argc`.
    ///
    /// Separate from [`Self::Call`] because nothing about it is a frame: there
    /// is no callee to give registers to and nothing to return into. The whole
    /// call happens inside one instruction, which is also why it is the one
    /// place a script cannot be suspended mid-way — a native either completes
    /// or faults.
    NativeCall {
        /// Index into the [`NativeRegistry`](crate::native::NativeRegistry).
        function: usize,
        /// First argument register.
        base: Reg,
        /// Argument count.
        argc: u8,
        /// Where the result goes.
        dst: Reg,
    },

    /// Suspends until `seconds` of game time have passed.
    ///
    /// The same suspension the fuel budget uses — the machine stops on an
    /// instruction boundary and can be resumed. Only the *reason* differs, and
    /// the reason is what the caller reads to decide when to come back.
    Await {
        /// Register holding the duration, in seconds.
        seconds: Reg,
        /// Where the awaited value goes. `Unit` for a duration: it is the
        /// waiting that matters, not what it produces.
        dst: Reg,
    },

    /// Loads the entity the running behavior is attached to — `this`.
    ///
    /// An instruction rather than a native, because it reads nothing and can
    /// fail in only one way: a free function has no entity, and the checker
    /// already refuses `this` there. It is the address of the subject, which is
    /// what every effect a script asks for has to be aimed at.
    LoadSelf {
        /// Where it goes.
        dst: Reg,
    },

    /// Suspends voluntarily, for no stated reason.
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
