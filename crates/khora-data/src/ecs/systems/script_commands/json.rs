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

//! Turning a script's value into JSON — for the hierarchy alone.
//!
//! A script's component writes go through each component's typed column
//! operations, field by field, with no serialisation. `Parent` and `Children`
//! are the exception: the hierarchy module that owns both halves of an edge
//! takes its writes as JSON (`World::write_hierarchy_by_name`), the form the
//! editor hands it too.
//!
//! JSON has no NaN, and a script can produce one from `0.0 / 0.0`. Converting
//! it silently would write a `null` where a number belongs; [`to_json`]
//! refuses instead, and the fault names the field.

use khora_core::script::ScriptValue;
use serde_json::{Map, Value as Json};

/// How one table row becomes JSON.
///
/// Three rows answer for themselves. `Bool` and `Int` are cheaper as their own
/// JSON kinds than through serde, and a `Float` has to **refuse** a NaN with a
/// message naming the field — `serde_json` turns one into `null`, which
/// deserializes as a missing field and leaves the author hunting a write that
/// appeared to succeed. Everything else derives `Serialize`, so its JSON shape
/// is by construction the one the mirror expects.
///
/// Matching on the literal variant before the general arm is how
/// `ergon_type!` already spells an exception in a generated table.
macro_rules! json_of {
    (Bool, $inner:expr) => {
        Json::Bool(*$inner)
    };
    (Int, $inner:expr) => {
        Json::from(*$inner)
    };
    (Float, $inner:expr) => {
        number_from_float(*$inner)?
    };
    ($variant:ident, $inner:expr) => {
        encode($inner)?
    };
}

macro_rules! define_to_json {
    ($($variant:ident : $rust:ty ;)*) => {
        /// Converts a script value into JSON the component mirror can read.
        ///
        /// Generated from [`script_value_table`](khora_core::script_value_table)
        /// rather than listed. Six of the nine rows were spelled out here — the
        /// fifth conversion of the same universe, and the one the value bridge
        /// was written to remove. There is no `_` arm: a row added to the table
        /// has to break this file, not slip past it.
        pub(super) fn to_json(value: &ScriptValue) -> Result<Json, String> {
            Ok(match value {
                $(ScriptValue::$variant(inner) => json_of!($variant, inner),)*

                // ── Irregular ──────────────────────────────────────────────
                ScriptValue::Unit => Json::Null,
                ScriptValue::Str(text) => Json::String(text.clone()),
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
                ScriptValue::Null => Json::Null,
            })
        }
    };
}
khora_core::script_value_table!(define_to_json);

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
