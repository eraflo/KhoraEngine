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

//! Runtime values.
//!
//! Narrow on purpose. The type checker has already proved the program
//! well-typed, so the VM does not need a value shape rich enough to
//! re-litigate that — it needs one small enough to copy cheaply into a register
//! file that gets frozen and thawed on every suspension.
//!
//! `Duration` and `Angle` are distinct at check time but plain floats here:
//! once the units have been proved to agree, carrying the distinction into
//! every arithmetic instruction would cost something and prove nothing.

use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

use crate::arena::{Arena, ArenaRef, Object};

/// A runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// The absence of a value: an unset register, or `void`.
    Unit,
    /// 64-bit signed integer.
    Int(i64),
    /// 32-bit float. Also carries `Duration` (seconds) and `Angle` (radians).
    Float(f32),
    /// Boolean.
    Bool(bool),
    /// An ECS entity handle.
    ///
    /// Carried inline rather than through the arena because it is two `u32`s
    /// and a register already holds more than that — and because an entity
    /// outlives the frame that named it, which the arena's contents do not.
    Entity(EntityId),
    /// Text, wherever it lives.
    Str(StrRef),
    /// A position, direction or scale.
    ///
    /// Inline like [`Entity`](Self::Entity) rather than through the arena: it is
    /// three floats, which a register already holds, and gameplay computes
    /// vectors constantly — an arena allocation per intermediate would make the
    /// commonest arithmetic in a game the most expensive thing in the frame.
    ///
    /// The only engine type a register carries today. `Quat` and `Color` follow
    /// the same road when something needs them; adding them for symmetry alone
    /// would widen every register for values nothing produces.
    Vec3(khora_core::math::Vec3),
    /// Absent optional.
    Null,
}

/// Where a string's characters actually are.
///
/// Two places, because literals and computed text have nothing in common but
/// their type. A literal is fixed when the program is compiled, so it lives in
/// the program and costs nothing to name — a `"hit"` inside a loop must not
/// allocate once per iteration. Text a program *builds* did not exist at
/// compile time and has to go somewhere that can grow, which is the frame
/// arena.
///
/// The consequence to keep in mind: two `StrRef`s comparing unequal says
/// nothing about their text. `==` on strings resolves both sides first — see
/// [`Machine::resolve_str`](crate::vm::Machine::resolve_str) — which is why
/// string equality is not simply `Value == Value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrRef {
    /// A literal, in the program's constant table.
    Const(u32),
    /// Text built while running, in the frame arena.
    Arena(ArenaRef),
}

/// Why a string reference did not resolve.
///
/// Deliberately not a [`Fault`](crate::vm::Fault) or a
/// [`NativeError`](crate::native::NativeError): the lookup is one thing, but its
/// two callers report to different audiences — the VM faults a behavior, an
/// engine function tells its author. Each maps this to its own, and the lookup
/// itself exists once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrError {
    /// The value is not a string at all.
    NotAString(&'static str),
    /// A constant index the running program does not have.
    NotInProgram,
    /// Arena text that no longer exists — from an earlier frame.
    Gone,
}

/// The text a string value stands for.
///
/// Needs the literals *and* the arena because a string lives in one or the
/// other and the value only says which.
pub fn resolve_str<'a>(
    value: Value,
    strings: &'a [String],
    arena: &'a Arena,
) -> Result<&'a str, StrError> {
    let reference = value
        .as_str_ref()
        .ok_or(StrError::NotAString(value.type_name()))?;

    match reference {
        StrRef::Const(index) => strings
            .get(index as usize)
            .map(String::as_str)
            .ok_or(StrError::NotInProgram),
        // A string from an earlier frame does not read as whatever landed at
        // its index: the arena's generation catches it.
        StrRef::Arena(handle) => match arena.get(handle) {
            Ok(Object::Str(text)) => Ok(text),
            _ => Err(StrError::Gone),
        },
    }
}

impl Value {
    /// The integer inside, or `None`.
    pub fn as_int(self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(n),
            _ => None,
        }
    }

    /// The float inside, widening an integer.
    ///
    /// The widening matches the one implicit conversion the language allows, so
    /// the compiler does not have to emit a conversion for every literal used
    /// in a float context.
    pub fn as_float(self) -> Option<f32> {
        match self {
            Self::Float(x) => Some(x),
            Self::Int(n) => Some(n as f32),
            _ => None,
        }
    }

    /// The boolean inside, or `None`.
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The entity inside, or `None`.
    pub fn as_entity(self) -> Option<EntityId> {
        match self {
            Self::Entity(id) => Some(id),
            _ => None,
        }
    }

    /// The vector inside, or `None`.
    pub fn as_vec3(self) -> Option<khora_core::math::Vec3> {
        match self {
            Self::Vec3(v) => Some(v),
            _ => None,
        }
    }

    /// The string reference inside, or `None`.
    ///
    /// A *reference*, not the text: resolving it needs the program and the
    /// arena, which a value on its own does not carry.
    pub fn as_str_ref(self) -> Option<StrRef> {
        match self {
            Self::Str(reference) => Some(reference),
            _ => None,
        }
    }

    /// Whether this is the absent optional.
    pub fn is_null(self) -> bool {
        matches!(self, Self::Null)
    }

    /// Short type name, for fault messages.
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Unit => "void",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Bool(_) => "bool",
            Self::Entity(_) => "Entity",
            Self::Str(_) => "string",
            Self::Vec3(_) => "Vec3",
            Self::Null => "null",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_int_reads_back_as_a_float() {
        assert_eq!(Value::Int(3).as_float(), Some(3.0));
        assert_eq!(Value::Float(1.5).as_float(), Some(1.5));
    }

    /// A float must not silently truncate into an integer: the language has no
    /// implicit narrowing, and the VM must not invent one.
    #[test]
    fn a_float_does_not_read_back_as_an_int() {
        assert_eq!(Value::Float(1.5).as_int(), None);
    }

    #[test]
    fn null_is_distinguishable_from_unit() {
        assert!(Value::Null.is_null());
        assert!(!Value::Unit.is_null());
    }
}
