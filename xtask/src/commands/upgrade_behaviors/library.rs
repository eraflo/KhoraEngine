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

//! A project's prefabs, held in memory by asset id: the files as they were,
//! to read instances against, and as they are converted, to write against.

use std::collections::HashMap;

use khora_core::asset::AssetUUID;
use khora_sdk::khora_data::scene::{PrefabSource, SceneRecord};

/// Records by asset id.
#[derive(Debug, Clone, Default)]
pub(super) struct Library(pub(super) HashMap<AssetUUID, SceneRecord>);

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?} in the project"))
    }
}
