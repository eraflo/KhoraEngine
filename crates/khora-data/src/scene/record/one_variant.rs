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

//! How the reader walks into a variant carrying a value, read as a map with
//! its name as the one key.

use serde::de::value::StrDeserializer;
use serde::de::{self, DeserializeSeed, IntoDeserializer, MapAccess, Visitor};

use super::access::Fields;
use super::de::{Cx, Reader};
use super::watch::Step;
use super::{Record, RecordError, VariantPayload};

/// A variant carrying a value, as a map with its name as the one key.
pub(super) struct OneVariant<'a, 'c, 'r> {
    pub(super) name: Option<&'a str>,
    pub(super) payload: &'a VariantPayload,
    pub(super) cx: &'c mut Cx<'r>,
    pub(super) node: usize,
}

impl<'de> MapAccess<'de> for OneVariant<'_, '_, '_> {
    type Error = RecordError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, RecordError> {
        match self.name {
            Some(name) => {
                let key: StrDeserializer<'_, RecordError> = name.into_deserializer();
                seed.deserialize(key).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, RecordError> {
        let name = self.name.take().unwrap_or_default();
        let node = self
            .cx
            .watch
            .child(self.node, Step::Variant(name.to_owned()));
        seed.deserialize(PayloadReader {
            payload: self.payload,
            cx: &mut *self.cx,
            node,
        })
    }
}

/// What a variant carries, read as a plain value.
pub(super) struct PayloadReader<'a, 'c, 'r> {
    payload: &'a VariantPayload,
    cx: &'c mut Cx<'r>,
    node: usize,
}

impl<'de> de::Deserializer<'de> for PayloadReader<'_, '_, '_> {
    type Error = RecordError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, RecordError> {
        match self.payload {
            VariantPayload::Unit => visitor.visit_unit(),
            VariantPayload::Newtype(inner) => de::Deserializer::deserialize_any(
                Reader {
                    record: inner,
                    cx: self.cx,
                    node: self.node,
                },
                visitor,
            ),
            VariantPayload::Tuple(items) => Reader {
                record: &Record::Unit,
                cx: self.cx,
                node: self.node,
            }
            .visit_elements(items, visitor),
            VariantPayload::Struct(fields) => {
                visitor.visit_map(Fields::new(fields, self.cx, self.node))
            }
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}
