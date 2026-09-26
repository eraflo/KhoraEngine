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
    let rel = entry.source_path.clone();
    match entry.asset_type.as_str() {
        "mesh" => {
            // Dropped on a row → parent the new entity under it (Unity/Godot
            // convention); dropped on empty space → spawn at scene root.
            state.pending_spawn_mesh_asset = Some((rel, [0.0, 0.0, 0.0], target));
            log::info!(
                "Hierarchy: mesh '{}' dropped — spawning{}",
                entry.name,
                if target.is_some() {
                    " as child"
                } else {
                    " at root"
                }
            );
        }
        "prefab" => {
            state.pending_prefab_spawn = Some((rel, target));
            log::info!(
                "Hierarchy: prefab '{}' dropped — instantiating{}",
                entry.name,
                if target.is_some() {
                    " as child"
                } else {
                    " at root"
                }
            );
        }
        "scene" => {
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
        "texture" | "material" => match target.or_else(|| state.selection.iter().copied().next()) {
            Some(entity) => {
                state.pending_assign_texture = Some((rel, entity));
                log::info!("Hierarchy: '{}' dropped — assigning to entity", entry.name);
            }
            None => log::warn!("Hierarchy: drop '{}' on an entity to assign it", entry.name),
        },
        other => log::info!("Hierarchy: asset type '{other}' is not droppable here"),
    }
}

/// Packs an `EntityId` into a `u64` for drag-and-drop payloads. Layout:
/// high 32 = generation, low 32 = index. Also used by the asset
/// browser's drop handler to ingest entity drags from the scene tree
/// (drop = create prefab in the current folder).
pub(crate) fn pack_entity(e: khora_sdk::prelude::ecs::EntityId) -> u64 {
    ((e.generation as u64) << 32) | (e.index as u64)
}

/// Inverse of [`pack_entity`].
pub(crate) fn unpack_entity(payload: u64) -> khora_sdk::prelude::ecs::EntityId {
    khora_sdk::prelude::ecs::EntityId {
        index: payload as u32,
        generation: (payload >> 32) as u32,
    }
}

/// `true` when `payload` is *not* one of the asset-browser's
/// dedicated drag tags (the type-agnostic asset-tile tag). Lets receivers
/// disambiguate scene-tree entity payloads from asset-tile payloads on
/// the same `dnd_take_drop_payload` channel.
///
/// The check is conservative: any payload whose top 32 bits don't
/// match a known tag is assumed to be a packed `EntityId`. Entity
/// generations are tiny u32s (start at 1) so this can't collide with
/// the ASCII-encoded tag constants in
/// [`crate::panels::asset_browser`].
pub(crate) fn payload_is_entity(payload: u64) -> bool {
    !crate::panels::asset_browser::is_asset_drag(payload)
}
