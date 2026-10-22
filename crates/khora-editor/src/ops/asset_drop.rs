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

//! What dropping an asset asks for, wherever it is dropped.
//!
//! The viewport and the hierarchy apply the same rules; only where the result
//! lands differs (a drop point, a row). A request names the asset by its
//! identity: a path cannot name a part inside a file.

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::asset::AssetUUID;

/// The request a dropped asset makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetDrop {
    /// Instantiate this prefab: a `.kprefab`, a glTF scene part, or the
    /// default scene of a dropped glTF file.
    Prefab(AssetUUID),
    /// Spawn one entity showing this mesh.
    Mesh(AssetUUID),
    /// Give this material or texture to an entity.
    Assign(AssetUUID),
    /// Open the scene file at this path (relative to `assets/`).
    Scene(String),
    /// Nothing: the asset's type cannot be dropped.
    Ignored,
}

/// What dropping `entry` asks for, among the project's `entries`.
///
/// A glTF file whose default scene is listed places that scene, node by node
/// with their materials; any other mesh — an OBJ, a glTF primitive part, a
/// glTF whose scene is not listed — spawns as one mesh.
pub fn asset_drop(entries: &[AssetEntry], entry: &AssetEntry) -> AssetDrop {
    match entry.asset_type.as_str() {
        "mesh" => {
            if entry.container.is_none() {
                let scene = AssetUUID::sub_asset(entry.uuid, "scene");
                if entries.iter().any(|listed| listed.uuid == scene) {
                    return AssetDrop::Prefab(scene);
                }
            }
            AssetDrop::Mesh(entry.uuid)
        }
        "prefab" => AssetDrop::Prefab(entry.uuid),
        "texture" | "material" => AssetDrop::Assign(entry.uuid),
        "scene" => AssetDrop::Scene(entry.source_path.clone()),
        _ => AssetDrop::Ignored,
    }
}
