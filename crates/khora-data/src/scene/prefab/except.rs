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

//! A prefab source that refuses the prefab being written.

use khora_core::asset::AssetUUID;

use super::super::scene_record::SceneRecord;
use super::PrefabSource;

/// A source that refuses one prefab — the one being written: a prefab never
/// links to itself, so an instance of it inside it is written whole.
pub(super) struct Except<'a> {
    pub(super) source: &'a dyn PrefabSource,
    pub(super) written: AssetUUID,
}

impl PrefabSource for Except<'_> {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        if id == self.written {
            return Err(format!("{id} is the prefab being written"));
        }
        self.source.prefab(id)
    }
}
