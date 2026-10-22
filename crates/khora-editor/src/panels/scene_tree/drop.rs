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

//! Drag and drop onto the tree: entity payloads and dropped assets.

use khora_sdk::editor_ui::*;

use crate::ops::AssetDrop;

/// Routes an asset dropped onto the hierarchy to the right `pending_*` action,
/// mirroring the viewport's drop dispatch. Meshes spawn at the world origin
/// (a tree has no 3D drop point), prefabs instantiate, scenes load, and a
/// texture/material assigns to the `target` row (or the selection).
pub(super) fn dispatch_asset_drop(
    state: &mut EditorState,
    idx: usize,
    target: Option<khora_sdk::prelude::ecs::EntityId>,
) {
    let Some(entry) = state.asset_entries.get(idx).cloned() else {
        return;
    };
    let under = if target.is_some() {
        " as child"
    } else {
        " at root"
    };
    match crate::ops::asset_drop(&state.asset_entries, &entry) {
        AssetDrop::Mesh(uuid) => {
            // Dropped on a row → parent the new entity under it (Unity/Godot
            // convention); dropped on empty space → spawn at scene root.
            state.pending_spawn_mesh_asset = Some((uuid, [0.0, 0.0, 0.0], target));
            log::info!("Hierarchy: mesh '{}' dropped — spawning{under}", entry.name);
        }
        AssetDrop::Prefab(uuid) => {
            state.pending_prefab_spawn = Some((uuid, target));
            log::info!("Hierarchy: '{}' dropped — instantiating{under}", entry.name);
        }
        AssetDrop::Scene(rel) => {
            if let Some(pf) = state.project_folder.clone() {
                let abs = std::path::Path::new(&pf)
                    .join("assets")
                    .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                state.pending_scene_load = Some(abs.to_string_lossy().to_string());
                log::info!("Hierarchy: scene '{}' dropped — loading", entry.name);
            } else {
                log::warn!("Hierarchy: cannot load scene '{}' — no project folder", rel);
            }
        }
        AssetDrop::Assign(uuid) => {
            match target.or_else(|| state.selection.iter().copied().next()) {
                Some(entity) => {
                    state.pending_assign_texture = Some((uuid, entity));
                    log::info!("Hierarchy: '{}' dropped — assigning", entry.name);
                }
                None => log::warn!(
                    "Hierarchy: drop '{}' on a row, or select an entity, to assign it",
                    entry.name
                ),
            }
        }
        AssetDrop::Ignored => log::info!(
            "Hierarchy: asset type '{}' is not droppable here",
            entry.asset_type
        ),
    }
}
