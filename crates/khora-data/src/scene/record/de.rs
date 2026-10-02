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

//! Reading a value back from a record.

use khora_core::ecs::entity::EntityId;
use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeOwned, Visitor};

use super::access::{Elements, Entries, Fields, OneVariant, Payload, Variant};
use super::number::Number;
use super::watch::{Blind, Event, Step, Watcher};
use super::{EntityRef, Record, RecordError, VariantPayload, ASSET_NAME, ENTITY_NAME, MAX_DEPTH};

/// Turns the references a save kept into entities of the world being loaded.
pub trait ReferenceReader {
    /// The entity `reference` stands for in the world being loaded.
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError>;
}

/// Reads a `T` from `record`, matching fields by name.
///
/// A field the record lacks takes the type's serde default, a field the type
/// no longer has is ignored, a renamed one is read through its serde alias, and
/// a number is widened when that loses nothing. Anything else is an error, never
/// a panic: a record comes from a file, and a file is input.
pub fn from_record<T: DeserializeOwned>(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<T, RecordError> {
    read_watched(record, references, &mut Blind)
}

/// [`from_record`], noting what the read did on `watch`.
pub(super) fn read_watched<T: DeserializeOwned>(
    record: &Record,
    references: &mut dyn ReferenceReader,
    watch: &mut dyn Watcher,
) -> Result<T, RecordError> {
    check_depth(record)?;
    let mut cx = Cx { references, watch };
    T::deserialize(Reader {
        record,
        cx: &mut cx,
        node: 0,
    })
}

/// Refuses a record nested deeper than any value the engine persists.
///
/// Checked once, up front and without recursion, so neither the read nor a
/// type that skips what it does not need can be walked into exhausting the
/// stack.
fn check_depth(record: &Record) -> Result<(), RecordError> {
    let mut pending = vec![(record, 0usize)];
    while let Some((record, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err(RecordError(format!(
                "a record nests deeper than {MAX_DEPTH} levels"
            )));
        }
        let below = depth + 1;
        match record {
            Record::Some(inner) | Record::Newtype { value: inner, .. } => {
                pending.push((inner, below));
            }
            Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
                pending.extend(items.iter().map(|item| (item, below)));
            }
            Record::Map(entries) => {
                for (key, value) in entries {
                    pending.push((key, below));
                    pending.push((value, below));
                }
            }
            Record::Struct { fields, .. } => {
                pending.extend(fields.iter().map(|(_, value)| (value, below)));
            }
            Record::Variant { payload, .. } => match payload {
                VariantPayload::Unit => {}
                VariantPayload::Newtype(inner) => pending.push((inner, below)),
                VariantPayload::Tuple(items) => {
                    pending.extend(items.iter().map(|item| (item, below)));
                }
                VariantPayload::Struct(fields) => {
                    pending.extend(fields.iter().map(|(_, value)| (value, below)));
                }
            },
            _ => {}
        }
    }
    Ok(())
}

/// What every nested reader shares: the references to resolve and the
/// watcher to tell.
pub(super) struct Cx<'r> {
    pub(super) references: &'r mut dyn ReferenceReader,
    pub(super) watch: &'r mut dyn Watcher,
}

/// The deserializer over one record.
pub(super) struct Reader<'a, 'c, 'r> {
    pub(super) record: &'a Record,
    pub(super) cx: &'c mut Cx<'r>,
    /// This value's node on the watcher.
    pub(super) node: usize,
}

impl Reader<'_, '_, '_> {
    fn wrong(&self, expected: &str) -> RecordError {
        RecordError(format!("expected {expected}, found {}", kind(self.record)))
    }

    fn note(&mut self, event: Event) {
        self.cx.watch.note(self.node, event);
    }
}

/// A short name for a record's kind, for error messages.
fn kind(record: &Record) -> &'static str {
    match record {
        Record::Unit => "a unit",
        Record::Bool(_) => "a bool",
        Record::I64(_) | Record::U64(_) => "an integer",
        Record::F32(_) | Record::F64(_) => "a float",
        Record::Char(_) => "a char",
        Record::Str(_) => "a string",
        Record::Bytes(_) => "bytes",
        Record::None | Record::Some(_) => "an optional",
        Record::Seq(_) => "a sequence",
        Record::Map(_) => "a map",
        Record::UnitStruct { .. } => "a unit struct",
        Record::Struct { .. } => "a struct",
        Record::TupleStruct { .. } => "a tuple struct",
        Record::Newtype { .. } => "a newtype",
        Record::Variant { .. } => "an enum variant",
        Record::Entity(_) => "an entity reference",
        Record::Asset(_) => "an asset reference",
    }
}

impl<'a, 'c, 'r> Reader<'a, 'c, 'r> {
    /// A reader over a value inside this one, reached by `step`.
    fn below<'s>(&'s mut self, record: &'a Record, step: Option<Step>) -> Reader<'a, 's, 'r> {
        let node = match step {
            Some(step) => self.cx.watch.child(self.node, step),
            None => self.node,
        };
        Reader {
            record,
            cx: &mut *self.cx,
            node,
        }
    }

    fn number(&self) -> Result<Number, RecordError> {
        Number::of(self.record).ok_or_else(|| self.wrong("a number"))
    }

    /// Reads an integer, handing it to the visitor at a width that fits so
    /// serde's own range check decides whether the target can hold it.
    fn visit_integer<'de, V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, RecordError> {
        let number = self.number()?;
        let value = number.integer().ok_or_else(|| {
            RecordError(format!(
                "{} cannot be read as an integer without losing its value",
                kind(self.record)
            ))
        })?;
        if number.is_float() {
            self.note(Event::Widened);
        }
        if let Ok(signed) = i64::try_from(value) {
            visitor.visit_i64(signed)
        } else if let Ok(unsigned) = u64::try_from(value) {
            visitor.visit_u64(unsigned)
        } else {
            Err(RecordError(format!(
                "{value} does not fit any integer field"
            )))
        }
    }

    fn entity(self) -> Result<EntityId, RecordError> {
        match *self.record {
            Record::Entity(reference) => self.cx.references.read_entity(reference),
            _ => Err(self.wrong("an entity reference")),
        }
    }

    /// Reads a sequence, refusing one longer than the target takes: a
    /// fixed-size value that stops early would drop the rest in silence.
    pub(super) fn visit_elements<'de, V: Visitor<'de>>(
        self,
        items: &'a [Record],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        let mut elements = Elements::new(items, self.cx, self.node);
        let value = visitor.visit_seq(&mut elements)?;
        match elements.items.len() {
            0 => Ok(value),
            left => Err(RecordError(format!(
                "{left} element(s) more than the code reads"
            ))),
        }
    }

    fn visit_enum_named<'de, V: Visitor<'de>>(
        self,
        name: &'a str,
        payload: Payload<'a>,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        let node = self
            .cx
            .watch
            .child(self.node, Step::Variant(name.to_owned()));
        visitor.visit_enum(Variant {
            name,
            payload,
            cx: self.cx,
            node,
        })
    }
}

/// An entity, presented the way `EntityId` deserializes: its two numbers.
fn entity_fields(
    entity: EntityId,
) -> MapDeserializer<'static, std::array::IntoIter<(&'static str, u32), 2>, RecordError> {
    MapDeserializer::new([("index", entity.index), ("generation", entity.generation)].into_iter())
}

/// An asset identity, presented as the sixteen bytes it serializes to.
struct AssetBytes([u8; 16]);

impl<'de> de::Deserializer<'de> for AssetBytes {
    type Error = RecordError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_bytes(&self.0)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de> de::Deserializer<'de> for Reader<'_, '_, '_> {
    type Error = RecordError;

    /// Matches the serializer, so a type with a human and a compact form
    /// reads the compact one it was written in.
    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, RecordError> {
        let record = self.record;
        match record {
            Record::Unit | Record::UnitStruct { .. } => visitor.visit_unit(),
            Record::Bool(v) => visitor.visit_bool(*v),
            Record::I64(v) => visitor.visit_i64(*v),
            Record::U64(v) => visitor.visit_u64(*v),
            Record::F32(v) => visitor.visit_f32(*v),
            Record::F64(v) => visitor.visit_f64(*v),
            Record::Char(v) => visitor.visit_char(*v),
            Record::Str(v) => visitor.visit_str(v),
            Record::Bytes(v) => visitor.visit_bytes(v),
            Record::None => visitor.visit_none(),
            Record::Some(inner) => visitor.visit_some(self.below(inner, None)),
            Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
                self.visit_elements(items, visitor)
            }
            Record::Map(entries) => visitor.visit_map(Entries::new(entries, self.cx, self.node)),
            Record::Struct { fields, .. } => {
                visitor.visit_map(Fields::new(fields, self.cx, self.node))
            }
            Record::Newtype { value, .. } => visitor.visit_newtype_struct(self.below(value, None)),
            // Presented the way a self-describing format writes a variant — its
            // name, or its name keying what it carries — because a buffering
            // type cannot take a variant as such.
            Record::Variant {
                variant,
                payload: VariantPayload::Unit,
                ..
            } => visitor.visit_str(variant),
            Record::Variant {
                variant, payload, ..
            } => visitor.visit_map(OneVariant {
                name: Some(variant),
                payload,
                cx: self.cx,
                node: self.node,
            }),
            // Through a buffering type the target is not known yet: hand over
            // what `EntityId` and `AssetUUID` read from.
            Record::Entity(_) => {
                let entity = self.entity()?;
                visitor.visit_map(entity_fields(entity))
            }
            Record::Asset(uuid) => visitor.visit_newtype_struct(AssetBytes(*uuid.as_bytes())),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Bool(v) => visitor.visit_bool(*v),
            _ => Err(self.wrong("a bool")),
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.visit_integer(visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, RecordError> {
        let number = self.number()?;
        let value = number.single().ok_or_else(|| {
            RecordError(format!(
                "{} cannot be read as an f32 without losing precision",
                kind(self.record)
            ))
        })?;
        if !matches!(number, Number::Single(_)) {
            self.note(Event::Widened);
        }
        visitor.visit_f32(value)
    }

    fn deserialize_f64<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, RecordError> {
        let number = self.number()?;
        let value = number.double().ok_or_else(|| {
            RecordError(format!(
                "{} cannot be read as an f64 without losing precision",
                kind(self.record)
            ))
        })?;
        if !matches!(number, Number::Double(_)) {
            self.note(Event::Widened);
        }
        visitor.visit_f64(value)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Char(v) => visitor.visit_char(*v),
            Record::Str(v) => visitor.visit_str(v),
            _ => Err(self.wrong("a char")),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Str(v) => visitor.visit_str(v),
            Record::Char(v) => visitor.visit_char(*v),
            _ => Err(self.wrong("a string")),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Bytes(v) => visitor.visit_bytes(v),
            Record::Seq(items) => self.visit_elements(items, visitor),
            _ => Err(self.wrong("bytes")),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, RecordError> {
        let record = self.record;
        match record {
            Record::None | Record::Unit => visitor.visit_none(),
            Record::Some(inner) => visitor.visit_some(self.below(inner, None)),
            // A format that writes a present optional as the bare value.
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Unit | Record::UnitStruct { .. } => visitor.visit_unit(),
            _ => Err(self.wrong("a unit")),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Unit | Record::UnitStruct { .. } => visitor.visit_unit(),
            Record::Struct { fields, .. } if fields.is_empty() => visitor.visit_unit(),
            Record::Map(entries) if entries.is_empty() => visitor.visit_unit(),
            _ => Err(self.wrong("a unit struct")),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        let record = self.record;
        if name == ASSET_NAME {
            return match record {
                Record::Asset(uuid) => visitor.visit_newtype_struct(AssetBytes(*uuid.as_bytes())),
                _ => Err(self.wrong("an asset reference")),
            };
        }
        match record {
            Record::Newtype { value, .. } => visitor.visit_newtype_struct(self.below(value, None)),
            Record::Entity(_) | Record::Asset(_) => Err(self.wrong("a newtype")),
            // A format that writes a newtype as the value it wraps.
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
                self.visit_elements(items, visitor)
            }
            Record::Bytes(bytes) => {
                let mut elements = SeqDeserializer::<_, RecordError>::new(bytes.iter().copied());
                let value = visitor.visit_seq(&mut elements)?;
                // Bytes the target did not take are refused, as elements are.
                elements.end()?;
                Ok(value)
            }
            _ => Err(self.wrong("a sequence")),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Map(entries) => visitor.visit_map(Entries::new(entries, self.cx, self.node)),
            Record::Struct { fields, .. } => {
                visitor.visit_map(Fields::new(fields, self.cx, self.node))
            }
            _ => Err(self.wrong("a map")),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        if name == ENTITY_NAME {
            let entity = self.entity()?;
            return visitor.visit_map(entity_fields(entity));
        }
        let record = self.record;
        match record {
            Record::Struct {
                fields: written, ..
            } => {
                self.note(Event::Fields(fields));
                visitor.visit_map(Fields::new(written, self.cx, self.node))
            }
            Record::Map(entries) => {
                self.note(Event::Fields(fields));
                visitor.visit_map(Entries::new(entries, self.cx, self.node))
            }
            Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
                self.note(Event::Positional);
                self.visit_elements(items, visitor)
            }
            _ => Err(self.wrong("a struct")),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        let record = self.record;
        match record {
            Record::Variant {
                variant, payload, ..
            } => self.visit_enum_named(variant, Payload::Recorded(payload), visitor),
            // How a self-describing format writes a variant: its name as the
            // one key, what it carries as the value.
            Record::Map(entries) if entries.len() == 1 => match &entries[0] {
                (Record::Str(name), value) => {
                    self.visit_enum_named(name, Payload::Value(value), visitor)
                }
                _ => Err(RecordError(
                    "an enum written as a map must key it by the variant's name".to_owned(),
                )),
            },
            Record::Str(name) => {
                self.visit_enum_named(name, Payload::Recorded(&VariantPayload::Unit), visitor)
            }
            _ => Err(self.wrong("an enum variant")),
        }
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.record {
            Record::Str(v) => visitor.visit_str(v),
            Record::U64(v) => visitor.visit_u64(*v),
            _ => Err(self.wrong("an identifier")),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(
        mut self,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.note(Event::Ignored);
        visitor.visit_unit()
    }
}
