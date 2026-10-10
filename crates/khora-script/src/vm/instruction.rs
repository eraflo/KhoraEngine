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
    /// `dst = (float)src`.
    IntToFloat {
        /// Destination.
        dst: Reg,
        /// Operand.
        src: Reg,
    },
    /// `dst = (int)src`, truncated toward zero; faults on a float no `int`
    /// holds.
    FloatToInt {
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
    /// Jump when `src` holds `null` — the test every optional construct
    /// needs: `if (var …)`, `while (var …)`, a `match` arm.
    JumpIfNull {
        /// The value tested.
        src: Reg,
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
    /// Writes a field as the last step of something that must not stop
    /// halfway — an update through a path rooted at the field, a state's
    /// entry, an initialiser. Never a place the run stops at: a stop between
    /// loading a field and writing it back would erase a write another body
    /// made meanwhile, and one between a state's stores would leave it
    /// half-entered. Costs one unit: what it copies was paid for when it was
    /// loaded, copied or built.
    WriteBack {
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

    /// `entity.Set(C { … })` or `entity.Add(C { … })`: queues a write of the
    /// fields of [`Program::patches`](super::Program::patches)`[patch]`, whose
    /// values are in `base..base + count`, for the frame boundary.
    WriteComponent {
        /// Set the named fields, or attach then set them.
        mode: WriteMode,
        /// The entity written to.
        entity: Reg,
        /// Which patch.
        patch: u32,
        /// First value register.
        base: Reg,
        /// How many fields the patch names.
        count: u16,
    },

    /// `entity.Remove(C)`: queues the removal of the component named by
    /// [`Program::strings`](super::Program::strings)`[component]`.
    RemoveComponent {
        /// The entity.
        entity: Reg,
        /// The component's name, as a string index.
        component: u32,
    },

    /// `Spawn(position, C1 { … }, …)`: queues a new entity at `position`
    /// carrying each patch of [`Program::spawns`](super::Program::spawns)`[spawn]`,
    /// whose values follow one another from `base`.
    SpawnEntity {
        /// The new entity's position.
        position: Reg,
        /// Which spawn.
        spawn: u32,
        /// First value register.
        base: Reg,
        /// How many fields all its patches name together.
        count: u16,
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

    /// A place the run may stop when its fuel is spent: a statement's start,
    /// or a loop's head.
    ///
    /// Fuel is checked only at such places — here, at a function's entry, and
    /// just after a call returns — so every machine stopped for fuel stands at
    /// a place the compiler named, with no half-evaluated expression in its
    /// registers. Between two of them a run spends past its budget if it must;
    /// back edges land on one, so that stretch is never a loop.
    Safepoint,

    /// `dst = [base .. base + count]` — a new array of consecutive registers.
    NewArray {
        /// Where the array goes.
        dst: Reg,
        /// The first element's register.
        base: Reg,
        /// How many elements.
        count: u16,
    },
    /// Appends `base .. base + count` to the array in `array` — how a literal
    /// longer than the registers a call can lay out is built, in chunks.
    Extend {
        /// The array.
        array: Reg,
        /// The first element's register.
        base: Reg,
        /// How many elements.
        count: u16,
    },
    /// `dst = object[index]`.
    GetIndex {
        /// Destination.
        dst: Reg,
        /// The array.
        object: Reg,
        /// The index, an `int`.
        index: Reg,
    },
    /// `object[index] = src`, in place.
    SetIndex {
        /// The array.
        object: Reg,
        /// The index, an `int`.
        index: Reg,
        /// The value written.
        src: Reg,
    },
    /// `dst = src.Length` — an array's elements, a string's characters.
    Length {
        /// Destination.
        dst: Reg,
        /// The array or string.
        src: Reg,
    },
    /// `dst = a deep copy of src` — what binding a value read from a place
    /// means under value semantics. Sized: costs what it copies.
    Copy {
        /// Destination.
        dst: Reg,
        /// What is copied.
        src: Reg,
    },

    /// `dst = layout { base .. base + count }` — a new struct of consecutive
    /// registers, one per field in declaration order.
    NewStruct {
        /// Where the struct goes.
        dst: Reg,
        /// Which struct, an index into the program's.
        layout: u16,
        /// The first field's register.
        base: Reg,
        /// How many fields.
        count: u16,
    },
    /// `dst = object.field`, the field by its declaration order.
    GetField {
        /// Destination.
        dst: Reg,
        /// The struct.
        object: Reg,
        /// Which struct it is — what names the field.
        layout: u16,
        /// The field's position.
        slot: u16,
    },
    /// `object.field = src`, in place.
    SetField {
        /// The struct.
        object: Reg,
        /// Which struct it is.
        layout: u16,
        /// The field's position.
        slot: u16,
        /// The value written.
        src: Reg,
    },

    /// `array.Push(src)`, in place.
    Push {
        /// The array.
        array: Reg,
        /// What is appended.
        src: Reg,
    },
    /// `array.RemoveAt(index)`, in place: what follows shifts down.
    RemoveAt {
        /// The array.
        array: Reg,
        /// The index, an `int`.
        index: Reg,
        /// Whether it is charged what it shifts, and a place the run may stop
        /// before — on a variable's array. Inside an update of a field, the
        /// field's load paid for the array, and nothing stops halfway.
        charged: bool,
        /// Where a run that cannot pay for it stops: the start of the path
        /// walked to reach the array. Walking it again is free of effects,
        /// and stopping there rather than here leaves no reference into the
        /// array held across the stop — a reference a suspension would copy
        /// apart from the array it points into.
        from: usize,
    },

    /// Suspends voluntarily, for no stated reason.
    Yield,
    /// Stops the program.
    Halt,
}

impl Instruction {
    /// Where a run stops when it cannot pay for this one, when that is not
    /// the instruction itself.
    pub fn stops_at(&self) -> Option<usize> {
        match self {
            Self::RemoveAt {
                charged: true,
                from,
                ..
            } => Some(*from),
            _ => None,
        }
    }

    /// What running this costs from the fuel budget.
    ///
    /// Uniform today, except for a safepoint, which marks a place rather than
    /// doing work. It is a method rather than a constant because a call and a
    /// move plainly do not cost the same, and the shape should be here when
    /// measurement says so — not retrofitted through every call site.
    ///
    /// Building an array pays for its elements: a ten-thousand-element literal
    /// is not one unit of work. An operation whose size only the running
    /// program knows — `Copy`, a field read or write of an array — is charged
    /// by the VM when it runs; this is its floor.
    pub fn cost(&self) -> u64 {
        match self {
            Self::Safepoint => 0,
            Self::NewArray { count, .. }
            | Self::Extend { count, .. }
            | Self::NewStruct { count, .. }
            | Self::WriteComponent { count, .. }
            | Self::SpawnEntity { count, .. } => 1 + u64::from(*count),
            _ => 1,
        }
    }
}

/// What a [`Instruction::WriteComponent`] does to an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteMode {
    /// Writes the named fields of a component the entity holds.
    Set,
    /// Attaches the component with its defaults, then writes the named fields.
    Add,
}
