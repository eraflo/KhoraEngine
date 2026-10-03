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

//! A record in the form self-describing formats write: JSON, MessagePack.
//!
//! Structs become maps keyed by field name, a variant its name or a map keyed
//! by it, an absent optional `null`, a present one its bare value. The two
//! kinds of reference keep a marker key no field is named — `$entity`,
//! `$asset` — so reading the text back recovers them for what they are. A
//! float that text cannot write as a number — an infinity or a NaN — is kept
//! the same way, under `$float`.
//!
//! Struct and enum names are not written: the reader matches by field and
//! variant name, and a self-describing format has nowhere natural for them.

use std::fmt;

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{EntityRef, Record, VariantPayload, MAX_DEPTH};

/// The key that marks an entity reference.
const ENTITY_KEY: &str = "$entity";

/// The key that marks an asset reference.
const ASSET_KEY: &str = "$asset";

/// The key that marks a float a text format has no number for: an infinity
/// or a NaN, which JSON would otherwise write as `null`.
const FLOAT_KEY: &str = "$float";

impl Serialize for Record {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Unit => serializer.serialize_unit(),
            Self::Bool(v) => serializer.serialize_bool(*v),
            Self::I64(v) => serializer.serialize_i64(*v),
            Self::U64(v) => serializer.serialize_u64(*v),
            Self::F32(v) => serialize_float(serializer, f64::from(*v), |s| s.serialize_f32(*v)),
            Self::F64(v) | Self::Decimal(v) => {
                serialize_float(serializer, *v, |s| s.serialize_f64(*v))
            }
            Self::Char(v) => serializer.serialize_char(*v),
            Self::Str(v) => serializer.serialize_str(v),
            Self::Bytes(v) => serializer.serialize_bytes(v),
            Self::None => serializer.serialize_none(),
            Self::Some(inner) => serializer.serialize_some(inner.as_ref()),
            Self::Seq(items) | Self::TupleStruct { fields: items, .. } => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Self::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            // An empty map rather than `null`: `null` already means an absent
            // optional, and a marker type is present.
            Self::UnitStruct { .. } => serializer.serialize_map(Some(0))?.end(),
            Self::Struct { fields, .. } => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    map.serialize_entry(name, value)?;
                }
                map.end()
            }
            Self::Newtype { value, .. } => value.serialize(serializer),
            Self::Variant {
                variant, payload, ..
            } => match payload {
                VariantPayload::Unit => serializer.serialize_str(variant),
                VariantPayload::Newtype(inner) => {
                    let mut map = serializer.serialize_map(Some(1))?;
                    map.serialize_entry(variant, inner.as_ref())?;
                    map.end()
                }
                VariantPayload::Tuple(items) => {
                    let mut map = serializer.serialize_map(Some(1))?;
                    map.serialize_entry(variant, &Items(items))?;
                    map.end()
                }
                VariantPayload::Struct(fields) => {
                    let mut map = serializer.serialize_map(Some(1))?;
                    map.serialize_entry(variant, &Named(fields))?;
                    map.end()
                }
            },
            Self::Entity(reference) => {
                let mut map = serializer.serialize_map(Some(1))?;
                match reference {
                    EntityRef::Id(id) => map.serialize_entry(ENTITY_KEY, &id.to_bits())?,
                    EntityRef::Outside => map.serialize_entry(ENTITY_KEY, &())?,
                }
                map.end()
            }
            Self::Asset(uuid) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(ASSET_KEY, &uuid.to_string())?;
                map.end()
            }
        }
    }
}

/// A tuple variant's values, as a list.
struct Items<'a>(&'a [Record]);

impl Serialize for Items<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            seq.serialize_element(item)?;
        }
        seq.end()
    }
}

/// A struct variant's fields, as a map.
struct Named<'a>(&'a [(String, Record)]);

impl Serialize for Named<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0 {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        RecordSeed { depth: 0 }.deserialize(deserializer)
    }
}

/// Reads one value `depth` levels into another. A file is input: however
/// deep it nests, reading it is an error past [`MAX_DEPTH`], never a stack
/// overflow — whatever bound the format's own reader has, or lacks.
struct RecordSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for RecordSeed {
    type Value = Record;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Record, D::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom(format!(
                "a value nests deeper than {MAX_DEPTH} levels"
            )));
        }
        // A format people read writes numbers as decimal text; one that is
        // not, writes them at their width.
        let text = deserializer.is_human_readable();
        deserializer.deserialize_any(RecordVisitor {
            text,
            depth: self.depth,
        })
    }
}

struct RecordVisitor {
    /// Whether the format writes numbers as decimal text.
    text: bool,
    /// How deep the value being read is.
    depth: usize,
}

impl RecordVisitor {
    /// The reader of a value one level inside this one.
    fn inner(&self) -> RecordSeed {
        RecordSeed {
            depth: self.depth + 1,
        }
    }
}

impl<'de> Visitor<'de> for RecordVisitor {
    type Value = Record;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a saved value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Record, E> {
        Ok(Record::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Record, E> {
        Ok(Record::I64(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Record, E> {
        Ok(Record::U64(v))
    }
    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Record, E> {
        Ok(Record::F32(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Record, E> {
        Ok(if self.text {
            Record::Decimal(v)
        } else {
            Record::F64(v)
        })
    }
    fn visit_char<E: de::Error>(self, v: char) -> Result<Record, E> {
        Ok(Record::Char(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Record, E> {
        Ok(Record::Str(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Record, E> {
        Ok(Record::Str(v))
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Record, E> {
        Ok(Record::Bytes(v.to_vec()))
    }
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Record, E> {
        Ok(Record::Bytes(v))
    }
    fn visit_none<E: de::Error>(self) -> Result<Record, E> {
        Ok(Record::None)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Record, E> {
        Ok(Record::None)
    }
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Record, D::Error> {
        self.inner().deserialize(deserializer)
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Record, D::Error> {
        self.inner().deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Record, A::Error> {
        // A length hint is the input's claim, so it only caps the first
        // allocation; the list grows with what is actually there.
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
        while let Some(item) = seq.next_element_seed(self.inner())? {
            items.push(item);
        }
        Ok(Record::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record, A::Error> {
        let mut entries: Vec<(Record, Record)> =
            Vec::with_capacity(map.size_hint().unwrap_or(0).min(1024));
        while let Some(key) = map.next_key_seed(self.inner())? {
            let value = map.next_value_seed(self.inner())?;
            entries.push((key, value));
        }
        if let [(Record::Str(key), value)] = entries.as_slice() {
            if key == ENTITY_KEY {
                return entity_marker(value).map(Record::Entity);
            }
            if key == ASSET_KEY {
                return asset_marker(value).map(Record::Asset);
            }
            if key == FLOAT_KEY {
                let value = float_marker(value)?;
                return Ok(if self.text {
                    Record::Decimal(value)
                } else {
                    Record::F64(value)
                });
            }
        }
        Ok(Record::Map(entries))
    }
}

/// The float a `$float` marker holds.
fn float_marker<E: de::Error>(value: &Record) -> Result<f64, E> {
    match value {
        Record::Str(text) if text == "inf" => Ok(f64::INFINITY),
        Record::Str(text) if text == "-inf" => Ok(f64::NEG_INFINITY),
        Record::Str(text) if text == "nan" => Ok(f64::NAN),
        _ => Err(E::custom(
            "a `$float` marker must hold \"inf\", \"-inf\" or \"nan\"",
        )),
    }
}

/// How a self-describing text format writes a float it has no number for.
fn non_finite_name(value: f64) -> Option<&'static str> {
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

/// Writes `value` as a number, or — when the format is text and has no
/// number for it — as a `$float` marker.
fn serialize_float<S: Serializer>(
    serializer: S,
    value: f64,
    write: impl FnOnce(S) -> Result<S::Ok, S::Error>,
) -> Result<S::Ok, S::Error> {
    match non_finite_name(value) {
        Some(name) if serializer.is_human_readable() => {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry(FLOAT_KEY, name)?;
            map.end()
        }
        _ => write(serializer),
    }
}

/// The reference an `$entity` marker holds.
fn entity_marker<E: de::Error>(value: &Record) -> Result<EntityRef, E> {
    match value {
        Record::None | Record::Unit => Ok(EntityRef::Outside),
        Record::U64(bits) => Ok(EntityRef::Id(PersistentId::from_bits(*bits))),
        Record::I64(bits) => u64::try_from(*bits)
            .map(|bits| EntityRef::Id(PersistentId::from_bits(bits)))
            .map_err(|_| E::custom("an `$entity` marker with a negative identity")),
        _ => Err(E::custom(
            "an `$entity` marker must hold an identity or null",
        )),
    }
}

/// The identity an `$asset` marker holds.
fn asset_marker<E: de::Error>(value: &Record) -> Result<AssetUUID, E> {
    match value {
        Record::Str(text) => text
            .parse::<AssetUUID>()
            .map_err(|error| E::custom(format!("an `$asset` marker with a bad UUID: {error}"))),
        _ => Err(E::custom("an `$asset` marker must hold a UUID string")),
    }
}
