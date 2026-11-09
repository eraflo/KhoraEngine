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

//! Opening the project named on the command line.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::{GameWorld, Runtime};

use crate::project_vfs::ProjectVfs;
use crate::{git, hot_reload, scene_io};

/// The project the editor opens behind its film: the folder, and the `Open`
/// filed for it with the engine's index hub, until it lands.
pub(crate) struct OpeningProject {
    /// The project's folder.
    pub(crate) root: std::path::PathBuf,
    /// The `Open` filed with the engine's index hub.
    pub(crate) ticket: khora_sdk::IndexTicket,
    /// The editor's scene choices when it was filed: a scene the user chose
    /// meanwhile is not replaced by the project's default one.
    pub(crate) choice: u64,
}

/// Starts opening the project at `root` off the frame, as `setup` does: takes
/// a hold on the runtime's `Readiness` into `hold`, creates `<root>/assets/`,
/// files an `Open` with the runtime's `IndexHub` and waits on it, and fills
/// in `editor_state` what needs no index — the folder, the name and engine
/// version `project.json` gives, the git branch. `None`, logged, when it
/// cannot be filed.
pub(crate) fn start_opening_project(
    root: std::path::PathBuf,
    runtime: &Runtime,
    editor_state: &Arc<Mutex<khora_sdk::EditorState>>,
    hold: &mut Option<khora_sdk::ReadinessHold>,
) -> Option<OpeningProject> {
    let readiness = runtime.resources.get::<Arc<khora_sdk::Readiness>>();
    let indexes = runtime.resources.get::<Arc<khora_sdk::IndexHub>>();
    let Some(indexes) = indexes else {
        log::error!("the engine holds no index hub: the project cannot be opened");
        return None;
    };
    // What the project opens with is filed in steps — the index, then its
    // scene and its scripts once it landed: the film waits for all of them.
    *hold = readiness.map(|readiness| readiness.hold());
    let assets = root.join("assets");
    if let Err(e) = std::fs::create_dir_all(&assets) {
        log::warn!("could not create {}: {e}", assets.display());
    }
    let ticket = indexes.file(root.clone(), khora_sdk::IndexKind::Open);
    if let Some(readiness) = readiness {
        readiness.wait_for_index(ticket);
    }

    // What needs no index: `project.json`'s few hundred bytes and the git
    // branch.
    let project_json: Option<serde_json::Value> =
        std::fs::read_to_string(root.join("project.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
    let text = |key: &str| {
        project_json
            .as_ref()
            .and_then(|json| json.get(key).and_then(|v| v.as_str()).map(String::from))
    };
    let mut choice = 0;
    if let Ok(mut state) = editor_state.lock() {
        choice = state.scene_choices;
        state.project_folder = Some(root.to_string_lossy().to_string());
        state.project_name = text("name");
        state.project_engine_version = text("engine_version");
        state.current_git_branch = git::read_git_branch(&root);
    }
    log::info!("Opening project '{}' off the frame", root.display());
    Some(OpeningProject {
        root,
        ticket,
        choice,
    })
}

/// Lands the project `opening` names once its `Open` landed, as
/// `before_frame` does after the scene loads land. `None`, `opening` kept,
/// while it is in flight.
///
/// - **`Ok`:** the project's VFS is built over `shared_assets` (which the
///   `Open` filled), reads its changes from `asset_watch` and files its
///   reindexes with the runtime's `IndexHub`; the browser's entries, folders
///   and epoch are filled; the default scene is opened by ticket when the
///   index holds it, that ticket waited on by the runtime's `Readiness`, or
///   created (spawned, then saved, the save reindexing off the frame).
///   Returns the VFS; `opening` is emptied. `hold` is kept: the scripts are
///   still to be mounted.
/// - **`Err`:** logged and reported to the runtime's `Problems` under
///   `ProblemSource::Load`; `opening` and `hold` are emptied. `None`.
pub(crate) fn land_opened_project(
    opening: &mut Option<OpeningProject>,
    world: &mut GameWorld,
    runtime: &Runtime,
    shared_assets: &Arc<Mutex<khora_sdk::AssetService>>,
    asset_watch: &Arc<khora_sdk::AssetWatch>,
    editor_state: &Arc<Mutex<khora_sdk::EditorState>>,
    hold: &mut Option<khora_sdk::ReadinessHold>,
) -> Option<Arc<Mutex<ProjectVfs>>> {
    let ticket = opening.as_ref()?.ticket;
    let indexes = runtime.resources.get::<Arc<khora_sdk::IndexHub>>()?;
    let outcome = indexes.take_outcome(ticket)?;
    let OpeningProject { root, choice, .. } = opening.take()?;
    let opened = outcome.map_err(|why| why.to_string()).and_then(|()| {
        ProjectVfs::over_service(
            root.clone(),
            Arc::clone(shared_assets),
            Arc::clone(asset_watch),
        )
        .map_err(|e| format!("{e:#}"))
    });
    let mut pvfs = match opened {
        Ok(pvfs) => pvfs,
        Err(why) => {
            log::error!(
                "The project '{}' could not be opened: {why}",
                root.display()
            );
            if let Some(problems) = runtime
                .resources
                .get::<Arc<khora_sdk::khora_core::problems::Problems>>()
            {
                problems.report(
                    khora_sdk::khora_core::problems::ProblemSource::Load,
                    vec![khora_sdk::khora_core::problems::Problem {
                        severity: khora_sdk::khora_core::problems::ProblemSeverity::Error,
                        module: None,
                        line: 0,
                        column: 0,
                        message: format!(
                            "the project '{}' could not be opened: {why}",
                            root.display()
                        ),
                        note: None,
                        entity: None,
                        behavior: None,
                        component: None,
                        field: None,
                    }],
                );
            }
            // The editor runs with no project: nothing of it is shown as
            // open, and nothing more is filed — the film goes.
            if let Ok(mut state) = editor_state.lock() {
                state.project_folder = None;
                state.project_name = None;
                state.project_engine_version = None;
                state.current_git_branch = None;
            }
            *hold = None;
            return None;
        }
    };
    pvfs.index_with(Arc::clone(indexes));

    let entries: Vec<AssetEntry> = hot_reload::collect_asset_entries(&pvfs);
    let dirs = pvfs.list_dirs();
    if let Ok(mut state) = editor_state.lock() {
        log::info!(
            "Opened project '{}' ({} assets)",
            root.display(),
            entries.len()
        );
        state.asset_entries = entries;
        state.asset_dirs = dirs;
        state.asset_epoch = state.asset_epoch.wrapping_add(1);
    }

    // Its default scene: opened by ticket when the index holds it, the film
    // waiting on it; created when there is none.
    // Whether there is one is asked of the disk, not of the index: a scene
    // written after the walk, or spelled with other capitals, is the user's
    // and is never saved over.
    let rel = khora_sdk::DEFAULT_SCENE_REL_PATH;
    let abs = root.join("assets").join(rel);
    let pvfs = Arc::new(Mutex::new(pvfs));
    let chosen_since = editor_state
        .lock()
        .is_ok_and(|state| state.scene_choices != choice);
    if chosen_since {
        // The user chose a scene meanwhile (the film gave up waiting): it
        // stays.
    } else if abs.exists() {
        if let Some(loads) = runtime.resources.get::<Arc<khora_sdk::SceneLoadHub>>() {
            let ticket =
                crate::commands::open_scene_dispatch(Some(&pvfs), world, editor_state, loads, &abs);
            if let (Some(ticket), Some(readiness)) =
                (ticket, runtime.resources.get::<Arc<khora_sdk::Readiness>>())
            {
                readiness.wait_for_scene(ticket);
            }
        }
    } else if let Ok(mut pvfs) = pvfs.lock() {
        scene_io::create_default_scene_in_project(&mut pvfs, world, rel);
    }
    Some(pvfs)
}

/// Brings the mounted scripts in line with `project`, as `before_agents`
/// does ([`follow_open_project`]); the compile a mount files is waited on by
/// the runtime's `Readiness`. Once a project is open — `project` is `Some` —
/// `hold` is let go: what the project opens with is all filed. Returns the
/// compile's ticket, as [`follow_open_project`] does.
///
/// [`follow_open_project`]: crate::project_scripts::follow_open_project
pub(crate) fn mount_opened_scripts(
    runtime: &Runtime,
    mounted: &mut Option<std::path::PathBuf>,
    project: Option<&std::path::Path>,
    hold: &mut Option<khora_sdk::ReadinessHold>,
) -> Option<khora_sdk::scripts::CompileTicket> {
    let ticket = crate::project_scripts::follow_open_project(runtime, mounted, project);
    if let (Some(ticket), Some(readiness)) =
        (ticket, runtime.resources.get::<Arc<khora_sdk::Readiness>>())
    {
        readiness.wait_for_compile(ticket);
    }
    // The scripts are the last of what the project opens with.
    if project.is_some() {
        *hold = None;
    }
    ticket
}
