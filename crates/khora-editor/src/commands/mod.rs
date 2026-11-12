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
mod transport;

pub use asset_ops::process_pending_asset_file_ops;
pub use asset_ops::process_pending_assign_texture;
pub use asset_ops::process_pending_spawn_mesh_asset;
pub use history::CommandHistory;
pub use material::apply_after_index;
pub use material::process_pending_assign_material;
pub use material::process_pending_save_as_material;
pub use prefab::process_pending_prefab_apply;
pub use prefab::process_pending_prefab_spawn;
pub use prefab::process_pending_save_as_prefab;
pub use scene::build_game_refused;
pub use scene::pick_project_folder;
pub use scene::run_build_game;
pub use scene::save_scene_dispatch;
pub use scene::{land_scene_loads, open_scene_dispatch};
#[cfg(test)]
pub use transport::PLAY_FAMILIES;
pub use transport::{process_transport, start_editing};

/// Process every pending menu action queued in `EditorState`. Drains
/// `pending_browse_project_folder` and `pending_menu_action` in turn.
///
/// Returns the folder "File > Open Project…" picked, for the app to open;
/// a restricted project (`trust`) builds no game.
pub fn process_menu_actions(
    project_vfs: &mut Option<Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
    command_history: &Arc<Mutex<CommandHistory>>,
    world: &mut GameWorld,
    trust: khora_sdk::trust::ProjectTrust,
) -> Option<std::path::PathBuf> {
    let wants_browse = editor_state
        .lock()
        .ok()
        .map(|mut s| std::mem::replace(&mut s.pending_browse_project_folder, false))
        .unwrap_or(false);

    let picked = if wants_browse {
        pick_project_folder()
    } else {
        None
    };

    // The transport is processed before the frame (`process_transport`): an
    // action of its asked since is left for the next frame's.
    let action = editor_state.lock().ok().and_then(|mut s| {
        if s.pending_menu_action
            .as_deref()
            .is_some_and(transport::is_transport)
        {
            None
        } else {
            s.pending_menu_action.take()
        }
    });

    let Some(action) = action else {
        return picked;
    };

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
        "save" => apply_save(project_vfs.as_ref(), world, editor_state),
        // Save-As does NOT expose an encoding picker. The engine knows
        // which `SerializationGoal` is right for each context — for
        // editor saves that's `EditorInterchange` (the compact encoding).
        // Code paths that need another goal (text for diffing, MessagePack
        // for tools) call `save_scene_dispatch_with_goal` directly.
        "save_as" => apply_save_as(project_vfs.as_ref(), world, editor_state),
        "open" => {
            apply_open(editor_state);
            forget_history(command_history);
        }
        "spawn_empty" => {
            if let Ok(mut state) = editor_state.lock() {
                state.pending_spawn = Some("Empty".to_owned());
            }
        }
        "build_game" => {
            if !build_game_refused(trust, editor_state) {
                run_build_game(project_vfs.as_ref(), editor_state);
            }
        }
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
    picked
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
        // An open waiting for the index opens nothing over the new scene.
        state.scene_choices = state.scene_choices.wrapping_add(1);
        // The new scene has the last word over a scene still opening or
        // being restored: withdrawn now, before a boundary lands it.
        let opening: Vec<_> = state.opening.drain(..).map(|(ticket, _)| ticket).collect();
        let withdrawn: Vec<_> = opening.into_iter().chain(state.restoring.take()).collect();
        match world.scene_loads() {
            Some(loads) => withdrawn
                .into_iter()
                .for_each(|ticket| loads.cancel(ticket)),
            None if !withdrawn.is_empty() => {
                log::error!("New scene: the scene loads in flight cannot be withdrawn");
            }
            None => {}
        }
        state.restore_problems.clear();
        // A new scene has nothing a load adapted.
        state.load_problems.clear();
        state.pending_load_problems = Some(Vec::new());
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
        if state.restoring.is_some() {
            // The world is still the last play's: its restore would undo
            // the delete.
            log::info!("Delete: refused while the scene is being restored");
            return;
        }
        ops::delete_selection(world, &mut state);
    }
}

fn apply_play(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        if state.restoring.is_some() {
            // The world is still the last play's until the restore lands: a
            // snapshot of it would be the wrong scene.
            log::info!("Play mode: refused while the scene is being restored");
            return;
        }
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

/// Stops the play: Editing at once, and the scene as Play found it filed
/// with `loads` to be restored off the frame — it lands at a later frame's
/// boundary, where [`land_scene_loads`] finishes it. Play is refused until
/// it does.
fn apply_stop(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    loads: &khora_sdk::SceneLoadHub,
) {
    let Ok(mut state) = editor_state.lock() else {
        return;
    };
    if state.play_mode != PlayMode::Playing && state.play_mode != PlayMode::Paused {
        return;
    }
    state.play_mode = PlayMode::Editing;
    let Some(snapshot) = state.scene_snapshot.take() else {
        log::info!("Play mode: stopped - no snapshot to restore");
        return;
    };
    // The last load's problems name entities the restore is about to
    // respawn: remember who they are, by persistent identity.
    state.restore_problems = state
        .load_problems
        .iter()
        .map(|problem| {
            let id = problem
                .entity
                .and_then(|entity| world.inner_world().persistent_id(entity));
            (id, problem.clone())
        })
        .collect();
    let ticket = loads.file(
        world.inner_world_mut(),
        khora_sdk::khora_data::scene::loading::LoadKind::Replace(
            khora_sdk::khora_data::scene::loading::SceneSource::Bytes(Arc::from(snapshot)),
        ),
        Arc::new(khora_sdk::khora_data::scene::NoPrefabs),
    );
    state.restoring = Some(ticket);
    log::info!("Play mode: stopped - restoring the scene");
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

fn apply_open(editor_state: &Arc<Mutex<EditorState>>) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .pick_file()
    {
        // Opened as a scene the asset browser opens: off the frame, the
        // next update filing it.
        if let Ok(mut state) = editor_state.lock() {
            state.pending_scene_load = Some(path.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests;
