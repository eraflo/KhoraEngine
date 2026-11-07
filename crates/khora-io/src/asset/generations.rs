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

//! When each asset last changed: what a decode off the frame holds to.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use khora_core::asset::AssetUUID;

/// When each asset last changed, on one counter: an invalidated asset moves
/// to the counter's next value, and a new index moves every asset. A decode
/// filed at one value of the counter is installed only while none of the
/// files it read changed after it.
///
/// Shared between the service and the leases of the decodes in flight. Its
/// lock is a leaf: held only to read or move a generation.
#[derive(Debug, Default)]
pub(crate) struct Generations(Mutex<State>);

#[derive(Debug, Default)]
struct State {
    /// The last generation handed out.
    counter: u64,
    /// The generation each asset was last moved to.
    by_asset: HashMap<AssetUUID, u64>,
    /// The generation every asset was moved to by the last new index.
    all: u64,
    /// The decodes a lease turned down since they were asked for again, by
    /// asset and type.
    abandoned: HashSet<(AssetUUID, TypeId)>,
}

impl Generations {
    /// The counter now: what a decode filed now is stamped with.
    pub(crate) fn now(&self) -> u64 {
        self.lock().counter
    }

    /// Whether none of `files` changed after `filed`.
    pub(crate) fn none_changed_since(
        &self,
        files: impl IntoIterator<Item = AssetUUID>,
        filed: u64,
    ) -> bool {
        files.into_iter().all(|file| self.of(&file) <= filed)
    }

    /// Notes that the decode of `uuid` as `type_id` was turned down: the
    /// next request for it files it again.
    pub(crate) fn abandon(&self, uuid: AssetUUID, type_id: TypeId) {
        self.lock().abandoned.insert((uuid, type_id));
    }

    /// Whether the decode of `uuid` as `type_id` was turned down since it
    /// was asked for, forgetting it.
    pub(crate) fn take_abandoned(&self, uuid: AssetUUID, type_id: TypeId) -> bool {
        self.lock().abandoned.remove(&(uuid, type_id))
    }

    /// Whether nothing changed at all after `filed`.
    pub(crate) fn nothing_changed_since(&self, filed: u64) -> bool {
        self.now() <= filed
    }

    /// The generation of `uuid` now: the counter's value when it last
    /// changed.
    pub(crate) fn of(&self, uuid: &AssetUUID) -> u64 {
        let state = self.lock();
        state
            .by_asset
            .get(uuid)
            .copied()
            .unwrap_or(0)
            .max(state.all)
    }

    /// Moves `uuid` to a new generation: its bytes changed.
    pub(crate) fn bump(&self, uuid: &AssetUUID) {
        let mut state = self.lock();
        state.counter += 1;
        let generation = state.counter;
        state.by_asset.insert(*uuid, generation);
    }

    /// Moves every asset to a new generation: the index was replaced.
    pub(crate) fn bump_all(&self) {
        let mut state = self.lock();
        state.counter += 1;
        state.all = state.counter;
    }

    // Whole after every write: read through the poison.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_asset_moves_alone_and_a_new_index_moves_them_all() {
        let generations = Generations::default();
        let (a, b) = (AssetUUID::new_v5("a"), AssetUUID::new_v5("b"));
        let (a0, b0) = (generations.of(&a), generations.of(&b));

        generations.bump(&a);
        assert_ne!(generations.of(&a), a0);
        assert_eq!(generations.of(&b), b0, "another asset stays");

        let a1 = generations.of(&a);
        generations.bump_all();
        assert_ne!(generations.of(&a), a1);
        assert_ne!(generations.of(&b), b0);
    }
}
