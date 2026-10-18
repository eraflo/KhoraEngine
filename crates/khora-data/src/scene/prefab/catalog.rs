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

//! The set of prefabs a project holds, for a script to check its spawns
//! against in its turn.

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError, RwLock};

use khora_core::asset::AssetUUID;

/// The prefab identities a project holds: a `Runtime` resource, replaced
/// whole when the project's assets are indexed again.
///
/// Read through a shared snapshot: a frame takes the set as it is and keeps
/// it, so a reindex mid-frame never changes what one turn sees. A clone is
/// the same catalog — what the runtime holds and what refreshes it share it.
#[derive(Debug, Default, Clone)]
pub struct PrefabCatalog {
    prefabs: Arc<RwLock<Arc<BTreeSet<AssetUUID>>>>,
}

impl PrefabCatalog {
    /// A catalog holding no prefab.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `prefabs` the catalog's whole content.
    pub fn replace(&self, prefabs: impl IntoIterator<Item = AssetUUID>) {
        let prefabs = Arc::new(prefabs.into_iter().collect());
        // A panic while replacing leaves a whole set, the old or the new.
        *self.prefabs.write().unwrap_or_else(PoisonError::into_inner) = prefabs;
    }

    /// Whether the catalog holds the prefab `uuid`.
    pub fn holds(&self, uuid: AssetUUID) -> bool {
        self.snapshot().contains(&uuid)
    }

    /// The catalog as it is now, shared.
    pub fn snapshot(&self) -> Arc<BTreeSet<AssetUUID>> {
        Arc::clone(&self.prefabs.read().unwrap_or_else(PoisonError::into_inner))
    }
}
