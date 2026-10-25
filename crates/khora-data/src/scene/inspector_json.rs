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

//! A component as the editor's inspector reads and writes it: JSON, with the
//! floats JSON has no number for kept.
//!
//! JSON writes an infinity or a NaN as `null`, and reads `null` back as no
//! float at all — so a component holding one (a camera loaded with an
//! infinite far plane) could be neither shown nor edited, not even to fix it.
//! Such a float is written as the marker the self-describing record form
//! already uses, `{"$float": "inf"}` (`"-inf"`, `"nan"`), and read back from
//! it. Every other value is plain `serde_json`.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use super::record::{
    from_record, to_record, EntityRef, Record, RecordError, ReferenceReader, ReferenceWriter,
    VariantPayload,
};

/// The key that marks a float JSON has no number for.
const FLOAT_KEY: &str = "$float";

/// `value` as the inspector's JSON; `None` when it cannot be written.
pub fn to_json<S: Serialize>(value: &S) -> Option<Value> {
    let mut json = serde_json::to_value(value).ok()?;
    // The record keeps every float as it is: it says which `null`s were one.
    if let Ok(record) = to_record(value, &mut Entities) {
        mark(&mut json, &record);
    }
    Some(json)
}

/// Reads an `S` from the inspector's JSON, `{"$float": …}` markers included.
pub fn from_json<S: Serialize + DeserializeOwned>(json: &Value) -> Result<S, String> {
    let mut floats = Vec::new();
    let plain = unmark(json, &mut Vec::new(), &mut floats)?;
    let value: S = serde_json::from_value(plain).map_err(|error| error.to_string())?;
    if floats.is_empty() {
        return Ok(value);
    }
    // Read with a stand-in number where each marker was, then put the real
    // float back where the record holds it.
    let mut record = to_record(&value, &mut Entities).map_err(|RecordError(error)| error)?;
    for (path, float) in floats {
        if !set_float(&mut record, &path, float) {
            return Err(format!(
                "`{}` holds a `$float` marker but is not a float",
                path_text(&path)
            ));
        }
    }
    from_record(&record, &mut Entities).map_err(|RecordError(error)| error)
}

/// One step into a JSON value: an object's key or an array's index.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Index(usize),
}

/// Writes the marker wherever `json` holds `null` and `record` a float with
/// no JSON number.
fn mark(json: &mut Value, record: &Record) {
    let record = transparent(record);
    if json.is_null() {
        if let Some(text) = non_finite(record) {
            *json = serde_json::json!({ FLOAT_KEY: text });
        }
        return;
    }
    match (json, record) {
        (Value::Object(map), Record::Struct { fields, .. }) => {
            for (name, field) in fields {
                if let Some(json) = map.get_mut(name) {
                    mark(json, field);
                }
            }
        }
        (Value::Object(map), Record::Map(entries)) => {
            for (key, entry) in entries {
                if let Record::Str(key) = transparent(key) {
                    if let Some(json) = map.get_mut(key) {
                        mark(json, entry);
                    }
                }
            }
        }
        (
            Value::Object(map),
            Record::Variant {
                variant, payload, ..
            },
        ) => {
            let Some(json) = map.get_mut(variant) else {
                return;
            };
            match payload {
                VariantPayload::Unit => {}
                VariantPayload::Newtype(inner) => mark(json, inner),
                VariantPayload::Tuple(items) => mark_items(json, items),
                VariantPayload::Struct(fields) => {
                    if let Value::Object(map) = json {
                        for (name, field) in fields {
                            if let Some(json) = map.get_mut(name) {
                                mark(json, field);
                            }
                        }
                    }
                }
            }
        }
        (
            json @ Value::Array(_),
            Record::Seq(items) | Record::TupleStruct { fields: items, .. },
        ) => mark_items(json, items),
        _ => {}
    }
}

fn mark_items(json: &mut Value, items: &[Record]) {
    if let Value::Array(values) = json {
        for (json, item) in values.iter_mut().zip(items) {
            mark(json, item);
        }
    }
}

/// Through the wrappers JSON does not show: a present optional, a newtype.
fn transparent(record: &Record) -> &Record {
    match record {
        Record::Some(inner) => transparent(inner),
        Record::Newtype { value, .. } => transparent(value),
        other => other,
    }
}

/// The marker text of a float with no JSON number.
fn non_finite(record: &Record) -> Option<&'static str> {
    let value = match *record {
        Record::F32(v) => f64::from(v),
        Record::F64(v) | Record::Decimal(v) => v,
        _ => return None,
    };
    if value.is_nan() {
        Some("nan")
    } else if value == f64::INFINITY {
        Some("inf")
    } else if value == f64::NEG_INFINITY {
        Some("-inf")
    } else {
        None
    }
}

/// `json` with every marker replaced by `0`, each marker's path and float
/// collected in `floats`.
fn unmark(
    json: &Value,
    path: &mut Vec<Step>,
    floats: &mut Vec<(Vec<Step>, f64)>,
) -> Result<Value, String> {
    match json {
        Value::Object(map) if map.len() == 1 && map.contains_key(FLOAT_KEY) => {
            let float = match map.get(FLOAT_KEY).and_then(Value::as_str) {
                Some("inf") => f64::INFINITY,
                Some("-inf") => f64::NEG_INFINITY,
                Some("nan") => f64::NAN,
                _ => {
                    return Err(format!(
                        "`{}`: a `$float` marker must hold \"inf\", \"-inf\" or \"nan\"",
                        path_text(path)
                    ))
                }
            };
            floats.push((path.clone(), float));
            Ok(Value::from(0.0))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, value) in map {
                path.push(Step::Key(key.clone()));
                let value = unmark(value, path, floats);
                path.pop();
                out.insert(key.clone(), value?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (index, value) in items.iter().enumerate() {
                path.push(Step::Index(index));
                let value = unmark(value, path, floats);
                path.pop();
                out.push(value?);
            }
            Ok(Value::Array(out))
        }
        other => Ok(other.clone()),
    }
}

/// Sets the float at the JSON `path` of `record`; `false` when no float is
/// there.
fn set_float(record: &mut Record, path: &[Step], float: f64) -> bool {
    match record {
        Record::Some(inner) => return set_float(inner, path, float),
        Record::Newtype { value, .. } => return set_float(value, path, float),
        _ => {}
    }
    let Some((step, rest)) = path.split_first() else {
        return match record {
            Record::F32(v) => {
                *v = float as f32;
                true
            }
            Record::F64(v) | Record::Decimal(v) => {
                *v = float;
                true
            }
            _ => false,
        };
    };
    match (record, step) {
        (Record::Struct { fields, .. }, Step::Key(key)) => set_in_fields(fields, key, rest, float),
        (Record::Map(entries), Step::Key(key)) => entries
            .iter_mut()
            .find(|(k, _)| matches!(transparent(k), Record::Str(k) if k == key))
            .is_some_and(|(_, value)| set_float(value, rest, float)),
        (Record::Seq(items) | Record::TupleStruct { fields: items, .. }, Step::Index(index)) => {
            items
                .get_mut(*index)
                .is_some_and(|item| set_float(item, rest, float))
        }
        (
            Record::Variant {
                variant, payload, ..
            },
            Step::Key(key),
        ) if variant == key => match (payload, rest.split_first()) {
            (VariantPayload::Newtype(inner), _) => set_float(inner, rest, float),
            (VariantPayload::Tuple(items), Some((Step::Index(index), rest))) => items
                .get_mut(*index)
                .is_some_and(|item| set_float(item, rest, float)),
            (VariantPayload::Struct(fields), Some((Step::Key(key), rest))) => {
                set_in_fields(fields, key, rest, float)
            }
            _ => false,
        },
        _ => false,
    }
}

fn set_in_fields(fields: &mut [(String, Record)], key: &str, rest: &[Step], float: f64) -> bool {
    fields
        .iter_mut()
        .find(|(name, _)| name == key)
        .is_some_and(|(_, field)| set_float(field, rest, float))
}

/// `projection.Perspective.fov_y_radians`, `points.2.x`.
fn path_text(path: &[Step]) -> String {
    path.iter()
        .map(|step| match step {
            Step::Key(key) => key.clone(),
            Step::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Entities written into the record and read back as themselves: the record
/// here never leaves the value it was made from.
struct Entities;

impl ReferenceWriter for Entities {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        let bits = (u64::from(entity.generation) << 32) | u64::from(entity.index);
        EntityRef::Id(PersistentId::from_bits(bits))
    }
}

impl ReferenceReader for Entities {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        match reference {
            EntityRef::Id(id) => {
                let bits = id.to_bits();
                Ok(EntityId {
                    index: bits as u32,
                    generation: (bits >> 32) as u32,
                })
            }
            EntityRef::Outside => Err(RecordError(
                "an entity outside the value it was written from".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum Shape {
        Ball { radius: f32 },
        Pair(f32, f64),
        Flat,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Sample {
        near: f32,
        far: f32,
        shape: Shape,
        list: Vec<f32>,
        maybe: Option<f64>,
        owner: Option<EntityId>,
    }

    fn sample() -> Sample {
        Sample {
            near: 0.1,
            far: 1000.0,
            shape: Shape::Ball { radius: 2.0 },
            list: vec![1.0, 2.0],
            maybe: Some(3.0),
            owner: Some(EntityId {
                index: 7,
                generation: 2,
            }),
        }
    }

    fn same(a: &Sample, b: &Sample) -> bool {
        // NaN != NaN: compare bit patterns through the debug form.
        format!("{a:?}") == format!("{b:?}")
    }

    /// A value whose floats are all finite is plain `serde_json`, both ways.
    #[test]
    fn a_finite_value_is_plain_json() {
        let value = sample();
        let json = to_json(&value).expect("writes");
        assert_eq!(json, serde_json::to_value(&value).unwrap());
        assert_eq!(from_json::<Sample>(&json).expect("reads"), value);
    }

    /// Infinities and NaNs anywhere — a field, a variant's field, a tuple
    /// variant, a list, an optional — come back as they were.
    #[test]
    fn non_finite_floats_round_trip_wherever_they_are() {
        let mut value = sample();
        value.far = f32::INFINITY;
        value.near = f32::NAN;
        value.list = vec![f32::NEG_INFINITY, 1.0];
        value.maybe = Some(f64::INFINITY);
        let json = to_json(&value).expect("writes");
        assert_eq!(json["far"], serde_json::json!({ "$float": "inf" }));
        assert_eq!(json["near"], serde_json::json!({ "$float": "nan" }));
        assert_eq!(json["list"][0], serde_json::json!({ "$float": "-inf" }));
        let back: Sample = from_json(&json).expect("reads");
        assert!(same(&back, &value), "{back:?} vs {value:?}");

        for shape in [
            Shape::Ball {
                radius: f32::INFINITY,
            },
            Shape::Pair(1.0, f64::NAN),
        ] {
            let value = Sample {
                shape: shape.clone(),
                ..sample()
            };
            let back: Sample = from_json(&to_json(&value).unwrap()).expect("reads");
            assert!(same(&back, &value), "{back:?} vs {value:?}");
        }
    }

    /// An edit of another field keeps a non-finite float as it was.
    #[test]
    fn an_edit_keeps_the_other_non_finite_floats() {
        let mut value = sample();
        value.far = f32::INFINITY;
        let mut json = to_json(&value).unwrap();
        json["near"] = serde_json::json!(0.5);
        let back: Sample = from_json(&json).expect("reads");
        assert_eq!(back.near, 0.5);
        assert_eq!(back.far, f32::INFINITY);
    }

    /// A marker can replace a number: an edit may write an infinity.
    #[test]
    fn a_marker_written_by_an_edit_is_read() {
        let mut json = to_json(&sample()).unwrap();
        json["far"] = serde_json::json!({ "$float": "inf" });
        let back: Sample = from_json(&json).expect("reads");
        assert_eq!(back.far, f32::INFINITY);
    }

    /// A marker that is not one of the three texts, or that stands where no
    /// float is, is an error naming the field — never a silent default.
    #[test]
    fn a_wrong_marker_is_an_error_naming_its_field() {
        let mut json = to_json(&sample()).unwrap();
        json["far"] = serde_json::json!({ "$float": "huge" });
        let error = from_json::<Sample>(&json).unwrap_err();
        assert!(error.contains("far"), "{error}");

        let mut json = to_json(&sample()).unwrap();
        json["shape"] = serde_json::json!("Flat");
        json["owner"] = serde_json::json!({ "index": { "$float": "inf" }, "generation": 2 });
        assert!(from_json::<Sample>(&json).is_err());
    }
}
