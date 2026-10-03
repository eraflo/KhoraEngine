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

//! Reading a value by position.
//!
//! Every count is checked against the bytes left before anything is sized
//! from it, the nesting is bounded, and a byte that is not one of the values
//! a place can hold is refused: the bytes come from a file.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use serde::de::{
    self, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};

use super::{unzigzag, FREE_ELEMENTS};
use crate::scene::record::{EntityRef, RecordError, ReferenceReader, ENTITY_NAME, MAX_DEPTH};

/// The deserializer.
pub(super) struct Reader<'de, 'r> {
    input: &'de [u8],
    at: usize,
    references: &'r mut dyn ReferenceReader,
    depth: usize,
}

fn error(message: impl Into<String>) -> RecordError {
    RecordError(message.into())
}

impl<'de, 'r> Reader<'de, 'r> {
    pub(super) fn new(input: &'de [u8], references: &'r mut dyn ReferenceReader) -> Self {
        Self {
            input,
            at: 0,
            references,
            depth: 0,
        }
    }

    /// Refuses bytes the value did not use.
    pub(super) fn finish(&self) -> Result<(), RecordError> {
        let left = self.left();
        if left > 0 {
            return Err(error(format!("{left} byte(s) after the value")));
        }
        Ok(())
    }

    fn left(&self) -> usize {
        self.input.len() - self.at
    }

    fn byte(&mut self) -> Result<u8, RecordError> {
        let byte = *self
            .input
            .get(self.at)
            .ok_or_else(|| error("the value ends early"))?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'de [u8], RecordError> {
        if len > self.left() {
            return Err(error("the value ends early"));
        }
        let taken = &self.input[self.at..self.at + len];
        self.at += len;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RecordError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn varint(&mut self) -> Result<u64, RecordError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err(error("a number too large for 64 bits"));
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(error("a number too large for 64 bits"))
    }

    fn unsigned<T: TryFrom<u64>>(&mut self, what: &str) -> Result<T, RecordError> {
        let value = self.varint()?;
        T::try_from(value).map_err(|_| error(format!("{value} does not fit a {what}")))
    }

    fn signed<T: TryFrom<i64>>(&mut self, what: &str) -> Result<T, RecordError> {
        let value = unzigzag(self.varint()?);
        T::try_from(value).map_err(|_| error(format!("{value} does not fit a {what}")))
    }

    /// A count of things, refused when the bytes left could not hold that
    /// many. An element may take no bytes at all — a unit, a marker — so a
    /// count may pass the bytes left by [`FREE_ELEMENTS`], never more: a
    /// count no input backs is still refused before anything loops over it.
    fn count(&mut self) -> Result<usize, RecordError> {
        let count = self.varint()?;
        let count = usize::try_from(count).map_err(|_| error("a count too large"))?;
        if count > self.left().saturating_add(FREE_ELEMENTS) {
            return Err(error(format!(
                "a count of {count} the value has no room for"
            )));
        }
        Ok(count)
    }

    /// A byte that is 0 or 1.
    fn flag(&mut self, what: &str) -> Result<bool, RecordError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(error(format!("{other} is not a {what}"))),
        }
    }

    fn descend(&mut self) -> Result<(), RecordError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(error(format!(
                "a value nests deeper than {MAX_DEPTH} levels"
            )));
        }
        Ok(())
    }

    fn ascend(&mut self) {
        self.depth -= 1;
    }

    fn entity(&mut self) -> Result<EntityId, RecordError> {
        let reference = if self.flag("reference tag")? {
            EntityRef::Id(PersistentId::from_bits(u64::from_le_bytes(self.array()?)))
        } else {
            EntityRef::Outside
        };
        self.references.read_entity(reference)
    }

    fn fixed<'a, V: Visitor<'de>>(
        &'a mut self,
        count: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.descend()?;
        let value = visitor.visit_seq(Fixed {
            reader: &mut *self,
            left: count,
        })?;
        self.ascend();
        Ok(value)
    }
}

impl<'de> de::Deserializer<'de> for &mut Reader<'de, '_> {
    type Error = RecordError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, RecordError> {
        Err(error(
            "a value written by position cannot be read without knowing its type",
        ))
    }
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_bool(self.flag("bool")?)
    }
    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_i8(self.signed("i8")?)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_i16(self.signed("i16")?)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_i32(self.signed("i32")?)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_i64(self.signed("i64")?)
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_u8(self.byte()?)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_u16(self.unsigned("u16")?)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_u32(self.unsigned("u32")?)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_u64(self.varint()?)
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_f32(f32::from_bits(u32::from_le_bytes(self.array()?)))
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_f64(f64::from_bits(u64::from_le_bytes(self.array()?)))
    }
    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        let code: u32 = self.unsigned("char")?;
        visitor.visit_char(char::from_u32(code).ok_or_else(|| error("not a char"))?)
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        let len = self.count()?;
        let text = std::str::from_utf8(self.take(len)?)
            .map_err(|_| error("a string that is not UTF-8"))?;
        visitor.visit_borrowed_str(text)
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.deserialize_str(visitor)
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        let len = self.count()?;
        visitor.visit_borrowed_bytes(self.take(len)?)
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        self.deserialize_bytes(visitor)
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        if !self.flag("presence tag")? {
            return visitor.visit_none();
        }
        self.descend()?;
        let value = visitor.visit_some(&mut *self)?;
        self.ascend();
        Ok(value)
    }
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_unit()
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        visitor.visit_unit()
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.descend()?;
        let value = visitor.visit_newtype_struct(&mut *self)?;
        self.ascend();
        Ok(value)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        let count = self.count()?;
        self.fixed(count, visitor)
    }
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.fixed(len, visitor)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.fixed(len, visitor)
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        let count = self.count()?;
        self.descend()?;
        let value = visitor.visit_map(Entries {
            reader: &mut *self,
            left: count,
        })?;
        self.ascend();
        Ok(value)
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        if name == ENTITY_NAME {
            let entity = self.entity()?;
            let numbers = [entity.index, entity.generation];
            return visitor.visit_seq(de::value::SeqDeserializer::new(numbers.into_iter()));
        }
        self.fixed(fields.len(), visitor)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        let index: u32 = self.unsigned("variant index")?;
        if index as usize >= variants.len() {
            return Err(error(format!(
                "variant {index} of an enum with {} variants",
                variants.len()
            )));
        }
        // The variant's payload counts its own level, as the writer does.
        visitor.visit_enum(Variant {
            reader: &mut *self,
            index,
        })
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        visitor.visit_u32(self.unsigned("identifier")?)
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(
        self,
        _visitor: V,
    ) -> Result<V::Value, RecordError> {
        Err(error(
            "a value written by position cannot be skipped without knowing its type",
        ))
    }
}

/// A known number of elements, in order.
struct Fixed<'a, 'de, 'r> {
    reader: &'a mut Reader<'de, 'r>,
    left: usize,
}

impl<'de> SeqAccess<'de> for Fixed<'_, 'de, '_> {
    type Error = RecordError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, RecordError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.reader).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.left)
    }
}

/// A known number of entries, each a key then a value.
struct Entries<'a, 'de, 'r> {
    reader: &'a mut Reader<'de, 'r>,
    left: usize,
}

impl<'de> MapAccess<'de> for Entries<'_, 'de, '_> {
    type Error = RecordError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, RecordError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.reader).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, RecordError> {
        seed.deserialize(&mut *self.reader)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.left)
    }
}

/// An enum's variant, by the index already read.
struct Variant<'a, 'de, 'r> {
    reader: &'a mut Reader<'de, 'r>,
    index: u32,
}

impl<'a, 'de, 'r> EnumAccess<'de> for Variant<'a, 'de, 'r> {
    type Error = RecordError;
    type Variant = Self;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self), RecordError> {
        let value = seed.deserialize(self.index.into_deserializer())?;
        Ok((value, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, 'de, '_> {
    type Error = RecordError;

    fn unit_variant(self) -> Result<(), RecordError> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, RecordError> {
        self.reader.descend()?;
        let value = seed.deserialize(&mut *self.reader)?;
        self.reader.ascend();
        Ok(value)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.reader.fixed(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        self.reader.fixed(fields.len(), visitor)
    }
}
