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

//! Whether a saved value can go back into a slot of a declared type.
//!
//! A value is carried into a field **by name**, across a hot reload or a load
//! into edited code — and the edit may have changed the field's type. The
//! checker never lets an author put a `null` in an `int` or text in a `float`,
//! so a carried value must not either: one that no longer fits is left behind,
//! and the field takes its default.

use khora_core::script::ScriptValue;

/// Whether `value` can be held by a slot declared `ty` (as written, `int?`,
/// `Vec3`, `string[]`).
///
/// An `int` fits a float-typed slot — the VM widens one where a float is read.
/// A type the value's kind cannot tell apart — a struct — is taken on trust,
/// and so is a slot whose type is not known (a layout from before types were
/// recorded). `Unit` is "nothing written" and fits anything.
pub fn fits(value: &ScriptValue, ty: &str) -> bool {
    if ty.is_empty() || matches!(value, ScriptValue::Unit) {
        return true;
    }
    if let Some(inner) = ty.strip_suffix('?') {
        return matches!(value, ScriptValue::Null) || fits(value, inner);
    }
    if ty.ends_with("[]") {
        return matches!(value, ScriptValue::Array(_));
    }
    match ty {
        "int" => matches!(value, ScriptValue::Int(_)),
        "float" | "Duration" | "Angle" => {
            matches!(value, ScriptValue::Float(_) | ScriptValue::Int(_))
        }
        "bool" => matches!(value, ScriptValue::Bool(_)),
        "string" => matches!(value, ScriptValue::Str(_)),
        "Entity" => matches!(value, ScriptValue::Entity(_)),
        "Vec2" => matches!(value, ScriptValue::Vec2(_)),
        "Vec3" => matches!(value, ScriptValue::Vec3(_)),
        "Vec4" => matches!(value, ScriptValue::Vec4(_)),
        "Quat" => matches!(value, ScriptValue::Quat(_)),
        "Color" => matches!(value, ScriptValue::Color(_)),
        _ => !matches!(value, ScriptValue::Null),
    }
}
