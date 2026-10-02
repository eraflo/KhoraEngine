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

//! How the reader walks into a value: a sequence's elements, a struct's
//! fields, a map's entries, an enum's variant.

use serde::de::value::StrDeserializer;
use serde::de::{
    self, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};

use super::de::{Cx, Reader};
use super::watch::{Event, Step};
use super::{Record, RecordError, VariantPayload};

/// The elements of a sequence, read one after another.
pub(super) struct Elements<'a, 'c, 'r> {
    pub(super) items: std::slice::Iter<'a, Record>,
    next: usize,
    cx: &'c mut Cx<'r>,
    node: usize,
}

impl<'a, 'c, 'r> Elements<'a, 'c, 'r> {
    pub(super) fn new(items: &'a [Record], cx: &'c mut Cx<'r>, node: usize) -> Self {
        Self {
            items: items.iter(),
            next: 0,
            cx,
            node,
        }
    }
}

impl<'de> SeqAccess<'de> for Elements<'_, '_, '_> {
    type Error = RecordError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, RecordError> {
        let Some(record) = self.items.next() else {
            return Ok(None);
        };
        let node = self.cx.watch.child(self.node, Step::Index(self.next));
        self.next += 1;
        seed.deserialize(Reader {
            record,
            cx: &mut *self.cx,
            node,
        })
        .map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

/// The named fields of a struct, presented as a map keyed by name.
pub(super) struct Fields<'a, 'c, 'r> {
    fields: std::slice::Iter<'a, (String, Record)>,
    value: Option<(&'a str, &'a Record)>,
    cx: &'c mut Cx<'r>,
    node: usize,
}

impl<'a, 'c, 'r> Fields<'a, 'c, 'r> {
    pub(super) fn new(fields: &'a [(String, Record)], cx: &'c mut Cx<'r>, node: usize) -> Self {
        Self {
            fields: fields.iter(),
            value: None,
            cx,
            node,
        }
    }
}

impl<'de> MapAccess<'de> for Fields<'_, '_, '_> {
    type Error = RecordError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, RecordError> {
        match self.fields.next() {
            Some((name, value)) => {
                self.value = Some((name, value));
                let key: StrDeserializer<'_, RecordError> = name.as_str().into_deserializer();
                seed.deserialize(key).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, RecordError> {
        let (name, record) = self
            .value
            .take()
            .ok_or_else(|| RecordError("a field value was read before its name".to_owned()))?;
        let node = self.cx.watch.child(self.node, Step::Field(name.to_owned()));
        seed.deserialize(Reader {
            record,
            cx: &mut *self.cx,
            node,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.fields.len())
    }
}

/// The entries of a map, keys and values both records.
pub(super) struct Entries<'a, 'c, 'r> {
    entries: std::slice::Iter<'a, (Record, Record)>,
    value: Option<(&'a Record, &'a Record)>,
    cx: &'c mut Cx<'r>,
    node: usize,
}

impl<'a, 'c, 'r> Entries<'a, 'c, 'r> {
    pub(super) fn new(entries: &'a [(Record, Record)], cx: &'c mut Cx<'r>, node: usize) -> Self {
        Self {
            entries: entries.iter(),
            value: None,
            cx,
            node,
        }
    }
}

impl<'de> MapAccess<'de> for Entries<'_, '_, '_> {
    type Error = RecordError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, RecordError> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some((key, value));
        // A key is a value in its own right — an entity keying a map is
        // remapped like any other.
        let node = self.cx.watch.child(self.node, Step::KeyOf);
        seed.deserialize(Reader {
            record: key,
            cx: &mut *self.cx,
            node,
        })
        .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, RecordError> {
        let (key, record) = self
            .value
            .take()
            .ok_or_else(|| RecordError("a map value was read before its key".to_owned()))?;
        let step = match key {
            Record::Str(name) => Step::Field(name.clone()),
            other => Step::Entry(other.clone()),
        };
        let node = self.cx.watch.child(self.node, step);
        seed.deserialize(Reader {
            record,
            cx: &mut *self.cx,
            node,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// What a variant carries, in either of the two forms a record can hold it.
pub(super) enum Payload<'a> {
    /// As the serializer wrote it.
    Recorded(&'a VariantPayload),
    /// As the value of a single-key map.
    Value(&'a Record),
}

/// One enum variant being read.
pub(super) struct Variant<'a, 'c, 'r> {
    pub(super) name: &'a str,
    pub(super) payload: Payload<'a>,
    pub(super) cx: &'c mut Cx<'r>,
    /// The node of what the variant carries.
    pub(super) node: usize,
}

impl<'de> EnumAccess<'de> for Variant<'_, '_, '_> {
    type Error = RecordError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self), RecordError> {
        let name: StrDeserializer<'_, RecordError> = self.name.into_deserializer();
        let value = seed.deserialize(name)?;
        Ok((value, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, '_, '_> {
    type Error = RecordError;

    fn unit_variant(self) -> Result<(), RecordError> {
        match self.payload {
            Payload::Recorded(VariantPayload::Unit) | Payload::Value(Record::Unit) => Ok(()),
            _ => Err(RecordError(format!(
                "variant `{}` carries a value, but the code declares none",
                self.name
            ))),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, RecordError> {
        let record = match self.payload {
            Payload::Recorded(VariantPayload::Newtype(inner)) => &**inner,
            Payload::Value(value) => value,
            Payload::Recorded(_) => {
                return Err(RecordError(format!(
                    "variant `{}` does not carry the single value the code expects",
                    self.name
                )))
            }
        };
        seed.deserialize(Reader {
            record,
            cx: self.cx,
            node: self.node,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        match self.payload {
            Payload::Recorded(VariantPayload::Tuple(items))
            | Payload::Value(Record::Seq(items)) => Reader {
                record: &Record::Unit,
                cx: self.cx,
                node: self.node,
            }
            .visit_elements(items, visitor),
            _ => Err(RecordError(format!(
                "variant `{}` does not carry the values the code expects",
                self.name
            ))),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, RecordError> {
        match self.payload {
            Payload::Recorded(VariantPayload::Struct(written))
            | Payload::Value(Record::Struct {
                fields: written, ..
            }) => {
                self.cx.watch.note(self.node, Event::Fields(fields));
                visitor.visit_map(Fields::new(written, self.cx, self.node))
            }
            Payload::Value(Record::Map(entries)) => {
                self.cx.watch.note(self.node, Event::Fields(fields));
                visitor.visit_map(Entries::new(entries, self.cx, self.node))
            }
            _ => Err(RecordError(format!(
                "variant `{}` does not carry the fields the code expects",
                self.name
            ))),
        }
    }
}

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
