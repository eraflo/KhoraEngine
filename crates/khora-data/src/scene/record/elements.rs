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

//! How the reader walks into a sequence: its elements, one after another.

use serde::de::{DeserializeSeed, SeqAccess};

use super::de::{Cx, Reader};
use super::watch::Step;
use super::{Record, RecordError};

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
