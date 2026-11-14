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

//! The prefabs of a project, read through its asset service.

use std::sync::{Arc, Mutex};

use khora_core::asset::AssetUUID;
use khora_data::scene::{PrefabSource, SceneRecord};

use crate::asset::AssetService;
use crate::serialization::read_prefab;

/// The prefabs of a project, read through its [`AssetService`]: a prefab is
/// the `.kprefab` its asset id names, decoded once and kept until the service
/// invalidates it.
pub struct AssetPrefabs {
    assets: Arc<Mutex<AssetService>>,
}

impl AssetPrefabs {
    /// Reads prefabs through `assets`.
    pub fn new(assets: Arc<Mutex<AssetService>>) -> Self {
        Self { assets }
    }
}

impl PrefabSource for AssetPrefabs {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        let mut assets = self
            .assets
            .lock()
            .map_err(|_| "the asset service is poisoned".to_owned())?;
        read_prefab(&mut assets, id)
    }
}
