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

//! How the tracer hands a type its parts: a struct's or a tuple's fixed
//! elements, a sequence's one element, a map's one entry, an enum's chosen
//! variant — each read in its own slot, its format written down.

use serde::de::{DeserializeSeed, EnumAccess, IntoDeserializer, VariantAccess, Visitor};

use super::trace::{child, TraceError, Tracer};
use super::{Fields, Format, Key, Payload};

pub(super) use super::elements::Elements;
pub(super) use super::entry::Entry;
pub(super) use super::one::One;

/// The variant a run chose for an enum.
pub(super) struct Variant<'t, 's> {
    pub(super) tracer: &'t mut Tracer<'s>,
    pub(super) key: Key,
    pub(super) index: usize,
    /// Whether this visit writes the variant's payload down.
    pub(super) record: bool,
}

impl Variant<'_, '_> {
    fn learned(&mut self, payload: Payload) {
        if !self.record || self.tracer.shadow > 0 {
            return;
        }
        if let Some(progress) = self.tracer.state.enums.get_mut(&self.key) {
            progress.payloads[self.index] = Some(payload);
        }
    }

    fn variant_name(&self) -> &'static str {
        self.tracer
            .state
            .enums
            .get(&self.key)
            .and_then(|progress| progress.variants.get(self.index).copied())
            .unwrap_or("")
    }
}

impl<'de> EnumAccess<'de> for Variant<'_, '_> {
    type Error = TraceError;
    type Variant = Self;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self), TraceError> {
        let index = u32::try_from(self.index).unwrap_or(u32::MAX);
        let value = seed.deserialize(index.into_deserializer())?;
        Ok((value, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, '_> {
    type Error = TraceError;

    fn unit_variant(mut self) -> Result<(), TraceError> {
        self.learned(Payload::Unit);
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        mut self,
        seed: T,
    ) -> Result<T::Value, TraceError> {
        let variant = self.variant_name();
        let slot = child(self.tracer.slot, self.key, variant, 0);
        let result = self
            .tracer
            .at(slot, |tracer| seed.deserialize(&mut *tracer));
        let inner = std::mem::replace(&mut self.tracer.out, Format::Unknown);
        if result.is_ok() {
            self.learned(Payload::Tuple(vec![inner]));
        }
        result
    }

    fn tuple_variant<V: Visitor<'de>>(
        mut self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let variant = self.variant_name();
        let (result, formats) = self.tracer.elements(self.key, variant, len, visitor);
        if result.is_ok() {
            self.learned(Payload::Tuple(formats));
        }
        result
    }

    fn struct_variant<V: Visitor<'de>>(
        mut self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let variant = self.variant_name();
        let (result, formats) = self
            .tracer
            .elements(self.key, variant, fields.len(), visitor);
        if result.is_ok() {
            self.learned(Payload::Struct(Fields {
                names: fields,
                formats,
            }));
        }
        result
    }
}
