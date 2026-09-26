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

//! Saving an entity as a prefab and spawning one.

use std::sync::{Arc, Mutex};

use khora_sdk::prelude::ecs::*;
use khora_sdk::{instantiate_subtree, serialize_subtree, EditorState, GameWorld};

use crate::project_vfs::ProjectVfs;
use crate::scene_io;

/// Drains [`EditorState::pending_save_as_prefab`] and
/// [`EditorState::pending_save_as_prefab_at`], writing the entity's
/// subtree as a `.kprefab` (Recipe-encoded).
///
/// - The `_at` variant carries a pre-chosen forward-slash relative
///   path under `<project>/assets/`; the dispatcher writes directly
///   without showing a dialog. Set by drag-drop (entity → asset
///   browser folder).
/// - The plain variant opens an `rfd::FileDialog`. Set by the scene
///   tree's "Save as Prefab…" context entry.
pub fn process_pending_save_as_prefab(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    // Drag-drop path: pre-chosen folder, no dialog.
    let auto = match editor_state.lock() {
        Ok(mut s) => s.pending_save_as_prefab_at.take(),
        Err(_) => None,
    };
    if let Some((entity, rel_path)) = auto {
        save_prefab_to_project_path(project_vfs, world, entity, &rel_path);
    }

    // Right-click path: file dialog.
    let entity = match editor_state.lock() {
        Ok(mut s) => s.pending_save_as_prefab.take(),
        Err(_) => None,
    };
    let Some(entity) = entity else {
        return;
    };

    let bytes = match serialize_subtree(world.inner_world(), entity) {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to serialize prefab subtree: {:?}", e);
            return;
        }
    };

    let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Prefab", &["kprefab"])
        .set_file_name("prefab.kprefab")
        .save_file()
    else {
        return;
    };
    let abs = path.clone();
    let path_str = path.to_string_lossy().to_string();

    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            let assets_root = pvfs.assets_root.clone();
            if let Some(rel_fwd) = scene_io::rel_inside_project(&abs, &assets_root) {
                write_prefab_through_vfs(&mut pvfs, &rel_fwd, &bytes);
                return;
            }
        }
    }

    match std::fs::write(&path_str, &bytes) {
        Ok(()) => log::warn!(
            "Prefab saved to '{}' ({} bytes) — outside project, not VFS-managed.",
            path_str,
            bytes.len()
        ),
        Err(e) => log::error!("Failed to write prefab '{}': {}", path_str, e),
    }
}

fn save_prefab_to_project_path(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    entity: EntityId,
    rel_path: &str,
) {
    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Prefab drop ignored: no project is open");
        return;
    };
    let bytes = match serialize_subtree(world.inner_world(), entity) {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to serialize prefab subtree: {:?}", e);
            return;
        }
    };
    let Ok(mut pvfs) = pvfs_arc.lock() else {
        log::error!("Project VFS mutex poisoned");
        return;
    };
    write_prefab_through_vfs(&mut pvfs, rel_path, &bytes);
}

fn write_prefab_through_vfs(pvfs: &mut ProjectVfs, rel_fwd: &str, bytes: &[u8]) {
    if let Err(e) = pvfs.write_asset(std::path::Path::new(rel_fwd), bytes) {
        log::error!("Failed to write prefab '{}': {:#}", rel_fwd, e);
        return;
    }
    if let Err(e) = pvfs.rebuild_index() {
        log::warn!("Prefab saved but index rebuild failed: {:#}", e);
    }
    log::info!("Prefab saved to '{}' ({} bytes)", rel_fwd, bytes.len());
}

/// Drains [`EditorState::pending_prefab_spawn`] and instantiates the
/// referenced `.kprefab` into the live world via
/// [`instantiate_subtree`]. The forward-slash relative path resolves
/// through the project's VFS / `AssetService`.
pub fn process_pending_prefab_spawn(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let pending = match editor_state.lock() {
        Ok(mut s) => s.pending_prefab_spawn.take(),
        Err(_) => None,
    };
    let Some((rel, parent)) = pending else {
        return;
    };

    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Prefab spawn requested ('{}') but no project is open", rel);
        return;
    };

    let bytes = {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Project VFS mutex poisoned");
            return;
        };
        let uuid = pvfs.resolve_uuid(&rel);
        match pvfs.asset_service.load_raw(&uuid) {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to read prefab '{}': {:#}", rel, e);
                return;
            }
        }
    };

    match instantiate_subtree(world.inner_world_mut(), &bytes) {
        Ok(new_root) => {
            // Parent under the hierarchy row it was dropped on, if any.
            if let Some(parent) = parent {
                world.set_parent(new_root, Some(parent));
            }
            log::info!(
                "Prefab '{}' instantiated (root entity index={}{})",
                rel,
                new_root.index,
                if parent.is_some() { ", parented" } else { "" }
            );
        }
        Err(e) => log::error!("Failed to instantiate prefab '{}': {:?}", rel, e),
    }
}
