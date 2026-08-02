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

//! Turning a script's value into component data.
//!
//! The bridge to `#[derive(Component)]`. The derive already emits `to_json` and
//! `from_json` against the generated `SerializableX` mirror, which is how the
//! editor's inspector commits a field edit without per-type code. A script write
//! is the same operation from a different keyboard, so it takes the same road
//! rather than laying a second one.
//!
//! Two things the road does not do on its own:
//!
//! - `from_json` deserializes the **whole** mirror, so a partial write has to be
//!   merged onto what the component already holds — see [`merge`]. Without it
//!   `health.current = 50` would mean "reset every other field to nothing".
//! - JSON has no NaN, and a script can produce one from `0.0 / 0.0`. Converting
//!   it silently would write a `null` where a number belongs; [`to_json`]
//!   refuses instead, and the fault names the field.

use khora_core::script::ScriptValue;
use serde_json::{Map, Value as Json};

/// Converts a script value into JSON the component mirror can read.
pub(super) fn to_json(value: &ScriptValue) -> Result<Json, String> {
    Ok(match value {
        ScriptValue::Unit => Json::Null,
        ScriptValue::Bool(flag) => Json::Bool(*flag),
        ScriptValue::Int(number) => Json::from(*number),
        ScriptValue::Float(number) => number_from_float(*number)?,
        ScriptValue::Str(text) => Json::String(text.clone()),
        // The math types and `EntityId` derive `Serialize`, so their JSON shape
        // is by construction the one the mirror expects — hand-writing it here
        // would be a second definition free to drift from the first.
        ScriptValue::Vec2(v) => encode(v)?,
        ScriptValue::Vec3(v) => encode(v)?,
        ScriptValue::Vec4(v) => encode(v)?,
        ScriptValue::Quat(q) => encode(q)?,
        ScriptValue::Color(c) => encode(c)?,
        ScriptValue::Entity(id) => encode(id)?,
        ScriptValue::Array(values) => Json::Array(
            values
                .iter()
                .map(to_json)
                .collect::<Result<Vec<_>, String>>()?,
        ),
        ScriptValue::Struct(fields) => {
            let mut object = Map::with_capacity(fields.len());
            for (name, field) in fields {
                object.insert(name.clone(), to_json(field)?);
            }
            Json::Object(object)
        }
    })
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Json, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// JSON numbers are finite, so a NaN or an infinity is refused rather than
/// quietly becoming `null` — which would deserialize as a missing field and
/// leave the author looking for a write that appeared to succeed.
fn number_from_float(number: f32) -> Result<Json, String> {
    serde_json::Number::from_f64(number as f64)
        .map(Json::Number)
        .ok_or_else(|| format!("{number} cannot be written to a component"))
}

/// Merges `patch` onto `base`, field by field.
///
/// Recursive on objects and replacing everywhere else: writing one field of a
/// nested struct leaves its siblings alone, while writing an array replaces it
/// whole. Element-wise array merging would make `waypoints = [a, b]` on a
/// four-element list keep the last two, which is not what the assignment says.
pub(super) fn merge(base: Json, patch: Json) -> Json {
    match (base, patch) {
        (Json::Object(mut base), Json::Object(patch)) => {
            for (key, value) in patch {
                let merged = match base.remove(&key) {
                    Some(existing) => merge(existing, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            Json::Object(base)
        }
        (_, patch) => patch,
    }
}
