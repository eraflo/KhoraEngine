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

//! Saving, loading and building the project's scene, and opening a project.

use std::sync::{Arc, Mutex};

use khora_sdk::{EditorState, GameWorld, SerializationGoal};

use crate::project_vfs::ProjectVfs;
use crate::{build_game, hot_reload, scene_io};

/// Save dispatch: routes through the project VFS when the target path
/// lives under `<project>/assets/`, falls back to direct `std::fs` for
/// arbitrary out-of-project Save-As destinations. Saves under the
/// `EditorInterchange` goal (the compact encoding).
/// Returns whether the scene reached disk. Failures are logged by the write
/// paths themselves; the flag lets a caller react instead of assuming success.
pub fn save_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    path_str: &str,
) -> bool {
    save_scene_dispatch_with_goal(
        project_vfs,
        world,
        path_str,
        SerializationGoal::EditorInterchange,
    )
}

/// Same as [`save_scene_dispatch`] but with an explicit serialization
/// goal — `HumanReadableDebug` for a JSON scene, `PortableBinary` for
/// MessagePack.
pub fn save_scene_dispatch_with_goal(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    path_str: &str,
    goal: SerializationGoal,
) -> bool {
    let abs = std::path::Path::new(path_str);
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            let assets_root = pvfs.assets_root.clone();
            if let Some(rel_fwd) = scene_io::rel_inside_project(abs, &assets_root) {
                // `save_scene_in_project_with_goal` logs its own failures; the
                // flag is propagated so a caller can react to the outcome
                // rather than assume success.
                return scene_io::save_scene_in_project_with_goal(
                    &mut pvfs,
                    world,
                    std::path::Path::new(&rel_fwd),
                    goal,
                );
            }
        }
    }
    scene_io::save_scene_to_path_with_goal(world, path_str, goal)
}

/// Load dispatch: same shape as `save_scene_dispatch`.
/// Loads a scene, clearing every entity reference the editor still holds.
///
/// The clearing happens here rather than at each call site: loading always
/// repopulates the world with fresh ids, so a caller that forgot would leave a
/// selection naming entities from the previous scene — and once a slot is
/// recycled, naming *different* ones.
pub fn load_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    abs: &std::path::Path,
) {
    if let Ok(mut state) = editor_state.lock() {
        state.clear_entity_references();
    }
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            let assets_root = pvfs.assets_root.clone();
            if let Some(rel_fwd) = scene_io::rel_inside_project(abs, &assets_root) {
                scene_io::load_scene_in_project(&mut pvfs, world, &rel_fwd);
                return;
            }
        }
    }
    scene_io::load_scene_from_path(world, &abs.to_string_lossy());
}

/// Build Game dispatch: packs the project's assets, copies the host-target
/// `khora-runtime` binary into `<project>/dist/<target>/`, and writes the
/// runtime config so the staged binary auto-loads the project's default
/// scene. Reports progress + final path through the editor's logger.
pub fn run_build_game(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let Some(pvfs_arc) = project_vfs else {
        log::error!("Build Game: no project is open");
        return;
    };
    let project_name = editor_state
        .lock()
        .ok()
        .and_then(|s| s.project_name.clone())
        .unwrap_or_else(|| "Game".to_owned());

    let pvfs = match pvfs_arc.lock() {
        Ok(p) => p,
        Err(_) => {
            log::error!("Build Game: project_vfs lock poisoned");
            return;
        }
    };

    log::info!(
        "Build Game: starting for project '{}' (host target: {:?})",
        project_name,
        build_game::BuildTarget::host()
    );
    match build_game::build_for_host(&pvfs, &project_name) {
        Ok(out) => {
            log::info!(
                "Build Game: success via {} — {} ({} assets, {} bytes packed)",
                out.strategy.label(),
                out.output_dir.display(),
                out.asset_count,
                out.pack_bytes
            );
        }
        Err(e) => {
            log::error!("Build Game: failed — {:#}", e);
        }
    }
}

/// Bridge: open a folder picker and rebuild the project VFS at the new
/// location. Used when the user invokes "File > Open Project…" while
/// another project is already loaded.
pub fn browse_and_open_project(
    editor_state: &Arc<Mutex<EditorState>>,
) -> Option<Arc<Mutex<ProjectVfs>>> {
    let path = rfd::FileDialog::new().pick_folder()?;

    let metrics = std::sync::Arc::new(khora_sdk::MetricsRegistry::new());
    match ProjectVfs::open(path.clone(), metrics) {
        Ok(pvfs) => {
            let entries = hot_reload::collect_asset_entries(&pvfs);
            let dirs = pvfs.list_dirs();
            if let Ok(mut state) = editor_state.lock() {
                state.project_folder = Some(path.to_string_lossy().to_string());
                state.asset_entries = entries;
                state.asset_dirs = dirs;
                state.asset_epoch = state.asset_epoch.wrapping_add(1);
                log::info!(
                    "Asset browser: scanned '{}' - {} assets found",
                    path.display(),
                    state.asset_entries.len()
                );
            }
            Some(Arc::new(Mutex::new(pvfs)))
        }
        Err(e) => {
            log::error!(
                "Failed to open ProjectVfs for '{}': {:#}",
                path.display(),
                e
            );
            None
        }
    }
}
