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

//! `FetchContext`: what a query's items are fetched with.

use crate::ecs::{ComponentKey, Tick};

/// What a query's items are fetched with: the tick a write through a
/// writable item carries — the world's [`current_tick`](crate::ecs::World::current_tick)
/// —, the query's writable terms (whether some observer watches each, whose
/// columns then gain row ticks on their first write, and its domain's change
/// epoch), and the tick its
/// [`Changed`](super::Changed) terms compare against.
#[derive(Debug, Clone, Copy)]
pub struct FetchContext<'s> {
    pub(crate) tick: Tick,
    pub(crate) writable: &'s [Writable],
    pub(crate) since: Tick,
}

/// One writable term's key, whether some observer watches it, and the change
/// epoch of its domain, which a write through it raises.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Writable {
    pub(crate) key: ComponentKey,
    pub(crate) observed: bool,
    pub(crate) epoch: *mut u64,
}

impl FetchContext<'_> {
    /// The context of a query that writes nothing.
    pub(crate) fn read_only(tick: Tick, since: Tick) -> Self {
        FetchContext {
            tick,
            writable: &[],
            since,
        }
    }

    /// Whether `key` is observed, and its domain's change epoch. Every
    /// writable term of a `query_mut` is listed; a term listed nowhere — a
    /// read-only query fetches none — raises no epoch.
    pub(super) fn writable(&self, key: ComponentKey) -> (bool, *mut u64) {
        self.writable
            .iter()
            .find(|w| w.key == key)
            .map_or((false, std::ptr::null_mut()), |w| (w.observed, w.epoch))
    }
}
