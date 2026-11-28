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

use khora_sdk::editor_ui::{EditorRequest, SceneInProject, SceneOpening};
use khora_sdk::khora_core::asset::kinds;
use khora_sdk::khora_data::scene::record::ReportEntry;
use khora_sdk::{EditorState, GameWorld, SerializationGoal};

use crate::project_vfs::{InProject, ProjectVfs};
use crate::{build_game, scene_io};

/// Why Build Game is refused in a restricted project.
const BUILD_REFUSED: &str = "Build Game is off in restricted mode: building runs this project's \
                             code. Trust the project to build it.";

// The bool form, for tests: the editor saves through the reporting one.
#[cfg(test)]
/// Saves the scene to the file at `path_str` under the open project's `assets/`, as a Save As does, under the `EditorInterchange` goal.
///
/// Returns whether the scene reached disk; a file outside the project's
/// `assets/`, or any with no project open, is refused ([`scene_at`]).
/// Failures are logged.
pub fn save_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    path_str: &str,
) -> bool {
    scene_at(project_vfs, std::path::Path::new(path_str)).is_some_and(|scene| {
        save_scene_reporting(
            project_vfs,
            world,
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .is_some()
    })
}

/// Names the file at `picked` as a scene of the project `project_vfs` opens; `None`, logged, when it is none.
///
/// Refused as a prefab's Save As is ([`saved_in_project`]): no project open,
/// a file outside the project's `assets/`, a path under the assets the
/// project refuses, or a file that is no `.kscene` by its extension, in any
/// case (a texture's `wall.png` is never written over). A name with no
/// extension is the `.kscene` of that name, as a file dialog adds it.
///
/// [`saved_in_project`]: super::saved_in_project::saved_in_project
pub(crate) fn scene_at(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    picked: &std::path::Path,
) -> Option<SceneInProject> {
    super::saved_in_project::saved_in_project(project_vfs, picked, kinds::SCENE)
        .map(|(assets, key)| SceneInProject::new(assets, key))
}

/// Saves the scene under `goal` to `scene`, through the project open, and returns what the save left out of the file.
///
/// `None`, logged, when the scene did not reach disk: no project open, a
/// scene of another project (it is refused, nothing written), or a failure.
pub(crate) fn save_scene_reporting(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    scene: &SceneInProject,
    goal: SerializationGoal,
) -> Option<Vec<ReportEntry>> {
    let Some(mut pvfs) = project_vfs.and_then(|pvfs| pvfs.lock().ok()) else {
        log::error!(
            "The scene is not saved: {}",
            super::saved_in_project::no_project(kinds::SCENE.name())
        );
        return None;
    };
    if !scene.is_in(&pvfs.assets_root) {
        log::error!(
            "The scene is not saved: '{}' belongs to another project than the one open",
            scene.path().display()
        );
        return None;
    }
    // The save logs its own failures.
    scene_io::save_scene_in_project_reporting(
        &mut pvfs,
        world,
        std::path::Path::new(scene.key()),
        goal,
    )
}

/// Asks to report what a save of a scene or a prefab left out, as the `Save` problems.
///
/// Replaces what the last save reported; a save that left nothing out clears it.
pub(crate) fn report_save(state: &mut EditorState, world: &GameWorld, left_out: &[ReportEntry]) {
    let problems = scene_io::report_problems(world.inner_world(), left_out);
    state.push_request(EditorRequest::ReportSaveProblems { problems });
}

/// Opens the scene at `abs` off the frame, replacing the open one; returns the load's ticket.
///
/// The load lands at a later frame's boundary, where [`land_scene_loads`]
/// finishes it; a scene of the project becomes the scene a save writes to. A
/// scene outside the project's `assets/` is read from its path
/// through a guard, without prefabs: the project root's, at the project's
/// trust; any other folder's as a downloaded file, reporting to `reads`'
/// refusals, the run's. A path under the assets the project refuses
/// ([`InProject::Refused`]) opens nothing, and says why. `None` when nothing
/// was filed.
pub fn open_scene_dispatch(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    loads: &khora_sdk::SceneLoadHub,
    reads: &crate::project_vfs::ProjectReads,
    abs: &std::path::Path,
) -> Option<khora_sdk::khora_core::scene::LoadTicket> {
    let mut opened = None;
    let mut scene = None;
    if let Some(pvfs_arc) = project_vfs {
        if let Ok(mut pvfs) = pvfs_arc.lock() {
            opened = Some(match pvfs.in_project(abs) {
                InProject::Inside(rel_fwd) => {
                    // A scene of the project it names nothing in opens nothing.
                    let choice = editor_state.lock().map_or(0, |state| state.scene_choices);
                    let found = scene_io::scene_in_project(&mut pvfs, &rel_fwd, choice)?;
                    scene = Some(SceneInProject::new(pvfs.assets_root.clone(), rel_fwd));
                    found
                }
                InProject::Refused(why) => {
                    log::error!("The scene is not opened: {why}");
                    return None;
                }
                InProject::Outside => scene_io::scene_on_disk(abs, pvfs.reads(), Some(&pvfs.root)),
            });
        }
    }
    let (source, prefabs) = opened.unwrap_or_else(|| scene_io::scene_on_disk(abs, reads, None));
    let ticket = loads.file(
        world.inner_world_mut(),
        khora_sdk::khora_data::scene::loading::LoadKind::Replace(source),
        prefabs,
    );
    if let Ok(mut state) = editor_state.lock() {
        state.opening.push(SceneOpening {
            ticket,
            path: abs.to_string_lossy().into_owned(),
            scene,
        });
        // An open waiting for the index, chosen before this one, opens
        // nothing over it.
        state.scene_choices = state.scene_choices.wrapping_add(1);
    }
    Some(ticket)
}

/// Finishes the scene loads the editor filed (an opened scene, Stop's restore) that landed.
///
/// Clears the editor's entity references, maps the load's problems onto the
/// entities it brought, and makes an opened scene's file the one a save writes
/// to (none for a file of no project). A failed load
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
            state.played_choice = None;
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
                    state.push_request(EditorRequest::ReportLoadProblems { problems: remapped });
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
                    state.push_request(EditorRequest::ReportLoadProblems { problems });
                }
            }
        }
    }
    let opening = std::mem::take(&mut state.opening);
    for opened in opening {
        let Some(outcome) = loads.take_outcome(opened.ticket) else {
            state.opening.push(opened);
            continue;
        };
        let SceneOpening { path, scene, .. } = opened;
        match outcome {
            Ok(report) => {
                scene_io::log_report(&path, &report);
                state.clear_entity_references();
                // What the load adapted goes to the Problems tab, named on
                // the entities it brought.
                let load = scene_io::load_problems(world.inner_world(), &report);
                state.load_problems = load.clone();
                state.push_request(EditorRequest::ReportLoadProblems { problems: load });
                // A file of no project has no file a save writes to.
                state.current_scene_path = scene;
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
                state.push_request(EditorRequest::ReportLoadProblems { problems });
            }
        }
    }
    replaced
}

/// Reports a load that changed nothing as a problem of the `Load` source.
fn load_failed(message: String) -> khora_sdk::khora_core::problems::Problem {
    khora_sdk::khora_core::problems::Problem::new(
        khora_sdk::khora_core::problems::ProblemSeverity::Error,
        message,
    )
}

/// Returns whether Build Game is refused in a project at `trust`: a restricted
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

/// Builds the game for the host under `<project>/dist/<target>/`.
///
/// It writes the `runtime.json` from which the staged binary loads the
/// project's default scene at startup. A failed build leaves the previous one
/// as it was.
///
/// Progress and the result line go to the log: built, with the files the pack
/// left out named, or why it failed; the refusals reach the run's problems.
/// See the book: reference/editor.md, "09 — Build Game".
pub(crate) fn run_build_game(
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
    let result = build_game::build_for_host(&pvfs, &project_name);
    let line = build_game::result_line(&result);
    match &result {
        Ok(out) if out.left_out.is_empty() => log::info!("{line}"),
        Ok(_) => log::warn!("{line}"),
        Err(_) => log::error!("{line}"),
    }
}

/// Returns the folder "File > Open Project…" picks, if any.
///
/// The app keeps it as `picked_project` and opens it before the next frame, at
/// what the user decided of it.
pub(crate) fn pick_project_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().pick_folder()
}
