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

//! Why a program cannot continue: the [`Fault`] a broken script stops with.

use super::Reg;

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
    /// `this` was reached with no entity in play.
    ///
    /// The checker refuses `this` outside a behavior, so reaching this means the
    /// caller ran a behavior's member without naming whose it was — a wiring
    /// mistake in the host, reported rather than aimed at entity zero.
    NoSubject,
    /// A component read whose copy does not fit the component the program
    /// declares — a projection from another build of the engine.
    Unreadable {
        /// The component.
        component: String,
        /// Why.
        reason: String,
    },
    /// A component write whose field has no form the engine takes.
    Unwritable {
        /// The component.
        component: String,
        /// The field.
        field: String,
        /// Why.
        reason: String,
    },
    /// A raise whose payload has no form an event carries — a program from
    /// another build: the checker refuses what the bridge cannot carry.
    Unraisable {
        /// The event.
        event: String,
        /// Why.
        why: String,
    },
    /// An engine function refused.
    NativeFailed {
        /// Which one.
        name: &'static str,
        /// What it said.
        message: String,
    },
    /// `(int)` of a float that no `int` holds: infinite, NaN, or outside the
    /// `i64` range.
    InvalidCast {
        /// The float that could not be converted, written out — a float is not
        /// `Eq`, and a fault is.
        value: String,
    },
    /// An index outside the array it was used on — negative, or at or past
    /// its length.
    IndexOutOfRange {
        /// The index, as the program computed it.
        index: i64,
        /// The array's length.
        len: usize,
    },
    /// An array was expected and something else was found — a reference that
    /// no longer exists, or a value of another kind.
    NotAnObject {
        /// What was found.
        found: &'static str,
    },
    /// A `Spawn` named no prefab: `null`, or an identity the project holds
    /// no prefab by.
    NoPrefab {
        /// What is wrong, naming the prefab as the call wrote it.
        message: String,
    },
}
