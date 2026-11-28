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

//! Editor command dispatch: file I/O, Build Game, menu actions.
//!
//! Free functions called from `EditorApp::update`, each taking only the state it needs.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorRequest, MenuAction, SceneInProject, SpawnShape};
use khora_sdk::prelude::ecs::*;
use khora_sdk::{EditorState, GameWorld, PlayMode};

use crate::project_vfs::ProjectVfs;
use crate::{ops, scene_io};

mod asset_ops;
pub(crate) mod history;
mod material;
mod prefab;
mod saved_in_project;
mod scene;
mod transport;

pub use asset_ops::assign_queued_drops;
// For the asset browser's tests: the app follows key changes through the file operations.
#[cfg(test)]
pub(crate) use asset_ops::follow_key_changes;
pub use asset_ops::run_queued_asset_file_ops;
pub use asset_ops::spawn_queued_meshes;
pub use history::CommandHistory;
pub use material::apply_after_index;
pub use material::assign_queued_materials;
pub use material::save_queued_materials;
pub use prefab::apply_queued_instances;
pub use prefab::save_queued_prefabs;
pub use prefab::spawn_queued_prefabs;
pub use scene::build_game_refused;
pub(crate) use scene::pick_project_folder;
pub(crate) use scene::run_build_game;
// The bool form, for tests: the editor saves through the reporting one.
#[cfg(test)]
pub use scene::save_scene_dispatch;
pub use scene::{land_scene_loads, open_scene_dispatch};
#[cfg(test)]
pub use transport::PLAY_FAMILIES;
pub use transport::{process_transport, start_editing};

/// Processes the menu actions queued in `EditorState`, oldest first.
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
    // The transport is processed before the frame (`process_transport`): an
    // action of its asked since stays queued for the next frame's.
    let actions = editor_state
        .lock()
        .map(|mut s| {
            s.take_requests(|request| match request {
                EditorRequest::Menu(action) => Ok(action),
                other => Err(other),
            })
        })
        .unwrap_or_default();

    let mut picked = None;
    for action in actions {
        let asked = run_menu_action(
            action,
            project_vfs,
            editor_state,
            command_history,
            world,
            trust,
        );
        picked = asked.or(picked);
    }
    picked
}

/// Carries out one menu `action`; returns the project folder it picked, if any.
fn run_menu_action(
    action: MenuAction,
    project_vfs: &mut Option<Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
    command_history: &Arc<Mutex<CommandHistory>>,
    world: &mut GameWorld,
    trust: khora_sdk::trust::ProjectTrust,
) -> Option<std::path::PathBuf> {
    match action {
        MenuAction::OpenProject => return pick_project_folder(),
        MenuAction::NewScene => {
            apply_new_scene(world, editor_state);
            forget_history(command_history);
        }
        MenuAction::Delete => apply_delete(world, editor_state),
        MenuAction::Quit => {
            log::info!("Quit requested from menu");
            std::process::exit(0);
        }
        MenuAction::Save => apply_save(project_vfs.as_ref(), world, editor_state),
        // No encoding picker: an editor save is `EditorInterchange`.
        MenuAction::SaveAs => apply_save_as(project_vfs.as_ref(), world, editor_state),
        MenuAction::Open => {
            apply_open(editor_state);
            forget_history(command_history);
        }
        MenuAction::SpawnEmpty => {
            if let Ok(mut state) = editor_state.lock() {
                state.push_request(EditorRequest::Spawn(SpawnShape::Empty));
            }
        }
        MenuAction::BuildGame => {
            if !build_game_refused(trust, editor_state) {
                run_build_game(project_vfs.as_ref(), editor_state);
            }
        }
        MenuAction::Documentation => {
            let _ = open::that("https://github.com/eraflo/KhoraEngine");
            log::info!("Opening documentation in browser");
        }
        MenuAction::About => {
            log::info!("Khora Engine v0.1.0-dev - experimental game engine");
        }
    }
    None
}

/// Empties the undo history after the world was replaced.
///
/// Every edit it holds names entities by id, and those ids now name nothing,
/// or, once a slot is recycled, someone else.
pub(crate) fn forget_history(command_history: &Arc<Mutex<CommandHistory>>) {
    if let Ok(mut history) = command_history.lock() {
        history.clear();
    }
}

/// Empties `world` of every entity and `state` of the scene it held; returns how many entities went.
///
/// What names an entity goes with them, the play under way ends (its
/// snapshot dropped), every scene still opening or being restored is
/// withdrawn through the world's `SceneLoadHub` (it lands nothing), what the
/// last load adapted is forgotten and asked cleared from the runtime's
/// `Problems` ([`EditorRequest::ReportLoadProblems`]), and so is the scene's
/// file: a save then asks where. Moves `scene_choices`, so an open waiting
/// for the index opens nothing over the empty world.
pub(crate) fn clear_scene(world: &mut GameWorld, state: &mut EditorState) -> usize {
    let all: Vec<EntityId> = world.iter_entities().collect();
    for entity in &all {
        world.despawn(*entity);
    }
    state.clear_entity_references();
    state.play_mode = PlayMode::Editing;
    state.scene_snapshot = None;
    state.played_choice = None;
    state.scene_choices = state.scene_choices.wrapping_add(1);
    // The empty world has the last word over a scene still opening or being
    // restored: withdrawn now, before a boundary lands it.
    let opening: Vec<_> = state
        .opening
        .drain(..)
        .map(|opening| opening.ticket)
        .collect();
    let withdrawn: Vec<_> = opening.into_iter().chain(state.restoring.take()).collect();
    match world.scene_loads() {
        Some(loads) => withdrawn
            .into_iter()
            .for_each(|ticket| loads.cancel(ticket)),
        None if !withdrawn.is_empty() => {
            log::error!("the scene loads in flight cannot be withdrawn");
        }
        None => {}
    }
    state.restore_problems.clear();
    // What the last load reported described the scene that went.
    state.load_problems.clear();
    state.push_request(EditorRequest::ReportLoadProblems {
        problems: Vec::new(),
    });
    state.current_scene_path = None;
    all.len()
}

/// Starts a new scene: the world emptied, with its `Load` problems ([`clear_scene`]).
fn apply_new_scene(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        let cleared = clear_scene(world, &mut state);
        log::info!("New scene created (cleared {cleared} entities)");
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

/// Plays the scene, or resumes it when paused; refused, logged, while a scene is restored or opened.
///
/// Until a restore or an opened scene lands, the world is not the scene the
/// editor holds: a snapshot of it would be restored at Stop as the wrong
/// scene. A play started snapshots the world and records the scene choice it
/// played (`EditorState::played_choice`).
fn apply_play(world: &mut GameWorld, editor_state: &Arc<Mutex<EditorState>>) {
    if let Ok(mut state) = editor_state.lock() {
        if state.restoring.is_some() {
            log::info!("Play mode: refused while the scene is being restored");
            return;
        }
        if !state.opening.is_empty() {
            log::info!(
                "Play mode: refused while a scene is opening: the world is not the scene it opens yet"
            );
            return;
        }
        match state.play_mode {
            PlayMode::Editing => {
                state.scene_snapshot = Some(scene_io::snapshot_scene(world));
                state.played_choice = Some(state.scene_choices);
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

/// Stops the play: Editing at once, the scene as Play found it filed with `loads`, as [`stop_play`] does.
fn apply_stop(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    loads: &khora_sdk::SceneLoadHub,
) {
    if let Ok(mut state) = editor_state.lock() {
        stop_play(world, &mut state, loads);
    }
}

/// Stops the play `state` holds: Editing at once, the scene as Play found it filed with `loads`.
///
/// Nothing is done unless playing or paused. The restore lands at a later
/// frame's boundary, where [`land_scene_loads`] finishes it; Play is refused
/// until it does.
pub(crate) fn stop_play(
    world: &mut GameWorld,
    state: &mut EditorState,
    loads: &khora_sdk::SceneLoadHub,
) {
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

/// Saves the scene to its file in the project open, or, with none, to the file a dialog picks, as Save As does.
fn apply_save(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let current = editor_state
        .lock()
        .ok()
        .and_then(|s| s.current_scene_path.clone());
    match current {
        Some(scene) => {
            save_and_report(project_vfs, world, editor_state, &scene);
        }
        None => apply_save_as(project_vfs, world, editor_state),
    }
}

/// Saves the scene to the file a dialog picks under the open project's `assets/`, as [`save_scene_as`] does.
///
/// With no project open, nothing is asked nor saved, and the log says why.
fn apply_save_as(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let Some(assets) = saved_in_project::assets_folder(project_vfs) else {
        log::error!(
            "The scene is not saved: {}",
            saved_in_project::no_project(khora_sdk::khora_core::asset::kinds::SCENE.name())
        );
        return;
    };
    if let Some(path) = pick_scene_to_save(&assets) {
        save_scene_as(project_vfs, world, editor_state, &path);
    }
}

/// Returns the scene file the user picks to save to, the dialog starting in the project's assets folder `assets`; `None` when cancelled.
fn pick_scene_to_save(assets: &std::path::Path) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .set_directory(assets)
        .save_file()
}

/// Saves the scene to the file at `picked`, which becomes the scene a save writes to once the save landed; returns whether it did.
///
/// The file must be a `.kscene` under the open project's `assets/`
/// ([`scene_at`]), so the scene's path is only ever a scene file: one
/// outside, one of another kind or of none, or any with no project open, is
/// refused and nothing is written. A name with no extension is saved as the
/// `.kscene` of that name.
/// A save refused or failed changes nothing of the editor's state: the scene
/// keeps the file it had, and the save problems stay as they were. See
/// [`save_and_report`].
///
/// [`scene_at`]: scene::scene_at
fn save_scene_as(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    picked: &std::path::Path,
) -> bool {
    let Some(scene) = scene::scene_at(project_vfs, picked) else {
        return false;
    };
    let landed = save_and_report(project_vfs, world, editor_state, &scene);
    if landed {
        if let Ok(mut state) = editor_state.lock() {
            state.current_scene_path = Some(scene);
        }
    }
    landed
}

/// Saves the scene to `scene` and asks to report what the save left out, as
/// the `Save` problems; returns whether the scene reached disk.
///
/// A save that did not reach disk reports nothing.
fn save_and_report(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    scene: &SceneInProject,
) -> bool {
    let saved = scene::save_scene_reporting(
        project_vfs,
        world,
        scene,
        khora_sdk::SerializationGoal::EditorInterchange,
    );
    let landed = saved.is_some();
    if let (Some(left_out), Ok(mut state)) = (saved, editor_state.lock()) {
        scene::report_save(&mut state, world, &left_out);
    }
    landed
}

fn apply_open(editor_state: &Arc<Mutex<EditorState>>) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Scene", &["kscene"])
        .pick_file()
    {
        // Opened as a scene the asset browser opens: off the frame, the
        // next update filing it.
        if let Ok(mut state) = editor_state.lock() {
            state.push_request(EditorRequest::OpenScene {
                path: path.to_string_lossy().into_owned(),
            });
        }
    }
}

#[cfg(test)]
mod tests;
