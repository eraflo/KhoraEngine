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

//! Writing a value by position.

use khora_core::ecs::entity::EntityId;
use serde::ser::{self, Serialize};

use super::{write_varint, zigzag};
use crate::scene::record::{
    to_record, EntityRef, Record, RecordError, ReferenceWriter, ENTITY_NAME, MAX_DEPTH,
};

/// The serializer.
pub(super) struct Writer<'o, 'r> {
    out: &'o mut Vec<u8>,
    references: &'r mut dyn ReferenceWriter,
    depth: usize,
}

impl<'o, 'r> Writer<'o, 'r> {
    pub(super) fn new(out: &'o mut Vec<u8>, references: &'r mut dyn ReferenceWriter) -> Self {
        Self {
            out,
            references,
            depth: 0,
        }
    }

    /// One level deeper; refused past [`MAX_DEPTH`] — the reader would
    /// refuse it too.
    fn descend(&mut self) -> Result<(), RecordError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(RecordError(format!(
                "a value nests deeper than {MAX_DEPTH} levels"
            )));
        }
        Ok(())
    }

    fn ascend(&mut self) {
        self.depth -= 1;
    }

    fn count(&mut self, len: Option<usize>) -> Result<(), RecordError> {
        let len = len.ok_or_else(|| {
            RecordError("a sequence or map of unknown length cannot be written by position".into())
        })?;
        write_varint(self.out, len as u64);
        Ok(())
    }

    fn entity(&mut self, entity: EntityId) {
        match self.references.write_entity(entity) {
            EntityRef::Outside => self.out.push(0),
            EntityRef::Id(id) => {
                self.out.push(1);
                self.out.extend_from_slice(&id.to_bits().to_le_bytes());
            }
        }
    }
}

impl<'w, 'o, 'r> ser::Serializer for &'w mut Writer<'o, 'r> {
    type Ok = ();
    type Error = RecordError;
    type SerializeSeq = Compound<'w, 'o, 'r>;
    type SerializeTuple = Compound<'w, 'o, 'r>;
    type SerializeTupleStruct = Compound<'w, 'o, 'r>;
    type SerializeTupleVariant = Compound<'w, 'o, 'r>;
    type SerializeMap = Entries<'w, 'o, 'r>;
    type SerializeStruct = Fields<'w, 'o, 'r>;
    type SerializeStructVariant = Compound<'w, 'o, 'r>;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_bool(self, v: bool) -> Result<(), RecordError> {
        self.out.push(u8::from(v));
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<(), RecordError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> Result<(), RecordError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> Result<(), RecordError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> Result<(), RecordError> {
        write_varint(self.out, zigzag(v));
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<(), RecordError> {
        self.out.push(v);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> Result<(), RecordError> {
        self.serialize_u64(v.into())
    }
    fn serialize_u32(self, v: u32) -> Result<(), RecordError> {
        self.serialize_u64(v.into())
    }
    fn serialize_u64(self, v: u64) -> Result<(), RecordError> {
        write_varint(self.out, v);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<(), RecordError> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), RecordError> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<(), RecordError> {
        self.serialize_u64(u64::from(u32::from(v)))
    }
    fn serialize_str(self, v: &str) -> Result<(), RecordError> {
        self.serialize_bytes(v.as_bytes())
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<(), RecordError> {
        write_varint(self.out, v.len() as u64);
        self.out.extend_from_slice(v);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), RecordError> {
        self.out.push(0);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), RecordError> {
        self.out.push(1);
        self.descend()?;
        value.serialize(&mut *self)?;
        self.ascend();
        Ok(())
    }
    fn serialize_unit(self) -> Result<(), RecordError> {
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), RecordError> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
    ) -> Result<(), RecordError> {
        self.serialize_u64(index.into())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        // A suspended machine's reserved name needs nothing more: its form
        // follows as an enum variant, index first.
        self.descend()?;
        value.serialize(&mut *self)?;
        self.ascend();
        Ok(())
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        write_varint(self.out, index.into());
        self.descend()?;
        value.serialize(&mut *self)?;
        self.ascend();
        Ok(())
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'w, 'o, 'r>, RecordError> {
        self.count(len)?;
        let mut sequence = Compound::open(self)?;
        sequence.counted = len.map(|len| (len, sequence.writer.out.len()));
        Ok(sequence)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Compound<'w, 'o, 'r>, RecordError> {
        Compound::open(self)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Compound<'w, 'o, 'r>, RecordError> {
        Compound::open(self)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'w, 'o, 'r>, RecordError> {
        write_varint(self.out, index.into());
        Compound::open(self)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Entries<'w, 'o, 'r>, RecordError> {
        self.count(len)?;
        self.descend()?;
        Ok(Entries {
            writer: self,
            entries: Vec::with_capacity(len.unwrap_or(0).min(1024)),
            key: None,
        })
    }
    fn serialize_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Fields<'w, 'o, 'r>, RecordError> {
        if name == ENTITY_NAME {
            return Ok(Fields::Entity {
                writer: self,
                index: None,
                generation: None,
            });
        }
        self.descend()?;
        Ok(Fields::Plain(self))
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'w, 'o, 'r>, RecordError> {
        write_varint(self.out, index.into());
        Compound::open(self)
    }
}

/// The elements of a sequence, tuple, map or variant, written in order.
pub(super) struct Compound<'w, 'o, 'r> {
    writer: &'w mut Writer<'o, 'r>,
    /// For a sequence: its count, and where its elements start.
    counted: Option<(usize, usize)>,
}

impl<'w, 'o, 'r> Compound<'w, 'o, 'r> {
    fn open(writer: &'w mut Writer<'o, 'r>) -> Result<Self, RecordError> {
        writer.descend()?;
        Ok(Self {
            writer,
            counted: None,
        })
    }

    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        value.serialize(&mut *self.writer)
    }

    fn close(self) -> Result<(), RecordError> {
        // A reader takes a count only as far as the bytes back it, give or
        // take the elements that take none: refuse what it would refuse.
        if let Some((count, start)) = self.counted {
            let bytes = self.writer.out.len() - start;
            if count > bytes.saturating_add(super::FREE_ELEMENTS) {
                return Err(RecordError(format!(
                    "a sequence of {count} elements in {bytes} bytes: more than \
                     {} elements that take no bytes cannot be written by position",
                    super::FREE_ELEMENTS
                )));
            }
        }
        self.writer.ascend();
        Ok(())
    }
}

impl ser::SerializeSeq for Compound<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.element(value)
    }
    fn end(self) -> Result<(), RecordError> {
        self.close()
    }
}

impl ser::SerializeTuple for Compound<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.element(value)
    }
    fn end(self) -> Result<(), RecordError> {
        self.close()
    }
}

impl ser::SerializeTupleStruct for Compound<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.element(value)
    }
    fn end(self) -> Result<(), RecordError> {
        self.close()
    }
}

impl ser::SerializeTupleVariant for Compound<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        self.element(value)
    }
    fn end(self) -> Result<(), RecordError> {
        self.close()
    }
}

/// A map's entries, written sorted by their keys' bytes: a map iterates in
/// whatever order it hashes to, and the same world must make the same bytes.
pub(super) struct Entries<'w, 'o, 'r> {
    writer: &'w mut Writer<'o, 'r>,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    key: Option<Vec<u8>>,
}

impl Entries<'_, '_, '_> {
    /// `value` written on its own, at the map's depth.
    fn bytes_of<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Vec<u8>, RecordError> {
        let mut bytes = Vec::new();
        let mut writer = Writer {
            out: &mut bytes,
            references: &mut *self.writer.references,
            depth: self.writer.depth,
        };
        value.serialize(&mut writer)?;
        Ok(bytes)
    }
}

impl ser::SerializeMap for Entries<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), RecordError> {
        self.key = Some(self.bytes_of(key)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), RecordError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| RecordError("a map value without its key".into()))?;
        let value = self.bytes_of(value)?;
        self.entries.push((key, value));
        Ok(())
    }
    fn end(mut self) -> Result<(), RecordError> {
        self.entries.sort_unstable();
        for (key, value) in &self.entries {
            self.writer.out.extend_from_slice(key);
            self.writer.out.extend_from_slice(value);
        }
        self.writer.ascend();
        Ok(())
    }
}

impl ser::SerializeStructVariant for Compound<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        self.element(value)
    }
    fn end(self) -> Result<(), RecordError> {
        self.close()
    }
}

/// A struct's fields — or, for an entity, its two numbers, written as the
/// reference the writer gives it.
pub(super) enum Fields<'w, 'o, 'r> {
    Plain(&'w mut Writer<'o, 'r>),
    Entity {
        writer: &'w mut Writer<'o, 'r>,
        index: Option<u32>,
        generation: Option<u32>,
    },
}

impl ser::SerializeStruct for Fields<'_, '_, '_> {
    type Ok = ();
    type Error = RecordError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), RecordError> {
        match self {
            Self::Plain(writer) => value.serialize(&mut **writer),
            Self::Entity {
                index, generation, ..
            } => {
                let number = entity_number(value)?;
                match key {
                    "index" => *index = Some(number),
                    "generation" => *generation = Some(number),
                    other => return Err(RecordError(format!("an entity has no field `{other}`"))),
                }
                Ok(())
            }
        }
    }

    fn end(self) -> Result<(), RecordError> {
        match self {
            Self::Plain(writer) => {
                writer.ascend();
                Ok(())
            }
            Self::Entity {
                writer,
                index: Some(index),
                generation: Some(generation),
            } => {
                writer.entity(EntityId { index, generation });
                Ok(())
            }
            Self::Entity { .. } => Err(RecordError(
                "an entity needs both its index and its generation".into(),
            )),
        }
    }
}

/// The number an entity field holds.
fn entity_number<T: Serialize + ?Sized>(value: &T) -> Result<u32, RecordError> {
    struct NoEntities;
    impl ReferenceWriter for NoEntities {
        fn write_entity(&mut self, _entity: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    match to_record(value, &mut NoEntities)? {
        Record::U64(number) => {
            u32::try_from(number).map_err(|_| RecordError("an entity field out of range".into()))
        }
        Record::I64(number) => {
            u32::try_from(number).map_err(|_| RecordError("an entity field out of range".into()))
        }
        other => Err(RecordError(format!(
            "an entity field is not a number: {other:?}"
        ))),
    }
}
