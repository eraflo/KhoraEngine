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

use khora_sdk::prelude::ecs::*;
use khora_sdk::{EditorState, GameWorld};

use crate::project_vfs::ProjectVfs;
use crate::{hot_reload, ops};

/// Resolves a forward-slash relative asset path to its UUID through the open
/// project's identity registry, falling back to the path-derived default when
/// no project is open.
pub(super) fn resolve_asset_uuid(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    rel_fwd: &str,
) -> khora_sdk::khora_core::asset::AssetUUID {
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(pvfs) = pvfs_arc.lock() {
            return pvfs.resolve_uuid(rel_fwd);
        }
    }
    ProjectVfs::uuid_for_rel_path(rel_fwd)
}

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

/// Drains the asset-explorer file operations (new folder / rename / move /
/// delete-to-trash / duplicate). Each routes through [`ProjectVfs`], which keeps
/// the identity registry consistent so references survive renames and moves.
pub fn process_pending_asset_file_ops(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let (create, rename, move_op, delete, duplicate) = match editor_state.lock() {
        Ok(mut s) => (
            s.pending_create_folder.take(),
            s.pending_rename_asset.take(),
            s.pending_move_asset.take(),
            s.pending_delete_asset.take(),
            s.pending_duplicate_asset.take(),
        ),
        Err(_) => return,
    };
    if create.is_none()
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
    let Some((rel, point, parent)) = pending else {
        return;
    };
    let uuid = resolve_asset_uuid(project_vfs, &rel);
    let entity = ops::spawn_mesh_asset(world, uuid, point, &rel);
    // Parent under the hierarchy row it was dropped on, if any.
    if let Some(parent) = parent {
        world.set_parent(entity, Some(parent));
    }
    if let Ok(mut s) = editor_state.lock() {
        s.select(entity);
    }
    log::info!("Spawned mesh '{rel}' as entity {entity:?} at {point:?}");
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
    let Some((rel, entity)) = pending else {
        return;
    };
    let uuid = resolve_asset_uuid(project_vfs, &rel);

    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "kmat" | "mat" => {
            world.add_component(entity, MaterialRef::Asset(uuid));
            log::info!("Assigned material '{rel}' to entity {entity:?}");
        }
        "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" => {
            let std_mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color_texture: Some(uuid),
                ..Default::default()
            };
            world.add_component(entity, MaterialRef::inline(Box::new(std_mat)));
            log::info!("Assigned texture '{rel}' to entity {entity:?} (base color)");
        }
        _ => {
            log::warn!("Drop of '{rel}' on entity {entity:?} ignored: not a texture or material")
        }
    }
}
