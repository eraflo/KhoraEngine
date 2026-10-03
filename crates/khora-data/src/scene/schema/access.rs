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

use serde::de::{
    DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess, Visitor,
};

use super::trace::{child, Slot, TraceError, Tracer, MAP};
use super::{Fields, Format, Key, Payload};

/// The fixed elements of a struct, a tuple or a variant, each in its slot.
pub(super) struct Elements<'t, 's, 'f> {
    pub(super) tracer: &'t mut Tracer<'s>,
    pub(super) parent: Slot,
    pub(super) owner: Key,
    pub(super) variant: &'static str,
    pub(super) next: usize,
    pub(super) count: usize,
    pub(super) formats: &'f mut Vec<Format>,
}

impl<'de> SeqAccess<'de> for Elements<'_, '_, '_> {
    type Error = TraceError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, TraceError> {
        if self.next == self.count {
            return Ok(None);
        }
        let slot = child(self.parent, self.owner, self.variant, self.next);
        self.next += 1;
        let result = self
            .tracer
            .at(slot, |tracer| seed.deserialize(&mut *tracer));
        self.formats
            .push(std::mem::replace(&mut self.tracer.out, Format::Unknown));
        result.map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.count - self.next)
    }
}

/// A sequence of one element — its type traced — or of none, if that
/// element would recurse, or if the run reads in shadow.
pub(super) struct One<'t, 's, 'f> {
    pub(super) tracer: &'t mut Tracer<'s>,
    pub(super) slot: Slot,
    pub(super) done: bool,
    pub(super) format: &'f mut Format,
}

impl<'de> SeqAccess<'de> for One<'_, '_, '_> {
    type Error = TraceError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, TraceError> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(None);
        }
        let result = self
            .tracer
            .at(self.slot, |tracer| seed.deserialize(&mut *tracer));
        *self.format = std::mem::replace(&mut self.tracer.out, Format::Unknown);
        match result {
            Ok(value) => Ok(Some(value)),
            Err(TraceError::Recursion) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(usize::from(!self.done))
    }
}

/// A map of one entry, or of none if its key would recurse, or if the run
/// reads in shadow.
pub(super) struct Entry<'t, 's, 'f> {
    pub(super) tracer: &'t mut Tracer<'s>,
    pub(super) slot: Slot,
    pub(super) done: bool,
    pub(super) key: &'f mut Format,
    pub(super) value: &'f mut Format,
}

impl<'de> MapAccess<'de> for Entry<'_, '_, '_> {
    type Error = TraceError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, TraceError> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(None);
        }
        let slot = child(self.slot, MAP, "", 0);
        let result = self
            .tracer
            .at(slot, |tracer| seed.deserialize(&mut *tracer));
        *self.key = std::mem::replace(&mut self.tracer.out, Format::Unknown);
        match result {
            Ok(key) => Ok(Some(key)),
            Err(TraceError::Recursion) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, TraceError> {
        let slot = child(self.slot, MAP, "", 1);
        let result = self
            .tracer
            .at(slot, |tracer| seed.deserialize(&mut *tracer));
        *self.value = std::mem::replace(&mut self.tracer.out, Format::Unknown);
        result
    }

    fn size_hint(&self) -> Option<usize> {
        Some(usize::from(!self.done))
    }
}

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
