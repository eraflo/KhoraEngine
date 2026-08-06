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

//! Between a register, a saved slot, and the engine.
//!
//! The one module that knows all three shapes of a value: the VM's [`Value`],
//! the store's [`Persisted`], and the boundary's [`ScriptValue`]. Everywhere
//! else calls in here.
//!
//! # Why one place, and not four
//!
//! There were four. Written in three crates, weeks apart, by the same hand, and
//! they had already diverged: one of them learned to carry a `Vec3` and another
//! never did. A script could raise an event holding a vector that the delivery
//! then refused — and the refusal read as a fault, so the *receiving* behavior
//! was disabled for good. Nobody wrote that behaviour; it fell out of a table
//! that existed twice.
//!
//! # How the arms are produced
//!
//! The regular ones come from [`script_value_table`], so a row added there
//! reaches all four directions at once. The irregular ones are written out
//! below, inside the same `match`, because each is irregular for a *reason*
//! worth reading — and because a `_` arm would let the next variant slip
//! through silently.
//!
//! **No `match` here has a catch-all.** Adding a variant to `ScriptValue` or to
//! `Value` must break this module's compilation, and only this module's. That
//! is the whole guarantee.
//!
//! [`script_value_table`]: khora_core::script_value_table

use khora_core::script::ScriptValue;

use crate::arena::{Arena, Object, Persisted};
use crate::vm::{resolve_str, StrRef, Value};

/// Why a value could not cross.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrepresentable {
    /// This kind of value has no form on the other side.
    ///
    /// Not a failure to convert — a statement that the target universe has no
    /// place for it. An array in a register, `null` in a scene file.
    Kind {
        /// What it was, as an author would name it.
        kind: &'static str,
        /// Where it was going.
        into: &'static str,
    },
    /// The value has a form; the frame arena had no room for it.
    NoRoom,
}

impl std::fmt::Display for Unrepresentable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kind { kind, into } => write!(f, "a {kind} cannot be put in {into}"),
            Self::NoRoom => f.write_str("the frame's memory is full"),
        }
    }
}

impl Unrepresentable {
    fn kind(kind: &'static str, into: &'static str) -> Self {
        Self::Kind { kind, into }
    }
}

// ─── Boundary → register ────────────────────────────────────────────────────

macro_rules! define_to_register {
    ($($variant:ident : $rust:ty ;)*) => {
        /// The register form of a boundary value.
        ///
        /// Takes the arena because text has to *live* somewhere to be named by a
        /// register, and a boundary string has no index in the running
        /// program's constant table. The conversion that refused strings refused
        /// them for exactly this reason — it had nowhere to put them. Handing it
        /// the arena removes the reason rather than working around it.
        pub fn to_register(
            value: &ScriptValue,
            arena: &mut Arena,
        ) -> Result<Value, Unrepresentable> {
            Ok(match value {
                $(ScriptValue::$variant(inner) => Value::$variant(*inner),)*

                // ── Irregular ──────────────────────────────────────────────
                ScriptValue::Unit => Value::Unit,
                // Copied into frame memory, so it lasts exactly as long as the
                // call that reads it.
                ScriptValue::Str(text) => {
                    let handle = arena
                        .alloc(Object::Str(text.clone()))
                        .map_err(|_| Unrepresentable::NoRoom)?;
                    Value::Str(StrRef::Arena(handle))
                }
                // A register holds one value. Putting an array in one would mean
                // an arena handle whose element type the VM cannot check.
                ScriptValue::Array(_) => {
                    return Err(Unrepresentable::kind("list", "a register"))
                }
                // Named fields exist for writing part of a component. Nothing in
                // the language reads one back.
                ScriptValue::Struct(_) => {
                    return Err(Unrepresentable::kind("struct", "a register"))
                }
            })
        }
    };
}
khora_core::script_value_table!(define_to_register);

// ─── Register → boundary ────────────────────────────────────────────────────

macro_rules! define_from_register {
    ($($variant:ident : $rust:ty ;)*) => {
        /// The boundary form of a register value.
        ///
        /// Needs the program's literals *and* the arena because a string lives
        /// in one or the other and the value only says which.
        pub fn from_register(
            value: Value,
            strings: &[String],
            arena: &Arena,
        ) -> Result<ScriptValue, Unrepresentable> {
            Ok(match value {
                $(Value::$variant(inner) => ScriptValue::$variant(inner),)*

                // ── Irregular ──────────────────────────────────────────────
                Value::Unit => ScriptValue::Unit,
                // Copied, not referenced: whatever the boundary value is handed
                // to outlives the frame, and the arena does not.
                Value::Str(_) => ScriptValue::Str(
                    resolve_str(value, strings, arena)
                        .map_err(|_| Unrepresentable::kind("string", "the engine"))?
                        .to_owned(),
                ),
                // `null` is the absent optional, plus two internal uses — a
                // spent `after`, an unarmed countdown. A handler declares
                // `int amount`, never `int? amount`, so an event carrying this
                // would arrive as something no parameter can be.
                Value::Null => {
                    return Err(Unrepresentable::kind("null", "the engine"))
                }
            })
        }
    };
}
khora_core::script_value_table!(define_from_register);

// ─── Boundary → store ───────────────────────────────────────────────────────

macro_rules! define_to_persisted {
    ($($variant:ident : $rust:ty ;)*) => {
        /// The stored form of a boundary value.
        pub fn to_persisted(value: &ScriptValue) -> Result<Persisted, Unrepresentable> {
            Ok(match value {
                $(ScriptValue::$variant(inner) => Persisted::Scalar(Value::$variant(*inner)),)*

                // ── Irregular ──────────────────────────────────────────────
                ScriptValue::Unit => Persisted::Scalar(Value::Unit),
                // Owned, like every string a field holds: an arena handle would
                // be stale by the next frame, let alone across a save.
                ScriptValue::Str(text) => Persisted::Owned(Object::Str(text.clone())),
                // `Object::Array` holds `Value`s, so this is convertible — but
                // nothing writes one yet, and a conversion with no caller is a
                // conversion nobody has checked.
                ScriptValue::Array(_) => {
                    return Err(Unrepresentable::kind("list", "a saved field"))
                }
                ScriptValue::Struct(_) => {
                    return Err(Unrepresentable::kind("struct", "a saved field"))
                }
            })
        }
    };
}
khora_core::script_value_table!(define_to_persisted);

// ─── Store → boundary ───────────────────────────────────────────────────────

macro_rules! define_from_persisted {
    ($($variant:ident : $rust:ty ;)*) => {
        /// The boundary form of a stored value.
        ///
        /// `Ok(None)` means *nothing was ever written here* — distinct from a
        /// value that happens to be empty. A scene recording "this field has no
        /// value" would load it as such and shadow the default the initialiser
        /// is meant to give it.
        pub fn from_persisted(value: &Persisted) -> Result<Option<ScriptValue>, Unrepresentable> {
            Ok(match value {
                Persisted::Scalar(scalar) => match scalar {
                    $(Value::$variant(inner) => Some(ScriptValue::$variant(*inner)),)*

                    // ── Irregular ──────────────────────────────────────────
                    // An unwritten slot, and a spent `after` — neither is a
                    // value the scene should record.
                    Value::Unit | Value::Null => None,
                    // A string in a *register* is a reference into the program
                    // or the arena, and neither survives the frame. A store
                    // holding one is already wrong; saying so beats reading it.
                    Value::Str(_) => {
                        return Err(Unrepresentable::kind("borrowed string", "the scene"))
                    }
                },
                Persisted::Owned(Object::Str(text)) => Some(ScriptValue::Str(text.clone())),
                Persisted::Owned(Object::Array(_)) => {
                    return Err(Unrepresentable::kind("list", "the scene"))
                }
            })
        }
    };
}
khora_core::script_value_table!(define_from_persisted);

#[cfg(test)]
mod tests;
