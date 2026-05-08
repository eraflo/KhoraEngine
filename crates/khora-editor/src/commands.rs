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

//! Editor command dispatch — file I/O, build-game, menu actions.
//!
//! Free functions invoked from `EditorApp::update`. They take only the
//! state they actually need so each one is independently testable and
//! easy to refactor as commands grow.

use std::sync::{Arc, Mutex};

use khora_sdk::prelude::ecs::*;
use khora_sdk::{
    instantiate_subtree, serialize_subtree, CommandHistory, EditorState, GameWorld, PlayMode,
    SerializationGoal,
};

use crate::build_game;
use crate::hot_reload;
use crate::ops;
use crate::project_vfs::ProjectVfs;
use crate::scene_io;

/// Save dispatch: routes through the project VFS when the target path
/// lives under `<project>/assets/`, falls back to direct `std::fs` for
/// arbitrary out-of-project Save-As destinations. Defaults to the
/// `EditorInterchange` strategy.
pub fn save_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    path_str: &str,
) {
    save_scene_dispatch_with_goal(
        project_vfs,
        world,
        path_str,
        SerializationGoal::EditorInterchange,
    );
}

/// Same as [`save_scene_dispatch`] but with an explicit serialization
/// goal. Used by "Export Scene as RON" (`HumanReadableDebug`) and any
/// future Save-As strategy picker.
pub fn save_scene_dispatch_with_goal(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    path_str: &str,
    goal: SerializationGoal,
) {
    let abs = std::path::Path::new(path_str);
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            let assets_root = pvfs.assets_root.clone();
            if let Some(rel_fwd) = scene_io::rel_inside_project(abs, &assets_root) {
                let _ = scene_io::save_scene_in_project_with_goal(
                    &mut pvfs,
                    world,
                    std::path::Path::new(&rel_fwd),
                    goal,
                );
                return;
            }
        }
    }
    scene_io::save_scene_to_path_with_goal(world, path_str, goal);
}

/// Load dispatch: same shape as `save_scene_dispatch`.
pub fn load_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    abs: &std::path::Path,
) {
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
            if let Ok(mut state) = editor_state.lock() {
                state.project_folder = Some(path.to_string_lossy().to_string());
                state.asset_entries = entries;
                log::info!(
                    "Asset browser: scanned '{}' - {} assets found",
                    path.display(),
                    state.asset_entries.len()
                );
            }
            Some(Arc::new(Mutex::new(pvfs)))
        }
        Err(e) => {
            log::error!("Failed to open ProjectVfs for '{}': {:#}", path.display(), e);
            None
        }
    }
}

/// Process every pending menu action queued in `EditorState`. Drains
/// `pending_browse_project_folder` and `pending_menu_action` in turn.
pub fn process_menu_actions(
    project_vfs: &mut Option<Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
    command_history: &Arc<Mutex<CommandHistory>>,
    world: &mut GameWorld,
) {
    let wants_browse = editor_state
        .lock()
        .ok()
        .map(|mut s| std::mem::replace(&mut s.pending_browse_project_folder, false))
        .unwrap_or(false);

    if wants_browse {
        if let Some(new_pvfs) = browse_and_open_project(editor_state) {
            *project_vfs = Some(new_pvfs);
        }
    }

    let action = editor_state
        .lock()
        .ok()
        .and_then(|mut s| s.pending_menu_action.take());

    let Some(action) = action else { return };

    match action.as_str() {
        "new_scene" => apply_new_scene(world, editor_state),
        "undo" => apply_undo(editor_state, command_history),
        "redo" => apply_redo(editor_state, command_history),
        "delete" => apply_delete(world, editor_state),
        "quit" => {
            log::info!("Quit requested from menu");
            std::process::exit(0);
        }
        "play" => apply_play(world, editor_state),
        "pause" => apply_pause(editor_state),
        "stop" => apply_stop(world, editor_state),
        "save" => apply_save(project_vfs.as_ref(), world, editor_state),
        // Save-As does NOT expose a strategy picker. The engine knows
        // which `SerializationGoal` is right for each context — for
        // editor saves that's `EditorInterchange` (Recipe / bincode).
        // Code paths that need a different goal (build pipeline, RON
        // export, etc.) call `save_scene_dispatch_with_goal` directly.
        "save_as" => apply_save_as(project_vfs.as_ref(), world, editor_state),
        "open" => apply_open(project_vfs.as_ref(), world, editor_state),
        "spawn_empty" => {
            if let Ok(mut state) = editor_state.lock() {
                state.pending_spawn = Some("Empty".to_owned());
            }
        }
        "build_game" => run_build_game(project_vfs.as_ref(), editor_state),
        "documentation" => {
            let _ = open::that("https://github.com/eraflo/KhoraEngine");
            log::info!("Opening documentation in browser");
        }
        "about" => {
            log::info!("Khora Engine v0.1.0-dev - experimental game engine");
        }
        "preferences" | "reset_layout" => {
            log::info!("Menu action '{}' (not yet implemented)", action);
        }
        other => {
            log::info!("Unhandled menu action: {}", other);
        }
    }
}

fn apply_new_scene(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        let all: Vec<EntityId> = world.iter_entities().collect();
        for entity in &all {
            world.despawn(*entity);
        }
        state.clear_selection();
        state.inspected = None;
        state.scene_roots.clear();
        state.entity_count = 0;
        state.play_mode = PlayMode::Editing;
        state.scene_snapshot = None;
        log::info!("New scene created (cleared {} entities)", all.len());
    }
}

fn apply_undo(editor_state: &Arc<Mutex<EditorState>>, command_history: &Arc<Mutex<CommandHistory>>) {
    if let Ok(mut history) = command_history.lock() {
        if let Some(edit) = history.undo() {
            if let Ok(mut state) = editor_state.lock() {
                state.push_edit(edit);
            }
        }
    }
}

fn apply_redo(editor_state: &Arc<Mutex<EditorState>>, command_history: &Arc<Mutex<CommandHistory>>) {
    if let Ok(mut history) = command_history.lock() {
        if let Some(edit) = history.redo() {
            if let Ok(mut state) = editor_state.lock() {
                state.push_edit(edit);
            }
        }
    }
}

fn apply_delete(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        ops::delete_selection(world, &mut state);
    }
}

fn apply_play(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        match state.play_mode {
            PlayMode::Editing => {
                state.scene_snapshot = Some(scene_io::snapshot_scene(world));
                state.play_mode = PlayMode::Playing;
                log::info!("Play mode: started");
            }
            PlayMode::Paused => {
                state.play_mode = PlayMode::Playing;
                log::info!("Play mode: resumed");
            }
            _ => {}
        }
    }
}

fn apply_pause(editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        if state.play_mode == PlayMode::Playing {
            state.play_mode = PlayMode::Paused;
            log::info!("Play mode: paused");
        }
    }
}

fn apply_stop(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        if state.play_mode == PlayMode::Playing || state.play_mode == PlayMode::Paused {
            if let Some(snapshot) = state.scene_snapshot.take() {
                drop(state);
                scene_io::restore_scene(world, &snapshot);
                if let Ok(mut state) = editor_state.lock() {
                    state.play_mode = PlayMode::Editing;
                }
            } else {
                state.play_mode = PlayMode::Editing;
            }
            log::info!("Play mode: stopped - scene restored");
        }
    }
}

fn apply_save(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let path = editor_state
        .lock()
        .ok()
        .and_then(|s| s.current_scene_path.clone());
    if let Some(path) = path {
        save_scene_dispatch(project_vfs, world, &path);
    } else if let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .save_file()
    {
        let path = path.to_string_lossy().to_string();
        save_scene_dispatch(project_vfs, world, &path);
        if let Ok(mut state) = editor_state.lock() {
            state.current_scene_path = Some(path);
        }
    }
}

fn apply_save_as(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .save_file()
    {
        let path = path.to_string_lossy().to_string();
        save_scene_dispatch(project_vfs, world, &path);
        if let Ok(mut state) = editor_state.lock() {
            state.current_scene_path = Some(path);
        }
    }
}

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
    let rel = match editor_state.lock() {
        Ok(mut s) => s.pending_prefab_spawn.take(),
        Err(_) => None,
    };
    let Some(rel) = rel else {
        return;
    };

    let Some(pvfs_arc) = project_vfs else {
        log::warn!(
            "Prefab spawn requested ('{}') but no project is open",
            rel
        );
        return;
    };

    let bytes = {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Project VFS mutex poisoned");
            return;
        };
        let uuid = ProjectVfs::uuid_for_rel_path(&rel);
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
            log::info!(
                "Prefab '{}' instantiated (root entity index={})",
                rel,
                new_root.index
            );
        }
        Err(e) => log::error!("Failed to instantiate prefab '{}': {:?}", rel, e),
    }
}

fn apply_open(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .pick_file()
    {
        let path_str = path.to_string_lossy().to_string();
        load_scene_dispatch(project_vfs, world, &path);
        if let Ok(mut state) = editor_state.lock() {
            state.current_scene_path = Some(path_str);
            state.clear_selection();
            state.inspected = None;
        }
    }
}
