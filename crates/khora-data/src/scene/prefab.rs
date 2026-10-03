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

//! Prefabs and duplicates: a subtree written down and brought back fresh.

use khora_core::ecs::entity::EntityId;
use khora_core::scene::SceneFile;

use super::apply::{apply, Identity, LoadFailure};
use super::capture::{capture_subtree, SaveError};
use super::encoding::CompactEncoding;
use super::file::{read_scene_file, write_scene_file};
use super::record::LoadReport;
use crate::ecs::World;

/// `root` and everything under it, as the bytes of a scene file.
pub fn serialize_subtree(world: &World, root: EntityId) -> Result<Vec<u8>, SaveError> {
    let record = capture_subtree(world, root)?;
    write_scene_file(&record, &CompactEncoding)
        .map(|file| file.to_bytes())
        .map_err(|error| SaveError::Encoding(error.to_string()))
}

/// Brings a subtree written by [`serialize_subtree`] into `world` as a new
/// instance — new authored identities — and returns its root.
pub fn instantiate_subtree(world: &mut World, bytes: &[u8]) -> Result<EntityId, LoadFailure> {
    let failed = |message: String| LoadFailure {
        message,
        report: LoadReport::default(),
    };
    let file = SceneFile::from_bytes(bytes).map_err(|error| failed(format!("{error:?}")))?;
    let record = read_scene_file(&file).map_err(|error| failed(error.to_string()))?;
    let applied = apply(world, &record, Identity::Fresh)?;
    applied
        .entities
        .first()
        .map(|(_, root)| *root)
        .ok_or_else(|| failed("the prefab holds no entity".to_owned()))
}
