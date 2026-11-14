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

//! How the tracer hands a map its one entry.

use serde::de::{DeserializeSeed, MapAccess};

use super::trace::{child, Slot, TraceError, Tracer, MAP};
use super::Format;

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
