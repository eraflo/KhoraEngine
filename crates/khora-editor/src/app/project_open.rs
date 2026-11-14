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

/// Starts opening the project at `root` off the frame, holding the runtime's `Readiness` in `hold`.
///
/// Creates `<root>/assets/`, files an `Open` with the runtime's `IndexHub` (the
/// runtime's `Readiness` waits on it), and fills `editor_state` with what needs
/// no index (folder, name, engine version, git branch); `None`, logged, when it
/// cannot be filed. `trust` is what the user decided: a decision sets the
/// runtime's `ProjectTrustState` at once, before the `Open` lands; `None` files
/// nothing and names the project in `EditorState::trust_prompt`.
pub(crate) fn start_opening_project(
    root: std::path::PathBuf,
    trust: Option<khora_sdk::trust::ProjectTrust>,
    runtime: &Runtime,
    editor_state: &Arc<Mutex<khora_sdk::EditorState>>,
    hold: &mut Option<khora_sdk::ReadinessHold>,
) -> Option<OpeningProject> {
    let Some(trust) = trust else {
        // Nothing decided yet: the open waits for the user's answer.
        log::info!(
            "Opening '{}' waits for the user to decide whether to trust it",
            root.display()
        );
        if let Ok(mut state) = editor_state.lock() {
            state.trust_prompt = Some(root);
        }
        return None;
    };
    match runtime
        .resources
        .get::<Arc<khora_sdk::trust::ProjectTrustState>>()
    {
        Some(state) => state.set_for(&root, trust),
        None => log::error!(
            "the engine holds no project trust state: '{}' opens as the engine reads it",
            root.display()
        ),
    }
    // A project opened trusted — trusted from the hub, say, since a run that
    // opened it restricted — holds none of what the restricted limits
    // refused: the quarantine outlives a run, and those files are read again.
    if trust == khora_sdk::trust::ProjectTrust::Trusted {
        if let Some(refusals) = runtime.resources.get::<Arc<khora_sdk::trust::Refusals>>() {
            refusals.release_strict_under(khora_sdk::trust::Trust::Restricted, &root);
        }
    }
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
    // Read as the game's identity reads them — trimmed, blank as none — so
    // the editor shows the game by the name and version it is built with.
    let text = |key: &str| {
        project_json
            .as_ref()
            .and_then(|json| crate::project_identity::field(json, key))
    };
    let mut choice = 0;
    if let Ok(mut state) = editor_state.lock() {
        choice = state.scene_choices;
        state.project_folder = Some(root.to_string_lossy().to_string());
        state.project_name = text("name");
        state.project_engine_version = text("engine_version");
        state.project_version =
            Some(text("version").unwrap_or_else(|| khora_sdk::DEFAULT_GAME_VERSION.to_owned()));
        state.current_git_branch = git::read_git_branch(&root);
        state.trust_prompt = None;
    }
    log::info!("Opening project '{}' off the frame", root.display());
    Some(OpeningProject {
        root,
        ticket,
        choice,
    })
}

impl super::EditorApp {
    /// Opens the project at `root` at what its user decided of it.
    ///
    /// A project nothing was decided of waits for the user's answer
    /// (`EditorState::trust_prompt`); one that opens replaces the project open.
    pub(super) fn open_at_its_trust(&mut self, root: std::path::PathBuf, runtime: &Runtime) {
        let store = self.trust_store.get_or_insert_with(|| {
            khora_sdk::trust::TrustStore::new(khora_sdk::user_dirs::trust_path(&|key| {
                std::env::var_os(key)
            }))
        });
        if let Some(opening) = open_project(
            root,
            store,
            runtime,
            &self.editor_state,
            &mut self.startup_hold,
        ) {
            self.project_vfs = None;
            self.opening = Some(opening);
        }
    }

    /// Acts on the user's answers about trust, carried by the editor's actions.
    ///
    /// `trust_open` / `restrict_open` decide the project waiting
    /// (`EditorState::trust_prompt`) and open it, `cancel_open` lets it go;
    /// `trust_project` trusts the project open.
    pub(super) fn follow_trust_answers(&mut self, runtime: &Runtime) {
        use khora_sdk::trust::ProjectTrust;
        let Some(action) = self.editor_state.lock().ok().and_then(|mut state| {
            let ours = state
                .pending_menu_action
                .as_deref()
                .is_some_and(|action| TRUST_ACTIONS.contains(&action));
            if ours {
                state.pending_menu_action.take()
            } else {
                None
            }
        }) else {
            return;
        };
        let store = self.trust_store.get_or_insert_with(|| {
            khora_sdk::trust::TrustStore::new(khora_sdk::user_dirs::trust_path(&|key| {
                std::env::var_os(key)
            }))
        });
        let waiting = || {
            self.editor_state
                .lock()
                .ok()
                .and_then(|mut state| state.trust_prompt.take())
        };
        match action.as_str() {
            "trust_open" | "restrict_open" => {
                let Some(root) = waiting() else { return };
                let decision = if action == "trust_open" {
                    ProjectTrust::Trusted
                } else {
                    ProjectTrust::Restricted
                };
                store.decide(&root, decision);
                self.open_at_its_trust(root, runtime);
            }
            "cancel_open" => {
                if let Some(root) = waiting() {
                    log::info!("'{}' is not opened", root.display());
                }
            }
            _ => {
                let open =
                    (self.editor_state.lock().ok()).and_then(|state| state.project_folder.clone());
                match open {
                    Some(root) => grant_trust(runtime, store, std::path::Path::new(&root)),
                    None => log::warn!("no project is open to trust"),
                }
            }
        }
    }

    /// The trust of the project open: `Trusted` when the engine shares none.
    pub(super) fn project_trust(&self) -> khora_sdk::trust::ProjectTrust {
        self.project_trust_state
            .as_ref()
            .map_or(khora_sdk::trust::ProjectTrust::Trusted, |state| state.get())
    }
}

/// The editor's actions that answer about trust.
const TRUST_ACTIONS: [&str; 4] = [
    "trust_open",
    "restrict_open",
    "cancel_open",
    "trust_project",
];

/// Opens the project at `root`, the one way the editor opens a project.
///
/// What `store` holds of it decides its trust, through [`start_opening_project`].
pub(crate) fn open_project(
    root: std::path::PathBuf,
    store: &khora_sdk::trust::TrustStore,
    runtime: &Runtime,
    editor_state: &Arc<Mutex<khora_sdk::EditorState>>,
    hold: &mut Option<khora_sdk::ReadinessHold>,
) -> Option<OpeningProject> {
    let trust = store.decision(&root);
    start_opening_project(root, trust, runtime, editor_state, hold)
}

/// Trusts the project at `project_root`, writing the decision to `store`.
///
/// Sets the runtime's `ProjectTrustState` to `Trusted`, releases what its
/// restricted limits refused, and files its index anew so every reader is
/// built at the new trust. See the book: concepts/trust.md, "The quarantine".
pub(crate) fn grant_trust(
    runtime: &Runtime,
    store: &khora_sdk::trust::TrustStore,
    project_root: &std::path::Path,
) {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};
    store.decide(project_root, ProjectTrust::Trusted);
    if let Some(state) = runtime.resources.get::<Arc<ProjectTrustState>>() {
        state.set_for(project_root, ProjectTrust::Trusted);
    }
    // What the restricted limits refused is read again, at a trusted
    // project's.
    if let Some(refusals) = runtime.resources.get::<Arc<Refusals>>() {
        refusals.release_strict_under(Trust::Restricted, project_root);
    }
    // What the restricted mode listed, an ignored overlay, is cleared.
    if let Some(problems) =
        (runtime.resources).get::<Arc<khora_sdk::khora_core::problems::Problems>>()
    {
        problems.report(
            khora_sdk::khora_core::problems::ProblemSource::Restricted,
            Vec::new(),
        );
    }
    // Every reader of the project is built again, at its new trust.
    match runtime.resources.get::<Arc<khora_sdk::IndexHub>>() {
        Some(indexes) => {
            indexes.file(project_root.to_path_buf(), khora_sdk::IndexKind::Open);
        }
        None => log::error!(
            "the engine holds no index hub: '{}' is trusted from its next open",
            project_root.display()
        ),
    }
    log::info!("'{}' is trusted", project_root.display());
}

/// Lands the project `opening` names once its `Open` landed; `None`, `opening` kept, in flight.
///
/// `opening` is emptied either way. On `Ok`, returns the project's VFS over
/// `shared_assets`, the browser filled and the default scene opened by ticket
/// (the runtime's `Readiness` waits on it), or created when there is none;
/// `hold` is kept for the scripts. On `Err`, reports to the runtime's
/// `Problems` (`ProblemSource::Load`), empties `hold` and clears the project's
/// folder, name, version and branch.
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
                state.project_version = None;
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

    // The default scene, opened by ticket, or created when there is none.
    // The disk is asked, not the index: a scene written after the walk, or
    // spelled with other capitals, is the user's and is never saved over.
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

/// Brings the mounted scripts in line with `project`, as [`follow_open_project`] does.
///
/// The runtime's `Readiness` waits on the compile a mount files; once a
/// project is open, `hold` is let go. Returns the compile's ticket.
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
