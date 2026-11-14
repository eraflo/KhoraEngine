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
use crate::{build_game, scene_io};

/// Saves the scene, through the project VFS when the path lies under `<project>/assets/`.
///
/// A Save-As elsewhere writes with `std::fs`. Saves under the
/// `EditorInterchange` goal. Returns whether the scene reached disk;
/// failures are logged.
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

/// Saves as [`save_scene_dispatch`] does, under an explicit serialization goal.
///
/// `HumanReadableDebug` for a JSON scene, `PortableBinary` for MessagePack.
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

/// Opens the scene at `abs` off the frame, replacing the open one; returns the load's ticket.
///
/// The load lands at a later frame's boundary, where [`land_scene_loads`]
/// finishes it. A scene outside the project is read from its path, without
/// prefabs. `None` when nothing was filed.
pub fn open_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    loads: &khora_sdk::SceneLoadHub,
    abs: &std::path::Path,
) -> Option<khora_sdk::khora_core::scene::LoadTicket> {
    let mut in_project = None;
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            let assets_root = pvfs.assets_root.clone();
            if let Some(rel_fwd) = scene_io::rel_inside_project(abs, &assets_root) {
                // A scene of the project it names nothing in opens nothing.
                let choice = editor_state.lock().map_or(0, |state| state.scene_choices);
                in_project = Some(scene_io::scene_in_project(&mut pvfs, &rel_fwd, choice)?);
            }
        }
    }
    let (source, prefabs) = in_project.unwrap_or_else(|| scene_io::scene_on_disk(abs));
    let ticket = loads.file(
        world.inner_world_mut(),
        khora_sdk::khora_data::scene::loading::LoadKind::Replace(source),
        prefabs,
    );
    if let Ok(mut state) = editor_state.lock() {
        state
            .opening
            .push((ticket, abs.to_string_lossy().into_owned()));
        // An open waiting for the index, chosen before this one, opens
        // nothing over it.
        state.scene_choices = state.scene_choices.wrapping_add(1);
    }
    Some(ticket)
}

/// Finishes the scene loads the editor filed (an opened scene, Stop's restore) that landed.
///
/// Clears the editor's entity references, maps the load's problems onto the
/// entities it brought, and sets an opened scene's path. A failed load
/// changes nothing and is reported. Returns whether one replaced the world.
pub fn land_scene_loads(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    loads: &khora_sdk::SceneLoadHub,
) -> bool {
    let Ok(mut state) = editor_state.lock() else {
        return false;
    };
    let mut replaced = false;
    if let Some(ticket) = state.restoring {
        if let Some(outcome) = loads.take_outcome(ticket) {
            state.restoring = None;
            let remembered = std::mem::take(&mut state.restore_problems);
            match outcome {
                Ok(report) => {
                    scene_io::log_report("play-mode restore", &report);
                    // The last load's problems, onto the entities the
                    // restore brought under the identities they named.
                    let remapped: Vec<_> = remembered
                        .into_iter()
                        .map(|(id, mut problem)| {
                            if problem.entity.is_some() {
                                problem.entity =
                                    id.and_then(|id| world.inner_world().entity_with_id(id));
                            }
                            problem
                        })
                        .collect();
                    state.load_problems = remapped.clone();
                    state.pending_load_problems = Some(remapped);
                    // Every entity was respawned: what named one names
                    // nothing now, or another once a slot is reused.
                    state.clear_entity_references();
                    replaced = true;
                    log::info!("Play mode: stopped - scene restored");
                }
                Err(error) => {
                    log::error!("Play-mode restore failed: {error}");
                    // The world is still as play left it: its entities are
                    // those the remembered problems name.
                    let mut problems: Vec<_> =
                        remembered.into_iter().map(|(_, problem)| problem).collect();
                    problems.push(load_failed(format!(
                        "the scene as Play found it could not be restored, the world is as play left it: {error}"
                    )));
                    state.load_problems = problems.clone();
                    state.pending_load_problems = Some(problems);
                }
            }
        }
    }
    let opening = std::mem::take(&mut state.opening);
    for (ticket, path) in opening {
        let Some(outcome) = loads.take_outcome(ticket) else {
            state.opening.push((ticket, path));
            continue;
        };
        match outcome {
            Ok(report) => {
                scene_io::log_report(&path, &report);
                state.clear_entity_references();
                // What the load adapted goes to the Problems tab, named on
                // the entities it brought.
                let load = scene_io::load_problems(world.inner_world(), &report);
                state.load_problems = load.clone();
                state.pending_load_problems = Some(load);
                state.current_scene_path = Some(path);
                replaced = true;
            }
            Err(error) => {
                log::error!("Failed to open scene '{path}': {error}");
                // The open scene stays, with its problems; the failure joins
                // them.
                let mut problems = state.load_problems.clone();
                problems.push(load_failed(format!(
                    "the scene '{path}' could not be opened: {error}"
                )));
                state.load_problems = problems.clone();
                state.pending_load_problems = Some(problems);
            }
        }
    }
    replaced
}

/// A load that changed nothing, as a problem of the `Load` source.
fn load_failed(message: String) -> khora_sdk::khora_core::problems::Problem {
    khora_sdk::khora_core::problems::Problem {
        severity: khora_sdk::khora_core::problems::ProblemSeverity::Error,
        module: None,
        line: 0,
        column: 0,
        message,
        note: None,
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

/// Whether Build Game is refused in a project at `trust`: a restricted
/// project's build would run its code.
///
/// Says why in `EditorState::refusal` when refused; a trusted project leaves it as it was.
pub fn build_game_refused(
    trust: khora_sdk::trust::ProjectTrust,
    editor_state: &Arc<Mutex<EditorState>>,
) -> bool {
    if trust == khora_sdk::trust::ProjectTrust::Trusted {
        return false;
    }
    log::warn!("Build Game is refused: the project is restricted");
    if let Ok(mut state) = editor_state.lock() {
        state.refusal = Some(BUILD_REFUSED.to_owned());
    }
    true
}

/// Why Build Game is refused in a restricted project.
const BUILD_REFUSED: &str = "Build Game is off in restricted mode: building runs this project's                              code. Trust the project to build it.";

/// Builds the game for the host under `<project>/dist/<target>/`.
///
/// It writes the `runtime.json` from which the staged binary loads the
/// project's default scene at startup.
///
/// Progress and the final path go to the log. See the book:
/// reference/editor.md, "09 — Build Game".
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

/// Returns the folder "File > Open Project…" picks, if any.
///
/// The app keeps it as `picked_project` and opens it before the next frame, at
/// what the user decided of it.
pub fn pick_project_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().pick_folder()
}
