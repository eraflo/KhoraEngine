// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `HubApp`: the hub's state and its non-rendering actions (engine choices,
//! project launch, GitHub sign-in, project creation).
//!
//! The per-frame drain of background channels lives in `async_pump`; the
//! `App::update` impl that paints a frame lives in `bootstrap`.

use std::path::PathBuf;
use std::time::Instant;

use khora_sdk::trust::TrustStore;
use khora_tool_ui::widgets::LoadingScreen;
use khora_tool_ui::widgets::film::Film;

use crate::services::config::{EngineInstall, HubConfig, RecentProject};
use crate::services::github;
use crate::services::github::auth;
use crate::services::project;
use crate::state::{
    AuthState, Banner, EngineChoice, EngineManagerState, HomeState, NewProjectState,
    ProjectRefusal, Screen, SettingsState,
};

/// The hub's whole state: its config, the screen shown and each screen's state.
pub struct HubApp {
    /// The hub's settings, saved on disk.
    pub config: HubConfig,
    /// The screen shown.
    pub screen: Screen,
    /// The message over the screen, if any.
    pub banner: Option<Banner>,
    /// The projects screen's state.
    pub home: HomeState,
    /// The New Project screen's state.
    pub new_project: NewProjectState,
    /// The Engine Manager screen's state.
    pub engine_manager: EngineManagerState,
    /// The Settings screen's state, GitHub sign-in included.
    pub settings: SettingsState,
    /// The sign film over the hub while it opens, with the clock it plays
    /// on; gone once it has faded out.
    pub splash: Option<(LoadingScreen, Instant)>,
    /// What the user decided of each project, shared with the editor.
    pub trust_store: TrustStore,
    /// The project whose launch waits for the user's trust decision: it was
    /// never decided of, and is not launched until it is.
    pub trust_prompt: Option<RecentProject>,
}

impl HubApp {
    /// Loads the config and restores the GitHub session whose token is on disk.
    ///
    /// A stored token GitHub refuses is forgotten.
    pub fn new() -> Self {
        let config = HubConfig::load();
        let local_repo = config.local_engine_repo.clone().unwrap_or_default();

        // Restore previous GitHub session if a token is on disk.
        let auth = match auth::load_token() {
            Some(token) => match github::get_authenticated_user(&token) {
                Ok(user) => AuthState::Connected {
                    token,
                    login: user.login,
                },
                Err(e) => {
                    log::warn!("Stored GitHub token is invalid, forgetting: {e}");
                    let _ = auth::forget_token();
                    AuthState::Disconnected
                }
            },
            None => AuthState::Disconnected,
        };

        let settings = SettingsState {
            auth,
            auth_rx: None,
            local_repo_draft: local_repo.clone(),
        };

        Self {
            engine_manager: EngineManagerState::new(local_repo),
            new_project: NewProjectState::new(),
            config,
            screen: Screen::Home,
            banner: None,
            home: HomeState::default(),
            settings,
            splash: Some((LoadingScreen::new(Film::Sign, 0.0, false), Instant::now())),
            trust_store: TrustStore::new(khora_sdk::user_dirs::trust_path(
                &khora_sdk::user_dirs::process_env,
            )),
            trust_prompt: None,
        }
    }

    /// Returns a short label describing the current screen.
    pub(crate) fn screen_label(&self) -> &'static str {
        match self.screen {
            Screen::Home => "Projects",
            Screen::NewProject => "New Project",
            Screen::EngineManager => "Engine Manager",
            Screen::Settings => "Settings",
        }
    }

    /// Returns the New Project engine choices: installed engines, then releases to download.
    ///
    /// A release is offered only when it is not installed and has an editor for this platform.
    pub(crate) fn engine_choices(&self) -> Vec<EngineChoice> {
        let mut out: Vec<EngineChoice> = self
            .available_engines()
            .into_iter()
            .map(EngineChoice::Installed)
            .collect();

        let installed: std::collections::HashSet<String> =
            out.iter().map(|c| c.version().to_owned()).collect();

        for r in &self.new_project.releases {
            if installed.contains(&r.tag_name) {
                continue;
            }
            if let Some(asset) = r.editor_asset() {
                let runtime = r.runtime_asset();
                out.push(EngineChoice::Remote {
                    version: r.tag_name.clone(),
                    download_url: asset.browser_download_url.clone(),
                    size: asset.size,
                    runtime_url: runtime.map(|a| a.browser_download_url.clone()),
                    runtime_size: runtime.map(|a| a.size),
                });
            }
        }
        out
    }

    /// Returns all available engine options (dev + installed).
    pub(crate) fn available_engines(&self) -> Vec<EngineInstall> {
        let mut engines = Vec::new();
        if let Some(dev) = self.config.dev_engine() {
            engines.push(dev);
        }
        engines.extend(self.config.engines.clone());
        engines
    }

    /// Launches the editor on `proj` with its engine version, or the first engine.
    ///
    /// The outcome is shown as a banner built for this launch alone: the
    /// refusals still listed of the project, then what the launch said. A
    /// project the user never decided of is not launched: it waits for their
    /// answer (`trust_prompt`), its refusals shown alone.
    pub fn launch_project(&mut self, proj: &RecentProject) {
        let path = PathBuf::from(&proj.path);
        let outcome = self.launch_outcome(proj, &path);
        self.banner = Banner::of_launch(&path, self.home.refusals.of(&path), outcome);
    }

    /// Dismisses the banner, and the refusals it shows.
    pub(crate) fn dismiss_banner(&mut self) {
        if let Some(project) = self.banner.take().and_then(|banner| banner.refusals_of) {
            self.home.refusals.dismiss(&project);
        }
    }

    /// Applies the user's answer to the trust prompt and closes it.
    ///
    /// A decision is written to the trust store and the waiting project is
    /// launched; `None` lets it go, launching nothing.
    pub fn answer_trust(&mut self, answer: Option<khora_sdk::trust::ProjectTrust>) {
        let Some(proj) = self.trust_prompt.take() else {
            return;
        };
        if let Some(decision) = answer {
            self.trust_store
                .decide(std::path::Path::new(&proj.path), decision);
            self.home.cards.forget();
            self.launch_project(&proj);
        }
    }

    /// Restricts `proj` in the trust store and says so in a banner.
    ///
    /// Behind the `Restrict this project` item of a project card's `More` menu.
    pub fn restrict_project(&mut self, proj: &RecentProject) {
        self.trust_store.decide(
            std::path::Path::new(&proj.path),
            khora_sdk::trust::ProjectTrust::Restricted,
        );
        self.home.cards.forget();
        self.banner = Some(Banner::info(format!(
            "'{}' is restricted: its scripts, shaders and Build Game are off.",
            proj.name
        )));
    }

    /// Opens the project at `path` and adds it to the recent projects, saving the hub's config.
    ///
    /// As [`Self::launch_existing_project`].
    pub(crate) fn open_existing_project(&mut self, path: PathBuf) {
        let proj = self.launch_existing_project(&path);
        self.config
            .push_recent(&proj.name, &path, &proj.engine_version);
        let _ = self.config.save();
    }

    /// Launches the project at `path` and returns it as the recent projects list it.
    ///
    /// Its name and engine version come from `project.json`, read under the
    /// strict caps, or else from the folder name and `dev`; a file refused or
    /// unreadable is kept in the refusals listed of the project, shown until
    /// dismissed, with what the launch then says.
    fn launch_existing_project(&mut self, path: &std::path::Path) -> RecentProject {
        let folder_name = || {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Project".to_owned());
            (name, "dev".to_owned())
        };
        let mut refusal = None;
        let (name, engine_version) = match crate::services::project::read_project_json(path) {
            Ok(Some(v)) => (
                v["name"].as_str().unwrap_or("Unknown").to_owned(),
                v["engine_version"].as_str().unwrap_or("dev").to_owned(),
            ),
            Ok(None) => folder_name(),
            Err(error) => {
                refusal = Some(format!("project.json is not read: {error}"));
                folder_name()
            }
        };

        let proj = RecentProject {
            name,
            path: path.to_string_lossy().to_string(),
            engine_version,
            last_opened: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        if let Some(message) = refusal {
            self.home.refusals.record(ProjectRefusal {
                project: path.to_path_buf(),
                name: proj.name.clone(),
                message,
            });
        }
        self.launch_project(&proj);
        proj
    }

    /// Launches the editor on `proj`, at `path`, and returns what the launch says; `None` while it waits for a trust decision.
    ///
    /// A project the user never decided of is not launched: it is named for
    /// the trust prompt. A launched project is added to the recent projects,
    /// the hub's config saved.
    fn launch_outcome(&mut self, proj: &RecentProject, path: &std::path::Path) -> Option<Banner> {
        if self.trust_store.decision(path).is_none() {
            self.trust_prompt = Some(proj.clone());
            return None;
        }
        let engines = self.available_engines();
        let engine = engines
            .iter()
            .find(|e| e.version == proj.engine_version)
            .or_else(|| engines.first());
        let Some(engine) = engine else {
            return Some(Banner::error(format!(
                "No engine available for version '{}'. Configure one in Engine Manager.",
                proj.engine_version
            )));
        };
        Some(match project::launch_editor(&engine.editor_binary, path) {
            Ok(()) => {
                self.config
                    .push_recent(&proj.name, path, &proj.engine_version);
                let _ = self.config.save();
                Banner::info(format!("Opened '{}' in editor.", proj.name))
            }
            Err(e) => Banner::error(format!("Failed to launch editor: {e}")),
        })
    }

    /// Starts the GitHub OAuth device flow, unless one is already running.
    pub(crate) fn start_github_auth(&mut self) {
        if matches!(self.settings.auth, AuthState::Connecting { .. })
            || self.settings.auth_rx.is_some()
        {
            return;
        }
        self.settings.auth = AuthState::Connecting {
            message: "Requesting device code…".to_owned(),
        };
        self.settings.auth_rx = Some(auth::start_device_flow());
    }

    /// Returns the [`project::GitInit`] the user's Git options ask for.
    ///
    /// Creates the GitHub repository when a remote is asked for. Without a
    /// GitHub session, or when the creation fails, it falls back to a local
    /// repository and says so in a banner.
    pub(crate) fn build_git_init(&mut self) -> project::GitInit {
        if !self.new_project.git_init {
            return project::GitInit::None;
        }
        if !self.new_project.git_remote {
            return project::GitInit::Local;
        }
        let Some(token) = self.settings.auth.token().map(str::to_owned) else {
            self.banner = Some(Banner::error(
                "Not connected to GitHub — falling back to local Git only.",
            ));
            return project::GitInit::Local;
        };
        let repo_name = if self.new_project.remote_repo_name.trim().is_empty() {
            self.new_project.name.clone()
        } else {
            self.new_project.remote_repo_name.clone()
        };
        match github::create_repo(&token, &repo_name, self.new_project.remote_private) {
            Ok(repo) => {
                self.banner = Some(Banner::info(format!(
                    "Created GitHub repo {}",
                    repo.full_name
                )));
                project::GitInit::LocalAndRemote {
                    remote_url: repo.clone_url,
                    push: self.new_project.remote_push,
                }
            }
            Err(e) => {
                self.banner = Some(Banner::error(format!(
                    "Could not create remote repo: {e} — falling back to local Git only."
                )));
                project::GitInit::Local
            }
        }
    }

    /// Creates the New Project screen's project with `engine`, trusts it and opens it.
    ///
    /// Call once the engine a project creation waited for is downloaded.
    pub(crate) fn finalize_new_project_creation(&mut self, engine: &EngineInstall) {
        self.new_project.creating_after_download = false;

        let version = engine.version.clone();
        let git = self.build_git_init();

        let result = project::create_project(
            &self.new_project.name,
            &PathBuf::from(&self.new_project.path),
            &version,
            &git,
        );

        match result {
            Ok(root) => {
                self.new_project.success = true;
                self.new_project.status = Some(format!("Project created at: {}", root.display()));
                self.config
                    .push_recent(&self.new_project.name, &root, &version);
                let _ = self.config.save();
                // A project the user just made is theirs: trusted.
                self.trust_store
                    .decide(&root, khora_sdk::trust::ProjectTrust::Trusted);
                self.home.cards.forget();

                match project::launch_editor(&engine.editor_binary, &root) {
                    Ok(()) => {
                        self.banner = Some(Banner::info("Editor launched!"));
                    }
                    Err(e) => {
                        self.banner = Some(Banner::error(format!(
                            "Project created but editor launch failed: {e}"
                        )));
                    }
                }
                self.screen = Screen::Home;
            }
            Err(e) => {
                self.new_project.success = false;
                self.new_project.status = Some(format!("Error: {e}"));
                self.banner = Some(Banner::error(format!("Project creation failed: {e}")));
            }
        }
    }
}

impl Default for HubApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
