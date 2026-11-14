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

//! How the tracer hands a struct, a tuple or a variant its fixed elements,
//! each in its slot.

use serde::de::{DeserializeSeed, SeqAccess};

use super::trace::{child, Slot, TraceError, Tracer};
use super::{Format, Key};

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
