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

//! The project's prefabs, as a load or a save reads them.

use khora_sdk::khora_core::asset::AssetUUID;

use crate::project_vfs::ProjectVfs;

/// The project's prefabs, read through its VFS: a prefab is the scene file its asset id names.
///
/// Locks the VFS for each read, so it can be handed to a load or a save that does not hold it.
pub struct ProjectPrefabs<'a>(pub &'a std::sync::Mutex<ProjectVfs>);

impl khora_sdk::khora_data::scene::PrefabSource for ProjectPrefabs<'_> {
    fn prefab(
        &self,
        id: AssetUUID,
    ) -> std::result::Result<khora_sdk::khora_data::scene::SceneRecord, String> {
        self.0
            .lock()
            .map_err(|_| "the project VFS is poisoned".to_owned())?
            .prefab_record(id)
    }
}
