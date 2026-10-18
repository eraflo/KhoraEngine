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

//! Reading a serde value from a [`ScriptValue`].

use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};

use super::{ValueSerdeError, ASSET_NAME, ENTITY_NAME};
use crate::script::ScriptValue;

impl de::Error for ValueSerdeError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// The deserializer every value is read through.
pub(super) struct Reader<'v>(pub(super) &'v ScriptValue);

impl Reader<'_> {
    fn refused(&self, expected: &str) -> ValueSerdeError {
        ValueSerdeError(format!("expected {expected}, found {}", self.0.type_name()))
    }

    fn int(&self) -> Result<i64, ValueSerdeError> {
        match self.0 {
            ScriptValue::Int(v) => Ok(*v),
            _ => Err(self.refused("an integer")),
        }
    }
}

macro_rules! narrow {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
            let v = self.int()?;
            visitor.$visit(
                <$ty>::try_from(v).map_err(|_| {
                    ValueSerdeError(format!("{v} does not fit a {}", stringify!($ty)))
                })?,
            )
        }
    };
}

impl<'de> de::Deserializer<'de> for Reader<'de> {
    type Error = ValueSerdeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Unit | ScriptValue::Null => visitor.visit_unit(),
            ScriptValue::Bool(v) => visitor.visit_bool(*v),
            ScriptValue::Int(v) => visitor.visit_i64(*v),
            ScriptValue::Float(v) => visitor.visit_f32(*v),
            ScriptValue::Str(v) => visitor.visit_borrowed_str(v),
            ScriptValue::Array(items) => visitor.visit_seq(Items(items.iter())),
            ScriptValue::Struct(fields) => visitor.visit_map(Fields {
                fields: fields.iter(),
                value: None,
            }),
            ScriptValue::Entity(entity) => visitor.visit_map(EntityParts {
                parts: [entity.index, entity.generation],
                at: 0,
            }),
            other => Err(ValueSerdeError(format!(
                "{} is read through its own type",
                other.type_name()
            ))),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Bool(v) => visitor.visit_bool(*v),
            _ => Err(self.refused("a flag")),
        }
    }

    narrow!(deserialize_i8, visit_i8, i8);
    narrow!(deserialize_i16, visit_i16, i16);
    narrow!(deserialize_i32, visit_i32, i32);
    narrow!(deserialize_u8, visit_u8, u8);
    narrow!(deserialize_u16, visit_u16, u16);
    narrow!(deserialize_u32, visit_u32, u32);

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        visitor.visit_i64(self.int()?)
    }

    /// The bits a `u64` was written as.
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        visitor.visit_u64(self.int()? as u64)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Float(v) => visitor.visit_f32(*v),
            ScriptValue::Int(v) => visitor.visit_f32(*v as f32),
            _ => Err(self.refused("a number")),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Float(v) => visitor.visit_f64(f64::from(*v)),
            ScriptValue::Int(v) => visitor.visit_f64(*v as f64),
            _ => Err(self.refused("a number")),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Str(v) if v.chars().count() == 1 => {
                visitor.visit_char(v.chars().next().unwrap_or_default())
            }
            _ => Err(self.refused("one character")),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Str(v) => visitor.visit_borrowed_str(v),
            _ => Err(self.refused("text")),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Null | ScriptValue::Unit => visitor.visit_unit(),
            _ => Err(self.refused("nothing")),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        match (name, self.0) {
            // An asset's identity reads itself from its hyphenated text.
            (ASSET_NAME, ScriptValue::Asset(asset)) => {
                visitor.visit_newtype_struct(asset.to_string().into_deserializer())
            }
            (ASSET_NAME, _) => Err(self.refused("an asset")),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Array(items) => visitor.visit_seq(Items(items.iter())),
            _ => Err(self.refused("a list")),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Struct(fields) => visitor.visit_map(Fields {
                fields: fields.iter(),
                value: None,
            }),
            _ => Err(self.refused("named fields")),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        match (name, self.0) {
            (ENTITY_NAME, ScriptValue::Entity(entity)) => visitor.visit_map(EntityParts {
                parts: [entity.index, entity.generation],
                at: 0,
            }),
            (ENTITY_NAME, _) => Err(self.refused("an entity")),
            _ => self.deserialize_map(visitor),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        match self.0 {
            ScriptValue::Str(name) => visitor.visit_enum(name.as_str().into_deserializer()),
            ScriptValue::Struct(fields) if fields.len() == 1 => visitor.visit_enum(Variant {
                name: &fields[0].0,
                payload: &fields[0].1,
            }),
            _ => Err(self.refused("a variant")),
        }
    }

    fn deserialize_identifier<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        visitor.visit_unit()
    }
}

struct Items<'v>(std::slice::Iter<'v, ScriptValue>);

impl<'de> de::SeqAccess<'de> for Items<'de> {
    type Error = ValueSerdeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, ValueSerdeError> {
        self.0
            .next()
            .map(|item| seed.deserialize(Reader(item)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct Fields<'v> {
    fields: std::slice::Iter<'v, (String, ScriptValue)>,
    value: Option<&'v ScriptValue>,
}

impl<'de> de::MapAccess<'de> for Fields<'de> {
    type Error = ValueSerdeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ValueSerdeError> {
        let Some((name, value)) = self.fields.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(name.as_str().into_deserializer())
            .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ValueSerdeError> {
        let value = self
            .value
            .take()
            .ok_or_else(|| ValueSerdeError("a field value with no name".to_owned()))?;
        seed.deserialize(Reader(value))
    }
}

/// An entity read as the struct its serde form is: `index`, `generation`.
struct EntityParts {
    parts: [u32; 2],
    at: usize,
}

impl<'de> de::MapAccess<'de> for EntityParts {
    type Error = ValueSerdeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ValueSerdeError> {
        let name = match self.at {
            0 => "index",
            1 => "generation",
            _ => return Ok(None),
        };
        seed.deserialize(name.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ValueSerdeError> {
        let part = self.parts.get(self.at).copied().unwrap_or_default();
        self.at += 1;
        seed.deserialize(part.into_deserializer())
    }
}

/// A variant with a payload, read from `Struct[(name, payload)]`.
struct Variant<'v> {
    name: &'v str,
    payload: &'v ScriptValue,
}

impl<'de> de::EnumAccess<'de> for Variant<'de> {
    type Error = ValueSerdeError;
    type Variant = Reader<'de>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Reader<'de>), ValueSerdeError> {
        let name = seed.deserialize(self.name.into_deserializer())?;
        Ok((name, Reader(self.payload)))
    }
}

impl<'de> de::VariantAccess<'de> for Reader<'de> {
    type Error = ValueSerdeError;

    fn unit_variant(self) -> Result<(), ValueSerdeError> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, ValueSerdeError> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        de::Deserializer::deserialize_seq(self, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ValueSerdeError> {
        de::Deserializer::deserialize_map(self, visitor)
    }
}
