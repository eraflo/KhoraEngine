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

/// What a query's items are fetched with: a write tick, the writable terms, and a `since` tick.
///
/// A write carries the world's [`current_tick`](crate::ecs::World::current_tick);
/// the [`Changed`](super::Changed) terms compare against `since`.
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
    /// Returns the context of a query that writes nothing.
    pub(crate) fn read_only(tick: Tick, since: Tick) -> Self {
        FetchContext {
            tick,
            writable: &[],
            since,
        }
    }

    /// Returns whether `key` is observed, and its domain's change epoch.
    ///
    /// A key that is not a writable term gets a null epoch, which no write raises.
    pub(super) fn writable(&self, key: ComponentKey) -> (bool, *mut u64) {
        self.writable
            .iter()
            .find(|w| w.key == key)
            .map_or((false, std::ptr::null_mut()), |w| (w.observed, w.epoch))
    }
}
