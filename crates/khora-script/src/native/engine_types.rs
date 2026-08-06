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

//! The engine's math types, as a script sees them.
//!
//! Exposing `Vec3` once cost a hand-written `ScriptType`, a hand-written
//! constructor, and an arm in each of four converters. The converters are gone
//! (see [`bridge`](crate::bridge)); this removes the other two. A type is now a
//! declaration, and `Quat`, `Color`, `Vec2` and `Vec4` cost a line each.
//!
//! # Why a declaration and not `#[ergon_type]` on the struct
//!
//! The plan called for an attribute, sister to `#[ergon_fn]`. Writing it made
//! the shape clear: an attribute belongs on the item it describes, and these
//! types live in `khora-core`, which must not carry scripting annotations. More
//! decisively, [`ScriptType`] needs a matching `Value` variant, and those come
//! from [`script_value_table`] — so a game *cannot* expose a type of its own
//! this way whatever the syntax. The thing being written down is not "this
//! struct is scriptable" but "this table row is reachable from a script", and a
//! declaration says that where an attribute would imply the other.
//!
//! No proc macro either: the only thing one would have added is turning `x` into
//! `X`, and naming the accessor outright turned out to read better than deriving
//! it — see below.
//!
//! # `v.x`, not `X(v)`
//!
//! The surface an author writes is field access, which is what the design
//! documents promise. A free function per component cannot work: the language
//! has no overloading, so one `X` could serve exactly one type, and `Vec2`,
//! `Vec4` and `Quat` would each need a different spelling of the same idea.
//!
//! So each component becomes a native named `Vec3.x` — a name **no source can
//! spell**, because a `.` is not an identifier. It cannot collide with a
//! function a game declares, and it leaves `X` free. The only thing that reaches
//! it is the compiler lowering `v.x`.
//!
//! [`script_value_table`]: khora_core::script_value_table
//! [`ScriptType`]: super::ScriptType

use super::{NativeContext, NativeError, NativeFn, NativeTy, ScriptType};
use crate::vm::Value;

/// Declares an engine type, its constructor, and one accessor per component.
///
/// `Variant` must be a row of [`script_value_table`](khora_core::script_value_table):
/// it names the `Value` variant, the type as Ergon writes it, and the
/// constructor. The Rust type must have a `new` taking the components in the
/// order given.
macro_rules! ergon_type {
    // Each component is a float; a repetition needs an expression per field, and
    // this is what turns a name into one. First, so it matches before the
    // declaration form below.
    (@float $field:ident) => { NativeTy::Float };

    (
        $(#[$meta:meta])*
        $variant:ident = $rust:ty { $($field:ident),+ $(,)? }
    ) => {
        impl ScriptType for $rust {
            const TY: NativeTy = NativeTy::Engine(stringify!($variant));

            fn from_value(value: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
                match value {
                    Value::$variant(inner) => Ok(inner),
                    other => Err(NativeError::new(format!(
                        "an engine function expected a {} but the call supplied {}",
                        stringify!($variant),
                        other.type_name()
                    ))),
                }
            }

            fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
                Ok(Value::$variant(self))
            }
        }

        const _: () = {
            $(#[$meta])*
            static CONSTRUCTOR: NativeFn = NativeFn {
                name: stringify!($variant),
                params: &[$(ergon_type!(@float $field)),+],
                result: NativeTy::Engine(stringify!($variant)),
                cost: 1,
                variadic: false,
                call: |_, args| {
                    let [$($field),+] = args else {
                        return Err(NativeError::new(concat!(
                            "`", stringify!($variant), "` takes its components in order"
                        )));
                    };
                    Ok(Value::$variant(<$rust>::new(
                        $($field.as_float().ok_or_else(|| NativeError::new(concat!(
                            "`", stringify!($variant), "` takes numbers"
                        )))?),+
                    )))
                },
            };
            inventory::submit! { super::NativeRegistration(&CONSTRUCTOR) }
        };

        // One scope per accessor: the constructor above destructures its
        // arguments into bindings named after the fields, and a `static` sharing
        // a name with a `let` in the same scope is a compile error.
        $(
            const _: () = {
                static ACCESSOR: NativeFn = NativeFn {
                    // Unspellable on purpose: only the lowering of `v.x` finds it.
                    name: concat!(stringify!($variant), ".", stringify!($field)),
                    params: &[NativeTy::Engine(stringify!($variant))],
                    result: NativeTy::Float,
                    cost: 1,
                    variadic: false,
                    call: |_, args| match args {
                        [Value::$variant(inner)] => Ok(Value::Float(inner.$field)),
                        _ => Err(NativeError::new(concat!(
                            "`", stringify!($variant), ".", stringify!($field),
                            "` reads a ", stringify!($variant)
                        ))),
                    },
                };
                inventory::submit! { super::NativeRegistration(&ACCESSOR) }
            };
        )+
    };
}

ergon_type! {
    /// `Vec2(x, y)`
    Vec2 = khora_core::math::Vec2 { x, y }
}

ergon_type! {
    /// `Vec3(x, y, z)` — the one gameplay reaches for constantly.
    Vec3 = khora_core::math::Vec3 { x, y, z }
}

ergon_type! {
    /// `Vec4(x, y, z, w)`
    Vec4 = khora_core::math::Vec4 { x, y, z, w }
}

ergon_type! {
    /// `Quat(x, y, z, w)` — the components, not an axis and an angle. An author
    /// building a rotation by hand wants `FromAxisAngle`, which is a function
    /// rather than a constructor.
    Quat = khora_core::math::Quaternion { x, y, z, w }
}

ergon_type! {
    /// `Color(r, g, b, a)`, linear.
    Color = khora_core::math::LinearRgba { r, g, b, a }
}
