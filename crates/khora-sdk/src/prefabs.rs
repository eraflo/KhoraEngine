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

//! The prefabs a running game spawns from: where the frame boundary reads
//! them, and the catalog a script's `Spawn` is checked against in its turn.

use std::sync::{Arc, Mutex};

use khora_core::Runtime;
use khora_data::scene::{PrefabCatalog, PrefabLibrary};
use khora_io::asset::AssetService;
use khora_io::serialization::{prefab_catalog, AssetPrefabs};

/// Makes the prefabs `assets` holds the ones a running game's scripts spawn.
///
/// Installs the library the frame boundary reads and the catalog a script's
/// `Spawn` is checked against, keeping either the runtime already has, and
/// fills the catalog from the same asset index.
pub fn install(runtime: &mut Runtime, assets: Arc<Mutex<AssetService>>) {
    let resources = &mut runtime.resources;
    if resources.get::<PrefabLibrary>().is_none() {
        resources.insert(PrefabLibrary(Arc::new(AssetPrefabs::new(Arc::clone(
            &assets,
        )))));
    }
    if resources.get::<PrefabCatalog>().is_none() {
        resources.insert(PrefabCatalog::new());
    }
    if let Some(catalog) = runtime.resources.get::<PrefabCatalog>() {
        fill(catalog, &assets);
    }
}

/// Fills the prefab catalog again from the runtime's asset service.
///
/// Call it after the project's assets were indexed anew, so a prefab added
/// since can be spawned.
pub fn refresh(runtime: &Runtime) {
    if let (Some(catalog), Some(assets)) = (
        runtime.resources.get::<PrefabCatalog>(),
        runtime.services.get::<Arc<Mutex<AssetService>>>(),
    ) {
        fill(catalog, assets);
    }
}

/// Replaces `catalog`'s prefabs with those `assets` indexes; logs a poisoned service.
fn fill(catalog: &PrefabCatalog, assets: &Mutex<AssetService>) {
    match assets.lock() {
        Ok(assets) => catalog.replace(prefab_catalog(&assets)),
        Err(_) => {
            log::error!("the asset service is poisoned: the prefab catalog was not refreshed")
        }
    }
}
