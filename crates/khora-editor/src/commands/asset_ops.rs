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

//! File operations on assets, and assigning or spawning them.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::prelude::ecs::*;
use khora_sdk::{AssetSource, EditorState, GameWorld};

use crate::project_vfs::ProjectVfs;
use crate::{hot_reload, ops};

/// Recomputes the asset-browser cache (entries + directories) after a file
/// operation and bumps the epoch so the panel rescans its flattened view.
fn refresh_asset_cache(pvfs_arc: &Arc<Mutex<ProjectVfs>>, editor_state: &Arc<Mutex<EditorState>>) {
    let refreshed = match pvfs_arc.lock() {
        Ok(pvfs) => Some((hot_reload::collect_asset_entries(&pvfs), pvfs.list_dirs())),
        Err(_) => None,
    };
    if let Some((entries, dirs)) = refreshed {
        if let Ok(mut s) = editor_state.lock() {
            s.asset_entries = entries;
            s.asset_dirs = dirs;
            s.asset_epoch = s.asset_epoch.wrapping_add(1);
        }
    }
}

/// Drains the asset-explorer file operations (new folder / new script / rename /
/// move / delete-to-trash / duplicate). Each routes through [`ProjectVfs`], which
/// keeps the identity registry consistent so references survive renames and
/// moves. A script written is then revealed, so the browser selects it.
pub fn process_pending_asset_file_ops(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let (create, script, rename, move_op, delete, duplicate) = match editor_state.lock() {
        Ok(mut s) => (
            s.pending_create_folder.take(),
            s.pending_create_script.take(),
            s.pending_rename_asset.take(),
            s.pending_move_asset.take(),
            s.pending_delete_asset.take(),
            s.pending_duplicate_asset.take(),
        ),
        Err(_) => return,
    };
    if create.is_none()
        && script.is_none()
        && rename.is_none()
        && move_op.is_none()
        && delete.is_none()
        && duplicate.is_none()
    {
        return;
    }
    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Asset file operation ignored: no project is open");
        return;
    };

    let mut written_script = None;
    {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Asset file op: project VFS mutex poisoned");
            return;
        };
        if let Some(dir) = create {
            match pvfs.create_folder(&dir) {
                Ok(()) => log::info!("Created folder '{dir}'"),
                Err(e) => log::error!("Create folder '{dir}' failed: {e:#}"),
            }
        }
        if let Some((rel, source)) = script {
            match pvfs.create_asset(&rel, source.as_bytes()) {
                Ok(()) => {
                    log::info!("Created script '{rel}'");
                    written_script = Some(rel);
                }
                Err(e) => log::error!("Create script '{rel}' failed: {e:#}"),
            }
        }
        if let Some((old, new)) = rename {
            match pvfs.rename_asset(&old, &new) {
                Ok(()) => log::info!("Renamed '{old}' → '{new}'"),
                Err(e) => log::error!("Rename '{old}' → '{new}' failed: {e:#}"),
            }
        }
        if let Some((src, dest)) = move_op {
            match pvfs.move_asset(&src, &dest) {
                Ok(()) => log::info!("Moved '{src}' → '{dest}/'"),
                Err(e) => log::error!("Move '{src}' → '{dest}' failed: {e:#}"),
            }
        }
        if let Some(rel) = delete {
            match pvfs.delete_to_trash(&rel) {
                Ok(()) => log::info!("Moved '{rel}' to the recycle bin"),
                Err(e) => log::error!("Delete '{rel}' failed: {e:#}"),
            }
        }
        if let Some(rel) = duplicate {
            match pvfs.duplicate_asset(&rel) {
                Ok(new_rel) => log::info!("Duplicated '{rel}' → '{new_rel}'"),
                Err(e) => log::error!("Duplicate '{rel}' failed: {e:#}"),
            }
        }
    }

    refresh_asset_cache(pvfs_arc, editor_state);
    if let Some(rel) = written_script {
        if let Ok(mut s) = editor_state.lock() {
            s.pending_reveal_asset = Some(rel);
        }
    }
}

/// Drains [`EditorState::pending_spawn_mesh_asset`]: spawns a `MeshRef::Asset`
/// entity at the drop point. Set by dragging a mesh tile onto the viewport; the
/// `asset_resolver_system` loads the mesh next tick.
pub fn process_pending_spawn_mesh_asset(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let pending = match editor_state.lock() {
        Ok(mut s) => s.pending_spawn_mesh_asset.take(),
        Err(_) => None,
    };
    let Some((uuid, point, parent)) = pending else {
        return;
    };
    let name = indexed_asset(project_vfs, uuid)
        .map_or_else(|| "Mesh".to_owned(), |asset| asset.display_name);
    let entity = ops::spawn_mesh_asset(world, uuid, point, &name);
    // Parent under the hierarchy row it was dropped on, if any.
    if let Some(parent) = parent {
        world.set_parent(entity, Some(parent));
    }
    if let Ok(mut s) = editor_state.lock() {
        s.select(entity);
    }
    log::info!("Spawned mesh '{name}' as entity {entity:?} at {point:?}");
}

/// An asset of the open project, as the editor names and dispatches it.
struct IndexedAsset {
    /// The index type (`"mesh"`, `"material"`, `"texture"`…).
    type_name: String,
    /// The file's stem, followed by the part's label for a part of a file
    /// (`ship mesh1/prim0`).
    display_name: String,
}

/// `uuid` as the open project's index knows it, if it does.
fn indexed_asset(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    uuid: AssetUUID,
) -> Option<IndexedAsset> {
    let pvfs = project_vfs?.lock().ok()?;
    let metadata = pvfs.asset_service.index().get_metadata(&uuid)?;
    let stem = metadata
        .source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let display_name = match metadata.variants.get("default") {
        Some(AssetSource::Sub { label, .. }) => format!("{stem} {label}"),
        _ => stem,
    };
    Some(IndexedAsset {
        type_name: metadata.asset_type_name.clone(),
        display_name,
    })
}

/// Drains [`EditorState::pending_assign_texture`]: assigns a dropped texture or
/// `.kmat` to a specific entity. A `.kmat` becomes a `MaterialRef::Asset`; an
/// image becomes the `base_color_texture` of a fresh inline standard material.
pub fn process_pending_assign_texture(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let pending = match editor_state.lock() {
        Ok(mut s) => s.pending_assign_texture.take(),
        Err(_) => None,
    };
    let Some((uuid, entity)) = pending else {
        return;
    };
    let Some(asset) = indexed_asset(project_vfs, uuid) else {
        log::warn!(
            "Drop of asset {uuid} on entity {entity:?} ignored: the project has no such asset"
        );
        return;
    };
    let name = &asset.display_name;
    match asset.type_name.as_str() {
        "material" => {
            world.add_component(entity, MaterialRef::Asset(uuid));
            log::info!("Assigned material '{name}' to entity {entity:?}");
        }
        "texture" => {
            let std_mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color_texture: Some(uuid),
                ..Default::default()
            };
            world.add_component(entity, MaterialRef::inline(Box::new(std_mat)));
            log::info!("Assigned texture '{name}' to entity {entity:?} (base color)");
        }
        other => log::warn!(
            "Drop of '{name}' on entity {entity:?} ignored: a {other}, not a texture or material"
        ),
    }
}
