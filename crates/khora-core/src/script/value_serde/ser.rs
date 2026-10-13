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

//! Writing a serde value as a [`ScriptValue`].

use serde::ser::{self, Serialize};

use super::{ValueSerdeError, ENTITY_NAME};
use crate::ecs::entity::EntityId;
use crate::script::ScriptValue;

impl ser::Error for ValueSerdeError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// The serializer every value is written through.
pub(super) struct Writer;

/// A variant with a payload: `Struct[(variant, payload)]`.
fn variant(name: &str, payload: ScriptValue) -> ScriptValue {
    ScriptValue::Struct(vec![(name.to_owned(), payload)])
}

impl ser::Serializer for Writer {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    type SerializeSeq = Items;
    type SerializeTuple = Items;
    type SerializeTupleStruct = Items;
    type SerializeTupleVariant = Items;
    type SerializeMap = Entries;
    type SerializeStruct = Fields;
    type SerializeStructVariant = Fields;

    fn serialize_bool(self, v: bool) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v))
    }
    fn serialize_u8(self, v: u8) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v.into()))
    }
    /// Its bits as an `int`: a fingerprint is a `u64` past what an `int`
    /// holds, and is read back at its own type.
    fn serialize_u64(self, v: u64) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Int(v as i64))
    }
    fn serialize_f32(self, v: f32) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Float(v))
    }
    fn serialize_f64(self, v: f64) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Float(v as f32))
    }
    fn serialize_char(self, v: char) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Str(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Str(v.to_owned()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Array(
            v.iter()
                .map(|byte| ScriptValue::Int((*byte).into()))
                .collect(),
        ))
    }
    fn serialize_none(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(
        self,
        value: &T,
    ) -> Result<ScriptValue, ValueSerdeError> {
        value.serialize(Writer)
    }
    fn serialize_unit(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Null)
    }
    /// A variant with nothing is its name.
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Str(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<ScriptValue, ValueSerdeError> {
        value.serialize(Writer)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        name: &'static str,
        value: &T,
    ) -> Result<ScriptValue, ValueSerdeError> {
        Ok(variant(name, value.serialize(Writer)?))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Items, ValueSerdeError> {
        Ok(Items {
            items: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Items, ValueSerdeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Items, ValueSerdeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        name: &'static str,
        len: usize,
    ) -> Result<Items, ValueSerdeError> {
        Ok(Items {
            items: Vec::with_capacity(len),
            variant: Some(name),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Entries, ValueSerdeError> {
        Ok(Entries {
            fields: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Fields, ValueSerdeError> {
        Ok(Fields {
            fields: Vec::with_capacity(len),
            variant: None,
            entity: name == ENTITY_NAME,
        })
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        name: &'static str,
        len: usize,
    ) -> Result<Fields, ValueSerdeError> {
        Ok(Fields {
            fields: Vec::with_capacity(len),
            variant: Some(name),
            entity: false,
        })
    }
}

/// A sequence, a tuple, or a tuple variant's payload.
pub(super) struct Items {
    items: Vec<ScriptValue>,
    variant: Option<&'static str>,
}

impl Items {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValueSerdeError> {
        self.items.push(value.serialize(Writer)?);
        Ok(())
    }

    fn finish(self) -> ScriptValue {
        let array = ScriptValue::Array(self.items);
        match self.variant {
            Some(name) => variant(name, array),
            None => array,
        }
    }
}

impl ser::SerializeSeq for Items {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ValueSerdeError> {
        self.push(value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTuple for Items {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ValueSerdeError> {
        self.push(value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleStruct for Items {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValueSerdeError> {
        self.push(value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleVariant for Items {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValueSerdeError> {
        self.push(value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(self.finish())
    }
}

/// A map: text keys only, written as a struct of them.
pub(super) struct Entries {
    fields: Vec<(String, ScriptValue)>,
    key: Option<String>,
}

impl ser::SerializeMap for Entries {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), ValueSerdeError> {
        match key.serialize(Writer)? {
            ScriptValue::Str(key) => {
                self.key = Some(key);
                Ok(())
            }
            other => Err(ValueSerdeError(format!(
                "a map key must be text, not {}",
                other.type_name()
            ))),
        }
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ValueSerdeError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| ValueSerdeError("a map value with no key".to_owned()))?;
        self.fields.push((key, value.serialize(Writer)?));
        Ok(())
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        Ok(ScriptValue::Struct(self.fields))
    }
}

/// A struct, or a struct variant's payload — or an entity, which a script
/// value holds as itself.
pub(super) struct Fields {
    fields: Vec<(String, ScriptValue)>,
    variant: Option<&'static str>,
    entity: bool,
}

impl Fields {
    fn push<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<(), ValueSerdeError> {
        self.fields.push((key.to_owned(), value.serialize(Writer)?));
        Ok(())
    }

    fn finish(self) -> Result<ScriptValue, ValueSerdeError> {
        if self.entity {
            let part = |name: &str| {
                self.fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .and_then(|(_, value)| match value {
                        ScriptValue::Int(v) => u32::try_from(*v).ok(),
                        _ => None,
                    })
                    .ok_or_else(|| ValueSerdeError(format!("an entity with no `{name}`")))
            };
            return Ok(ScriptValue::Entity(EntityId {
                index: part("index")?,
                generation: part("generation")?,
            }));
        }
        let value = ScriptValue::Struct(self.fields);
        Ok(match self.variant {
            Some(name) => variant(name, value),
            None => value,
        })
    }
}

impl ser::SerializeStruct for Fields {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ValueSerdeError> {
        self.push(key, value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for Fields {
    type Ok = ScriptValue;
    type Error = ValueSerdeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ValueSerdeError> {
        self.push(key, value)
    }
    fn end(self) -> Result<ScriptValue, ValueSerdeError> {
        self.finish()
    }
}
