// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A generic, type-safe storage for loaded asset handles.

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use std::collections::HashMap;

/// The in-memory cache of loaded assets of type `A`, keyed by UUID.
///
/// One handle per UUID: every request for a cached asset gets a clone of it.
#[derive(Default)]
pub struct Assets<A: Asset> {
    storage: HashMap<AssetUUID, AssetHandle<A>>,
}

impl<A: Asset> Clone for Assets<A> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
        }
    }
}

impl<A: Asset> Assets<A> {
    /// Creates a new, empty asset storage.
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
        }
    }

    /// Caches `handle` under `uuid`, replacing any handle already there.
    pub fn insert(&mut self, uuid: AssetUUID, handle: AssetHandle<A>) {
        self.storage.insert(uuid, handle);
    }

    /// Returns the handle cached under `uuid`, if any.
    pub fn get(&self, uuid: &AssetUUID) -> Option<&AssetHandle<A>> {
        self.storage.get(uuid)
    }

    /// Checks if an asset with the specified UUID exists in the storage.
    pub fn contains(&self, uuid: &AssetUUID) -> bool {
        self.storage.contains_key(uuid)
    }

    /// Drops the cached handle for `uuid`, returning it if present.
    ///
    /// Clones already handed out keep the old asset alive until they drop, so
    /// a reader in flight never sees a half-loaded replacement.
    pub fn remove(&mut self, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
        self.storage.remove(uuid)
    }

    /// Returns an iterator over every cached UUID.
    pub fn keys(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.storage.keys().copied()
    }

    /// Returns the number of cached assets.
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Returns `true` if no assets are cached.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::AssetUUID;

    /// Local newtype wrapper so the orphan rule lets us `impl Asset` for it.
    /// `Asset` is just a marker trait (`Send + Sync + 'static`).
    #[derive(Debug, PartialEq)]
    struct TestAsset(String);
    impl Asset for TestAsset {}

    #[test]
    fn remove_drops_cached_handle() {
        let mut assets = Assets::<TestAsset>::new();
        let uuid = AssetUUID::new_v5("textures/foo.png");
        assets.insert(uuid, AssetHandle::new(TestAsset("hello".into())));

        assert!(assets.contains(&uuid));
        let popped = assets.remove(&uuid);
        assert!(popped.is_some());
        assert!(!assets.contains(&uuid));
        // Removing again returns None (idempotent).
        assert!(assets.remove(&uuid).is_none());
    }
}
