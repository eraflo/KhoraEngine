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

//! Decodes a `.kprefab` into the scene record it holds.
//!
//! A prefab is an asset like any other: decoded once and kept by the
//! [`AssetService`](crate::asset::AssetService), so a bullet spawned every
//! frame reads nothing from the disk, and dropped by its `invalidate` when the
//! file changes.

use khora_core::asset::Asset;
use khora_core::scene::SceneFile;
use khora_data::scene::{read_scene_file, SceneRecord};

use crate::asset::{AssetDecoder, DecoderRegistration};

/// A prefab, parsed: the record of the entities it brings.
#[derive(Debug, Clone)]
pub struct PrefabAsset(pub SceneRecord);

impl Asset for PrefabAsset {}

/// Decodes a prefab file into its record.
#[derive(Clone, Default)]
pub struct PrefabDecoder;

impl AssetDecoder<PrefabAsset> for PrefabDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<PrefabAsset, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let file = SceneFile::from_bytes(bytes).map_err(|error| format!("{error:?}"))?;
        let record = read_scene_file(&file).map_err(|error| error.to_string())?;
        Ok(PrefabAsset(record))
    }
}

inventory::submit! {
    DecoderRegistration {
        type_name: "prefab",
        register: |svc| {
            svc.register_decoder::<PrefabAsset>("prefab", PrefabDecoder);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_are_no_scene_file_are_refused() {
        assert!(PrefabDecoder.load(b"not a scene").is_err());
    }
}
