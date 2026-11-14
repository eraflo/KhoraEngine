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

//! The prefabs of a project, read through a shared asset reader.

use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_data::scene::{PrefabSource, SceneRecord};

use crate::asset::{AssetReader, PrefabAsset};
use crate::serialization::PREFAB;

/// The prefabs of a project, read through a shared [`AssetReader`]: on any
/// thread, without the [`AssetService`](crate::asset::AssetService)'s lock,
/// and without its cache — a prefab is decoded from its file each time it is
/// read.
pub struct ReaderPrefabs {
    reader: Arc<AssetReader>,
}

impl ReaderPrefabs {
    /// Reads prefabs through `reader`.
    pub fn new(reader: Arc<AssetReader>) -> Self {
        Self { reader }
    }
}

impl PrefabSource for ReaderPrefabs {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        match self.reader.index().get_metadata(&id) {
            Some(metadata) if metadata.asset_type_name == PREFAB => {}
            Some(metadata) => {
                return Err(format!(
                    "the asset {id} is a {}, not a prefab",
                    metadata.asset_type_name
                ))
            }
            None => return Err(format!("the project holds no asset {id}")),
        }
        let decoded = self
            .reader
            .decode(&id, std::any::TypeId::of::<PrefabAsset>())
            .value?;
        decoded
            .downcast::<PrefabAsset>()
            .map(|prefab| prefab.0)
            .map_err(|_| format!("the asset {id} did not decode as a prefab"))
    }
}
