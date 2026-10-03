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

//! Writing a value into a record.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use serde::ser::{self, Serialize};

use super::{EntityRef, Record, RecordError, VariantPayload, ASSET_NAME, ENTITY_NAME, MAX_DEPTH};

/// Turns the entities a value refers to into references a save can keep.
pub trait ReferenceWriter {
    /// The reference that stands for `entity` in the save being written.
    fn write_entity(&mut self, entity: EntityId) -> EntityRef;
}

/// Writes `value` as a record.
pub fn to_record<T: Serialize + ?Sized>(
    value: &T,
    references: &mut dyn ReferenceWriter,
) -> Result<Record, RecordError> {
    let mut writer = Writer {
        references,
        depth: 0,
    };
    value.serialize(&mut writer)
}

/// The serializer: what every nested value is written through.
struct Writer<'r> {
    references: &'r mut dyn ReferenceWriter,
    /// How many values deep the current one sits.
    depth: usize,
}

impl Writer<'_> {
    /// Writes a value one level deeper, refusing a nesting no real value has —
    /// a self-referencing structure would otherwise exhaust the stack.
    fn nested<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Record, RecordError> {
        if self.depth >= MAX_DEPTH {
            return Err(RecordError(format!(
                "a value nests deeper than {MAX_DEPTH} levels"
            )));
        }
        self.depth += 1;
        let written = value.serialize(&mut *self);
        self.depth -= 1;
        written
    }
}

impl<'a, 'r> ser::Serializer for &'a mut Writer<'r> {
    type Ok = Record;
    type Error = RecordError;
    type SerializeSeq = Collect<'a, 'r>;
    type SerializeTuple = Collect<'a, 'r>;
    type SerializeTupleStruct = Collect<'a, 'r>;
    type SerializeTupleVariant = Collect<'a, 'r>;
    type SerializeMap = CollectMap<'a, 'r>;
    type SerializeStruct = CollectStruct<'a, 'r>;
    type SerializeStructVariant = CollectStruct<'a, 'r>;

    /// A save is not for people to read in this form; saying so makes types
    /// with two forms (a uuid as text or as bytes) pick the compact one.
    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_bool(self, v: bool) -> Result<Record, RecordError> {
        Ok(Record::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Record, RecordError> {
        Ok(Record::I64(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<Record, RecordError> {
        Ok(Record::I64(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<Record, RecordError> {
        Ok(Record::I64(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<Record, RecordError> {
        Ok(Record::I64(v))
    }
    fn serialize_u8(self, v: u8) -> Result<Record, RecordError> {
        Ok(Record::U64(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<Record, RecordError> {
        Ok(Record::U64(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<Record, RecordError> {
        Ok(Record::U64(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<Record, RecordError> {
        Ok(Record::U64(v))
    }
    fn serialize_f32(self, v: f32) -> Result<Record, RecordError> {
        Ok(Record::F32(v))
    }
    fn serialize_f64(self, v: f64) -> Result<Record, RecordError> {
        Ok(Record::F64(v))
    }
    fn serialize_char(self, v: char) -> Result<Record, RecordError> {
        Ok(Record::Char(v))
    }
    fn serialize_str(self, v: &str) -> Result<Record, RecordError> {
        Ok(Record::Str(v.to_owned()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Record, RecordError> {
        Ok(Record::Bytes(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Record, RecordError> {
        Ok(Record::None)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Record, RecordError> {
        Ok(Record::Some(Box::new(self.nested(value)?)))
    }
    fn serialize_unit(self) -> Result<Record, RecordError> {
        Ok(Record::Unit)
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<Record, RecordError> {
        Ok(Record::UnitStruct {
            name: name.to_owned(),
        })
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Record, RecordError> {
        Ok(variant_record(name, variant, VariantPayload::Unit))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Record, RecordError> {
        let inner = self.nested(value)?;
        if name == ASSET_NAME {
            return asset_from(inner);
        }
        Ok(Record::Newtype {
            name: name.to_owned(),
            value: Box::new(inner),
        })
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Record, RecordError> {
        let inner = self.nested(value)?;
        Ok(variant_record(
            name,
            variant,
            VariantPayload::Newtype(Box::new(inner)),
        ))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Collect<'a, 'r>, RecordError> {
        Ok(Collect::new(self, len, Shape::Seq))
    }
    fn serialize_tuple(self, len: usize) -> Result<Collect<'a, 'r>, RecordError> {
        Ok(Collect::new(self, Some(len), Shape::Seq))
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Collect<'a, 'r>, RecordError> {
        Ok(Collect::new(self, Some(len), Shape::TupleStruct(name)))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Collect<'a, 'r>, RecordError> {
        Ok(Collect::new(self, Some(len), Shape::Variant(name, variant)))
    }
    fn serialize_map(self, len: Option<usize>) -> Result<CollectMap<'a, 'r>, RecordError> {
        Ok(CollectMap {
            writer: self,
            entries: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<CollectStruct<'a, 'r>, RecordError> {
        Ok(CollectStruct::new(self, len, StructShape::Struct(name)))
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CollectStruct<'a, 'r>, RecordError> {
        Ok(CollectStruct::new(
            self,
            len,
            StructShape::Variant(name, variant),
        ))
    }
}

fn variant_record(enum_name: &str, variant: &str, payload: VariantPayload) -> Record {
    Record::Variant {
        enum_name: enum_name.to_owned(),
        variant: variant.to_owned(),
        payload,
    }
}

/// An asset reference, from the sixteen bytes its identity serialized as.
fn asset_from(inner: Record) -> Result<Record, RecordError> {
    let bytes = match inner {
        Record::Bytes(bytes) => bytes,
        other => {
            return Err(RecordError(format!(
                "an asset identity serialized as {other:?}, not as its sixteen bytes"
            )))
        }
    };
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| RecordError("an asset identity is not sixteen bytes".to_owned()))?;
    Ok(Record::Asset(AssetUUID::from_bytes(bytes)))
}

/// What a positional collection becomes once complete.
enum Shape {
    Seq,
    TupleStruct(&'static str),
    Variant(&'static str, &'static str),
}

/// Collects the elements of a sequence, tuple, tuple struct or tuple variant.
pub(super) struct Collect<'a, 'r> {
    writer: &'a mut Writer<'r>,
    items: Vec<Record>,
    shape: Shape,
}

impl<'a, 'r> Collect<'a, 'r> {
    fn new(writer: &'a mut Writer<'r>, len: Option<usize>, shape: Shape) -> Self {
        Self {
            writer,
            // A length hint is a claim, not a promise: capped, so a lying one
            // cannot ask for an allocation before a single element exists.
            items: Vec::with_capacity(len.unwrap_or(0).min(1024)),
            shape,
        }
    }

    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        let item = self.writer.nested(value)?;
        self.items.push(item);
        Ok(())
    }

    fn finish(self) -> Record {
        match self.shape {
            Shape::Seq => Record::Seq(self.items),
            Shape::TupleStruct(name) => Record::TupleStruct {
                name: name.to_owned(),
                fields: self.items,
            },
            Shape::Variant(name, variant) => {
                variant_record(name, variant, VariantPayload::Tuple(self.items))
            }
        }
    }
}

impl ser::SerializeSeq for Collect<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.push(value)
    }
    fn end(self) -> Result<Record, RecordError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTuple for Collect<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.push(value)
    }
    fn end(self) -> Result<Record, RecordError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleStruct for Collect<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.push(value)
    }
    fn end(self) -> Result<Record, RecordError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleVariant for Collect<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.push(value)
    }
    fn end(self) -> Result<Record, RecordError> {
        Ok(self.finish())
    }
}

/// Collects the entries of a map.
pub(super) struct CollectMap<'a, 'r> {
    writer: &'a mut Writer<'r>,
    entries: Vec<(Record, Record)>,
    key: Option<Record>,
}

impl ser::SerializeMap for CollectMap<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), RecordError> {
        self.key = Some(self.writer.nested(key)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| RecordError("a map value was written before its key".to_owned()))?;
        let value = self.writer.nested(value)?;
        self.entries.push((key, value));
        Ok(())
    }
    fn end(self) -> Result<Record, RecordError> {
        Ok(Record::Map(self.entries))
    }
}

/// What a named-field collection becomes once complete.
enum StructShape {
    Struct(&'static str),
    Variant(&'static str, &'static str),
}

/// Collects the fields of a struct or a struct variant.
pub(super) struct CollectStruct<'a, 'r> {
    writer: &'a mut Writer<'r>,
    fields: Vec<(String, Record)>,
    shape: StructShape,
}

impl<'a, 'r> CollectStruct<'a, 'r> {
    fn new(writer: &'a mut Writer<'r>, len: usize, shape: StructShape) -> Self {
        Self {
            writer,
            fields: Vec::with_capacity(len.min(1024)),
            shape,
        }
    }

    fn push<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        let value = self.writer.nested(value)?;
        self.fields.push((key.to_owned(), value));
        Ok(())
    }

    fn finish(self) -> Result<Record, RecordError> {
        match self.shape {
            StructShape::Struct(name) if name == ENTITY_NAME => {
                let entity = entity_from(&self.fields)?;
                Ok(Record::Entity(self.writer.references.write_entity(entity)))
            }
            StructShape::Struct(name) => Ok(Record::Struct {
                name: name.to_owned(),
                fields: self.fields,
            }),
            StructShape::Variant(name, variant) => Ok(variant_record(
                name,
                variant,
                VariantPayload::Struct(self.fields),
            )),
        }
    }
}

/// The entity whose two numbers were just written.
fn entity_from(fields: &[(String, Record)]) -> Result<EntityId, RecordError> {
    let number = |wanted: &str| -> Result<u32, RecordError> {
        fields
            .iter()
            .find(|(name, _)| name == wanted)
            .and_then(|(_, value)| match value {
                Record::U64(n) => u32::try_from(*n).ok(),
                _ => None,
            })
            .ok_or_else(|| RecordError(format!("an entity id without its `{wanted}`")))
    };
    Ok(EntityId {
        index: number("index")?,
        generation: number("generation")?,
    })
}

impl ser::SerializeStruct for CollectStruct<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        self.push(key, value)
    }
    fn end(self) -> Result<Record, RecordError> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for CollectStruct<'_, '_> {
    type Ok = Record;
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        self.push(key, value)
    }
    fn end(self) -> Result<Record, RecordError> {
        self.finish()
    }
}
