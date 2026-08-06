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

//! The one place that says which values cross the script boundary.
//!
//! A value has three shapes — a register's [`Value`], a saved [`Persisted`], and
//! this crate's [`ScriptValue`] — because each answers a different pressure. The
//! shapes are justified; **translating between them four separate times was
//! not.**
//!
//! Four hand-written conversions once existed, in three crates, written weeks
//! apart. They diverged exactly where you would expect: one learned to carry a
//! `Vec3`, another never did. A script could raise an event holding a vector
//! that the delivery then refused — and the refusal read as a fault, which
//! disabled the receiving behavior for good.
//!
//! # An X-macro, and why
//!
//! This macro defines nothing. It **calls back** with the table, so each crate
//! generates the form it needs from one source. `khora-script` builds the
//! register and persistence bridges; the component writer builds its JSON arms.
//! Adding `Quat` is one line here instead of eight edits nobody can enumerate.
//!
//! # What is deliberately absent
//!
//! The irregular variants. `Str` is a reference in a register, owned in a store,
//! and owned again at the boundary — three different things. `Array` and
//! `Struct` have no register form at all, and `Null` has no boundary form.
//! Putting them here behind a "special" flag would move their logic into a
//! macro, where it reads worst. They stay hand-written arms, visible **as**
//! exceptions.
//!
//! [`Value`]: ../../../khora_script/vm/enum.Value.html
//! [`Persisted`]: ../../../khora_script/arena/enum.Persisted.html
//! [`ScriptValue`]: super::ScriptValue

/// Calls `$callback` with every value that maps one-to-one across all three
/// shapes.
///
/// Each row is `Variant : RustType ;` — the variant name is the *same* in
/// [`ScriptValue`](super::ScriptValue) and in the VM's `Value`, which is what
/// makes the generated arms mechanical.
///
/// # Example
///
/// ```ignore
/// macro_rules! widen {
///     ($($variant:ident : $rust:ty ;)*) => {
///         fn name_of(value: &ScriptValue) -> &'static str {
///             match value {
///                 $(ScriptValue::$variant(_) => stringify!($variant),)*
///                 _ => "irregular",
///             }
///         }
///     };
/// }
/// khora_core::script_value_table!(widen);
/// ```
#[macro_export]
macro_rules! script_value_table {
    ($callback:path) => {
        $callback! {
            Bool   : bool                          ;
            Int    : i64                           ;
            Float  : f32                           ;
            Entity : $crate::ecs::entity::EntityId ;
            Vec2   : $crate::math::Vec2            ;
            Vec3   : $crate::math::Vec3            ;
            Vec4   : $crate::math::Vec4            ;
            Quat   : $crate::math::Quaternion      ;
            Color  : $crate::math::LinearRgba      ;
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::script::ScriptValue;

    macro_rules! survey {
        ($($variant:ident : $rust:ty ;)*) => {
            /// The names the table carries.
            const NAMES: &[&str] = &[$(stringify!($variant),)*];

            /// Whether a value is one the table covers.
            ///
            /// The point of the test: this `match` only compiles if every row's
            /// variant really exists on `ScriptValue` and really holds a value.
            fn is_regular(value: &ScriptValue) -> bool {
                match value {
                    $(ScriptValue::$variant(_) => true,)*
                    _ => false,
                }
            }

            /// Uses the Rust column, so a row naming the wrong type fails to
            /// compile rather than passing silently.
            fn widths() -> Vec<usize> {
                vec![$(core::mem::size_of::<$rust>(),)*]
            }
        };
    }
    crate::script_value_table!(survey);

    /// **The table has to name every regular variant.** One left out is exactly
    /// how the four hand-written conversions diverged, so this asserts the count
    /// rather than trusting the eye.
    ///
    /// `ScriptValue` holds four irregulars — `Unit`, `Str`, `Array`, `Struct` —
    /// which the table deliberately excludes.
    #[test]
    fn the_table_covers_every_regular_variant() {
        assert_eq!(
            NAMES,
            ["Bool", "Int", "Float", "Entity", "Vec2", "Vec3", "Vec4", "Quat", "Color"]
        );
        assert_eq!(widths().len(), NAMES.len(), "one width per row");
    }

    /// And the irregulars stay out of it, on purpose.
    #[test]
    fn the_irregular_variants_are_not_in_the_table() {
        assert!(is_regular(&ScriptValue::Int(0)));
        assert!(is_regular(&ScriptValue::Vec3(crate::math::Vec3::ZERO)));

        assert!(!is_regular(&ScriptValue::Unit));
        assert!(!is_regular(&ScriptValue::Str(String::new())));
        assert!(!is_regular(&ScriptValue::Array(Vec::new())));
        assert!(!is_regular(&ScriptValue::Struct(Vec::new())));
    }
}
