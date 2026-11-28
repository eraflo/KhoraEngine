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

//! Opening a project, and letting go of the one the editor held.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::editor_ui::{EditorRequest, SceneInProject, TrustAnswer};
use khora_sdk::khora_core::scene::LoadTicket;
use khora_sdk::{GameWorld, Runtime};

use crate::project_vfs::ProjectVfs;
use crate::{git, hot_reload, scene_io};

/// The sources of the problems the editor files of the project it holds: they leave with the project.
///
/// What a rename left aside, and what the last save of a file of the
/// project left out. What the last load reported (`Load`) describes the open
/// scene and leaves with it, as every scope says on `ProblemSource`.
const PROJECT_SOURCES: [khora_sdk::khora_core::problems::ProblemSource; 2] = [
    khora_sdk::khora_core::problems::ProblemSource::Leftover,
    khora_sdk::khora_core::problems::ProblemSource::Save,
];

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
    /// The scene loads in flight when it was filed: the held project's, which land nothing once it is let go.
    ///
    /// An opened scene's, and Stop's restore.
    pub(crate) filed_before: Vec<LoadTicket>,
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
    // branch, read at the trust just decided, reporting to the run's refusals.
    let project = khora_sdk::trust::FileGuard::new(
        &root,
        trust.trust(),
        khora_sdk::trust::Refusals::of(runtime),
    );
    let project_json = crate::project_identity::read_project_json(&project);
    // Read as the game's identity reads them — trimmed, blank as none — so
    // the editor shows the game by the name and version it is built with.
    let text = |key: &str| {
        project_json
            .as_ref()
            .and_then(|json| crate::project_identity::field(json, key))
    };
    let mut choice = 0;
    let mut filed_before = Vec::new();
    if let Ok(mut state) = editor_state.lock() {
        choice = state.scene_choices;
        filed_before = (state.opening.iter().map(|opening| opening.ticket))
            .chain(state.restoring)
            .collect();
        state.project_folder = Some(root.to_string_lossy().to_string());
        state.project_name = text("name");
        state.project_engine_version = text("engine_version");
        state.project_version =
            Some(text("version").unwrap_or_else(|| khora_sdk::DEFAULT_GAME_VERSION.to_owned()));
        state.current_git_branch = git::read_git_branch(&project);
        state.trust_prompt = None;
    }
    log::info!("Opening project '{}' off the frame", root.display());
    Some(OpeningProject {
        root,
        ticket,
        choice,
        filed_before,
    })
}

impl super::EditorApp {
    /// Returns the store of what the user decided of each project, opened at its first use.
    ///
    /// The one the hub shares, `~/.khora/trust.ron`; an editor that opens no
    /// project never reads it.
    pub(super) fn trust_store(&mut self) -> Arc<khora_sdk::trust::TrustStore> {
        let store = self.trust_store.get_or_insert_with(|| {
            Arc::new(khora_sdk::trust::TrustStore::new(
                khora_sdk::user_dirs::trust_path(&khora_sdk::user_dirs::process_env),
            ))
        });
        Arc::clone(store)
    }

    /// Opens the project at `root` at what its user decided of it.
    ///
    /// A project nothing was decided of waits for the user's answer
    /// (`EditorState::trust_prompt`); one that opens replaces the project open.
    pub(super) fn open_at_its_trust(&mut self, root: std::path::PathBuf, runtime: &Runtime) {
        let store = self.trust_store();
        if let Some(opening) = open_project(
            root.clone(),
            &store,
            runtime,
            &self.editor_state,
            &mut self.startup_hold,
        ) {
            self.project_vfs = None;
            self.opening = Some(opening);
            self.follow_project_physics(&root, runtime);
        }
    }

    /// Acts on the user's answers about trust, queued on the editor's state, oldest first.
    ///
    /// `TrustOpen` / `RestrictOpen` decide the project waiting
    /// (`EditorState::trust_prompt`) and open it, `CancelOpen` lets it go;
    /// `TrustProject` trusts the project open, reads its identity registry
    /// again and steps its physics at the step read at its new trust.
    pub(super) fn follow_trust_answers(&mut self, runtime: &Runtime) {
        let answers = self
            .editor_state
            .lock()
            .map(|mut state| {
                state.take_requests(|request| match request {
                    EditorRequest::Trust(answer) => Ok(answer),
                    other => Err(other),
                })
            })
            .unwrap_or_default();
        for answer in answers {
            self.follow_trust_answer(answer, runtime);
        }
    }

    /// Acts on one answer about trust.
    fn follow_trust_answer(&mut self, answer: TrustAnswer, runtime: &Runtime) {
        use khora_sdk::trust::ProjectTrust;
        let store = self.trust_store();
        let waiting = || {
            self.editor_state
                .lock()
                .ok()
                .and_then(|mut state| state.trust_prompt.take())
        };
        match answer {
            TrustAnswer::TrustOpen | TrustAnswer::RestrictOpen => {
                let Some(root) = waiting() else { return };
                let decision = if answer == TrustAnswer::TrustOpen {
                    ProjectTrust::Trusted
                } else {
                    ProjectTrust::Restricted
                };
                store.decide(&root, decision);
                self.open_at_its_trust(root, runtime);
            }
            TrustAnswer::CancelOpen => {
                if let Some(root) = waiting() {
                    log::info!("'{}' is not opened", root.display());
                }
            }
            TrustAnswer::TrustProject => {
                let open =
                    (self.editor_state.lock().ok()).and_then(|state| state.project_folder.clone());
                let Some(root) = open.map(std::path::PathBuf::from) else {
                    log::warn!("no project is open to trust");
                    return;
                };
                grant_trust(runtime, &store, &root);
                // What was read under the restricted caps is read again.
                if let Some(pvfs) = &self.project_vfs {
                    khora_sdk::khora_core::sync::LockExt::lock_recovered(&**pvfs)
                        .reread_at_its_trust();
                }
                self.follow_project_physics(&root, runtime);
            }
        }
    }

    /// Returns the trust of the project open: `Trusted` when the engine shares
    /// none.
    pub(super) fn project_trust(&self) -> khora_sdk::trust::ProjectTrust {
        self.project_trust_state
            .as_ref()
            .map_or(khora_sdk::trust::ProjectTrust::Trusted, |state| state.get())
    }

    /// Runs the physics family at the `physics.step` of the project at `root`, read at the trust decided of it.
    ///
    /// Build Game reads it at the same trust, so Play steps as the game it
    /// builds. Its refusals go to the run's.
    fn follow_project_physics(&mut self, root: &std::path::Path, runtime: &Runtime) {
        let decided = self.trust_store().decision(root);
        let settings = crate::project_physics::physics_at(
            root,
            decided,
            khora_sdk::trust::Refusals::of(runtime),
        );
        let applied = self.agent_registry.as_ref().is_some_and(|registry| {
            crate::project_physics::apply_physics_step(
                &khora_sdk::khora_core::sync::LockExt::lock_recovered(&**registry),
                settings,
            )
        });
        if !applied {
            log::warn!(
                "the engine runs no physics family: the physics.step of '{}' is not applied",
                root.display()
            );
        }
    }
}

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
/// `opening` is emptied either way, and the project the editor held is let
/// go first ([`leave_project`]): the new project's scene starts from an empty
/// world. On `Ok`, returns the project's VFS over `shared_assets`, what a
/// rename cut short left aside put back (what stays reported as
/// `ProblemSource::Leftover`), the browser filled and, unless the user chose
/// a scene since the open was filed, the default scene opened by ticket (the
/// runtime's `Readiness` waits on it), or created and saved when there is
/// none; either becomes the scene a save writes to once there. `hold` is kept
/// for the scripts. On `Err`, returns `None` with `opening` emptied: the
/// editor holds no project. The failure joins the open scene's load problems,
/// in the editor's state and the runtime's `Problems` (`ProblemSource::Load`);
/// `hold` is emptied and the project's folder, name, version and branch
/// cleared.
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
    let OpeningProject {
        root,
        choice,
        filed_before,
        ..
    } = opening.take()?;
    let chosen_since = editor_state
        .lock()
        .is_ok_and(|state| state.scene_choices != choice);
    leave_project(world, editor_state, runtime, &filed_before, chosen_since);
    let opened = outcome.map_err(|why| why.to_string()).and_then(|()| {
        // Its reads report to the run's refusals, at the trust its user decided.
        ProjectVfs::over_service(
            root.clone(),
            Arc::clone(shared_assets),
            Arc::clone(asset_watch),
            crate::project_vfs::ProjectReads::of(runtime),
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
            // The editor runs with no project: nothing of it is shown as
            // open, and nothing more is filed — the film goes.
            if let Ok(mut state) = editor_state.lock() {
                // The failure joins what the scene kept was loaded with.
                state
                    .load_problems
                    .push(khora_sdk::khora_core::problems::Problem::new(
                        khora_sdk::khora_core::problems::ProblemSeverity::Error,
                        format!(
                            "the project '{}' could not be opened: {why}",
                            root.display()
                        ),
                    ));
                report_load_problems(&mut state, runtime);
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
    reclaim_leftovers(&mut pvfs, runtime);
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
    if chosen_since {
        // The user chose a scene meanwhile (the film gave up waiting): it
        // stays.
    } else if abs.exists() {
        if let Some(loads) = runtime.resources.get::<Arc<khora_sdk::SceneLoadHub>>() {
            let ticket = crate::commands::open_scene_dispatch(
                Some(&pvfs),
                world,
                editor_state,
                loads,
                &crate::project_vfs::ProjectReads::of(runtime),
                &abs,
            );
            if let (Some(ticket), Some(readiness)) =
                (ticket, runtime.resources.get::<Arc<khora_sdk::Readiness>>())
            {
                readiness.wait_for_scene(ticket);
            }
        }
    } else if let Ok(mut pvfs) = pvfs.lock() {
        if scene_io::create_default_scene_in_project(&mut pvfs, world, rel) {
            if let Ok(mut state) = editor_state.lock() {
                state.current_scene_path = Some(SceneInProject::new(pvfs.assets_root.clone(), rel));
            }
        }
    }
    Some(pvfs)
}

/// Lets go of the project the editor held, as an open resolves: another project, the same again, or none when it failed.
///
/// Every project change goes through it, from [`land_opened_project`]. The
/// scene loads in flight when the open was filed (`filed_before`) are
/// withdrawn through the world's `SceneLoadHub`, and land nothing. The play
/// under way ends with the project it played: unless it plays the scene the
/// user chose since the open was filed (`chosen_since`, and the play started
/// on that choice, `EditorState::played_choice`), its snapshot is dropped and
/// a Stop's restore in flight withdrawn, so no Stop restores a scene of
/// another project. A play of the scene chosen ends as Stop ends it
/// ([`stop_play`]): its restore, or the one its Stop filed, lands the scene as
/// it was chosen, in Editing. Unless the user chose a scene since the open
/// was filed, the world is emptied as New Scene empties it ([`clear_scene`]):
/// every entity, what names one, what its load reported and every other load
/// in flight; a scene chosen keeps its load problems. Either way it forgets
/// the scene a save writes to, the requests and edits queued against the
/// project ([`EditorRequest::names_the_project`]), the asset browser's
/// listing, selection, reindex and key changes, the trust confirmation and the
/// last refusal, and what the runtime's `Problems` hold of the project
/// ([`PROJECT_SOURCES`]); the `Load` problems it holds are those of the scene
/// kept. The undo history, the app's, is emptied by the app where the open
/// resolves.
///
/// [`clear_scene`]: crate::commands::clear_scene
/// [`stop_play`]: crate::commands::stop_play
fn leave_project(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<khora_sdk::EditorState>>,
    runtime: &Runtime,
    filed_before: &[LoadTicket],
    chosen_since: bool,
) {
    if let Ok(mut state) = editor_state.lock() {
        let loads = world.scene_loads().cloned();
        // The play, or the restore its Stop filed, of the scene chosen since
        // the open was filed: it holds that scene as chosen, and is kept.
        let plays_the_choice =
            loads.is_some() && chosen_since && state.played_choice == Some(state.scene_choices);
        let mut withdrawn = filed_before.to_vec();
        if !plays_the_choice {
            withdrawn.extend(state.restoring.take());
        }
        match &loads {
            Some(loads) => {
                for ticket in &withdrawn {
                    loads.cancel(*ticket);
                    // Landed already, its outcome is the former project's.
                    let _ = loads.take_outcome(*ticket);
                }
                if plays_the_choice {
                    crate::commands::stop_play(world, &mut state, loads);
                }
            }
            None if !withdrawn.is_empty() => {
                log::error!("the former project's scene loads in flight cannot be withdrawn");
            }
            None => {}
        }
        state
            .opening
            .retain(|opening| !filed_before.contains(&opening.ticket));
        // Play ends with the project it played: the scene Stop would restore
        // is the former project's, unless it is the one chosen since.
        state.play_mode = khora_sdk::PlayMode::Editing;
        if !plays_the_choice {
            state.scene_snapshot = None;
            state.played_choice = None;
            state.restore_problems.clear();
        }
        if !chosen_since {
            crate::commands::clear_scene(world, &mut state);
        }
        state.current_scene_path = None;
        let _ = state.take_requests(|request| {
            if request.names_the_project() {
                Ok(())
            } else {
                Err(request)
            }
        });
        state.pending_edits.clear();
        state.asset_entries.clear();
        state.asset_dirs.clear();
        state.asset_epoch = state.asset_epoch.wrapping_add(1);
        state.reindexing = None;
        state.key_changes.clear();
        state.selected_asset = None;
        state.inspected_asset_path = None;
        state.asset_picker_query.clear();
        state.trust_confirm = None;
        state.refusal = None;
        // The report of the scene's problems the queue held went with it.
        report_load_problems(&mut state, runtime);
    }
    let Some(problems) =
        (runtime.resources).get::<Arc<khora_sdk::khora_core::problems::Problems>>()
    else {
        return;
    };
    for source in PROJECT_SOURCES {
        problems.report(source, Vec::new());
    }
}

/// Reports the open scene's load problems, as `state` holds them, to the runtime's `Problems` at once.
///
/// What the queued `ReportLoadProblems` would report, the state's list being
/// the last one asked: any still queued is taken, so none reports an older
/// list after.
fn report_load_problems(state: &mut khora_sdk::EditorState, runtime: &Runtime) {
    let _ = state.take_requests(|request| match request {
        EditorRequest::ReportLoadProblems { .. } => Ok(()),
        other => Err(other),
    });
    if let Some(problems) =
        (runtime.resources).get::<Arc<khora_sdk::khora_core::problems::Problems>>()
    {
        problems.report(
            khora_sdk::khora_core::problems::ProblemSource::Load,
            state.load_problems.clone(),
        );
    }
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

/// Puts back what a rename cut short left aside in the project `pvfs` opens.
///
/// What stays aside, its name taken, is reported to the runtime's `Problems`
/// (`ProblemSource::Leftover`) as a warning each, replacing what an earlier
/// open reported; what was put back is logged.
fn reclaim_leftovers(pvfs: &mut ProjectVfs, runtime: &Runtime) {
    use khora_sdk::khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
    let mut left = Vec::new();
    for reclaimed in pvfs.reclaim_asides() {
        match reclaimed {
            khora_sdk::fs::Reclaimed::Restored { .. } => log::info!("{reclaimed}"),
            stays => {
                log::warn!("{stays}");
                left.push(Problem::new(ProblemSeverity::Warning, stays.to_string()));
            }
        }
    }
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
        problems.report(ProblemSource::Leftover, left);
    }
}
