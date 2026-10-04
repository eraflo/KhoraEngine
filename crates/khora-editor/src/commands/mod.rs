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
use khora_sdk::{EditorState, GameWorld, PlayMode};

use crate::project_vfs::ProjectVfs;
use crate::{ops, scene_io};

mod asset_ops;
pub(crate) mod history;
mod material;
mod prefab;
mod scene;

pub use asset_ops::process_pending_asset_file_ops;
pub use asset_ops::process_pending_assign_texture;
pub use asset_ops::process_pending_spawn_mesh_asset;
pub use history::CommandHistory;
pub use material::process_pending_assign_material;
pub use material::process_pending_save_as_material;
pub use prefab::process_pending_prefab_apply;
pub use prefab::process_pending_prefab_spawn;
pub use prefab::process_pending_save_as_prefab;
pub use scene::browse_and_open_project;
pub use scene::load_scene_dispatch;
pub use scene::run_build_game;
pub use scene::save_scene_dispatch;

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
        "new_scene" => {
            apply_new_scene(world, editor_state);
            forget_history(command_history);
        }
        "undo" => apply_undo(editor_state, command_history),
        "redo" => apply_redo(editor_state, command_history),
        "delete" => apply_delete(world, editor_state),
        "quit" => {
            log::info!("Quit requested from menu");
            std::process::exit(0);
        }
        "play" => apply_play(world, editor_state),
        "pause" => apply_pause(editor_state),
        "stop" => {
            apply_stop(world, editor_state);
            forget_history(command_history);
        }
        "save" => apply_save(project_vfs.as_ref(), world, editor_state),
        // Save-As does NOT expose an encoding picker. The engine knows
        // which `SerializationGoal` is right for each context — for
        // editor saves that's `EditorInterchange` (the compact encoding).
        // Code paths that need another goal (text for diffing, MessagePack
        // for tools) call `save_scene_dispatch_with_goal` directly.
        "save_as" => apply_save_as(project_vfs.as_ref(), world, editor_state),
        "open" => {
            apply_open(project_vfs.as_ref(), world, editor_state);
            forget_history(command_history);
        }
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

/// Empties the undo history after the world was replaced: every edit it holds
/// names entities by id, and those ids now name nothing — or, once a slot is
/// recycled, someone else.
pub(crate) fn forget_history(command_history: &Arc<Mutex<CommandHistory>>) {
    if let Ok(mut history) = command_history.lock() {
        history.clear();
    }
}

fn apply_new_scene(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        let all: Vec<EntityId> = world.iter_entities().collect();
        for entity in &all {
            world.despawn(*entity);
        }
        state.clear_entity_references();
        state.play_mode = PlayMode::Editing;
        state.scene_snapshot = None;
        log::info!("New scene created (cleared {} entities)", all.len());
    }
}

fn apply_undo(
    editor_state: &Arc<Mutex<EditorState>>,
    command_history: &Arc<Mutex<CommandHistory>>,
) {
    if let Ok(mut history) = command_history.lock() {
        if let Some(edit) = history.undo() {
            if let Ok(mut state) = editor_state.lock() {
                state.push_edit(edit);
            }
        }
    }
}

fn apply_redo(
    editor_state: &Arc<Mutex<EditorState>>,
    command_history: &Arc<Mutex<CommandHistory>>,
) {
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
                    // The restore respawns everything with fresh ids, so any
                    // selection made while playing now names entities that no
                    // longer exist — or, once a slot is recycled, different
                    // ones entirely.
                    state.clear_entity_references();
                    state.play_mode = PlayMode::Editing;
                }
                log::info!("Play mode: stopped - scene restored");
            } else {
                state.play_mode = PlayMode::Editing;
                log::info!("Play mode: stopped - no snapshot to restore");
            }
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
        load_scene_dispatch(project_vfs, world, editor_state, &path);
        if let Ok(mut state) = editor_state.lock() {
            state.current_scene_path = Some(path_str);
        }
    }
}

#[cfg(test)]
mod tests;
