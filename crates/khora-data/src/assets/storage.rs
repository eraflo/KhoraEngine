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

/// A central, in-memory cache for a specific type of asset `A`.
///
/// This structure maps a unique `AssetUUID` to a shared `AssetHandle<A>`.
/// This ensures that any given asset is loaded only once. Subsequent requests
/// for the same asset will receive a clone of the cached handle.
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

    /// Inserts an asset handle into the storage, associated with its UUID.
    /// If an asset with the same UUID already exists, it will be replaced.
    /// This operation is always successful.
    ///
    /// # Arguments
    /// * `uuid` - The unique identifier for the asset.
    /// * `handle` - The handle to the asset to be stored.
    pub fn insert(&mut self, uuid: AssetUUID, handle: AssetHandle<A>) {
        self.storage.insert(uuid, handle);
    }

    /// Retrieves a reference to the asset handle associated with the given UUID.
    /// Returns `None` if no asset with the specified UUID is found.
    pub fn get(&self, uuid: &AssetUUID) -> Option<&AssetHandle<A>> {
        self.storage.get(uuid)
    }

    /// Checks if an asset with the specified UUID exists in the storage.
    pub fn contains(&self, uuid: &AssetUUID) -> bool {
        self.storage.contains_key(uuid)
    }
}
