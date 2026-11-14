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

//! How the tracer hands a sequence its one element.

use serde::de::{DeserializeSeed, SeqAccess};

use super::trace::{Slot, TraceError, Tracer};
use super::Format;

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
