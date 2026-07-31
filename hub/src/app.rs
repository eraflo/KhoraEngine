// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `HubApp` — top-level application state + non-rendering domain
//! methods (engine choices, project launch, GitHub auth kickoff,
//! create-project finalisation, …).
//!
//! The `App::update` impl + per-frame async pump lives in
//! `runtime::async_pump`; the per-frame paint composition lives in
//! `bootstrap` (which wires up `App::update`).

use std::path::PathBuf;

use crate::auth;
use crate::config::{EngineInstall, HubConfig, RecentProject};
use crate::github;
use crate::project;
use crate::state::{
    AuthState, Banner, EngineChoice, EngineManagerState, HomeState, NewProjectState, Screen,
    SettingsState,
};

pub struct HubApp {
    pub config: HubConfig,
    pub screen: Screen,
    pub banner: Option<Banner>,
    pub home: HomeState,
    pub new_project: NewProjectState,
    pub engine_manager: EngineManagerState,
    pub settings: SettingsState,
}

impl HubApp {
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
        }
    }

    /// Returns a short label describing the current screen.
    pub fn screen_label(&self) -> &'static str {
        match self.screen {
            Screen::Home => "Projects",
            Screen::NewProject => "New Project",
            Screen::EngineManager => "Engine Manager",
            Screen::Settings => "Settings",
        }
    }

    /// Builds the engine choices for the New Project combo: installed
    /// engines (incl. dev) followed by remote releases not yet
    /// installed and that have a binary for the current platform.
    pub fn engine_choices(&self) -> Vec<EngineChoice> {
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
    pub fn available_engines(&self) -> Vec<EngineInstall> {
        let mut engines = Vec::new();
        if let Some(dev) = self.config.dev_engine() {
            engines.push(dev);
        }
        engines.extend(self.config.engines.clone());
        engines
    }

    /// Launch the editor on `proj` using the matching engine binary.
    /// Surfaces success / failure as a banner.
    pub fn launch_project(&mut self, proj: &RecentProject) {
        let engines = self.available_engines();
        let engine = engines
            .iter()
            .find(|e| e.version == proj.engine_version)
            .or_else(|| engines.first());

        match engine {
            None => {
                self.banner = Some(Banner::error(format!(
                    "No engine available for version '{}'. Configure one in Engine Manager.",
                    proj.engine_version
                )));
            }
            Some(engine) => {
                let path = PathBuf::from(&proj.path);
                match project::launch_editor(&engine.editor_binary, &path) {
                    Ok(()) => {
                        self.config
                            .push_recent(&proj.name, &path, &proj.engine_version);
                        let _ = self.config.save();
                        self.banner =
                            Some(Banner::info(format!("Opened '{}' in editor.", proj.name)));
                    }
                    Err(e) => {
                        self.banner = Some(Banner::error(format!("Failed to launch editor: {e}")));
                    }
                }
            }
        }
    }

    /// Open a project that already exists on disk (from a folder
    /// picker). Reads `project.json` if present, otherwise infers
    /// name / engine version from the folder.
    pub fn open_existing_project(&mut self, path: PathBuf) {
        let descriptor_path = path.join("project.json");
        let (name, engine_version) = if descriptor_path.exists() {
            let text = std::fs::read_to_string(&descriptor_path).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            (
                v["name"].as_str().unwrap_or("Unknown").to_owned(),
                v["engine_version"].as_str().unwrap_or("dev").to_owned(),
            )
        } else {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Project".to_owned());
            (name, "dev".to_owned())
        };

        let proj = RecentProject {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            engine_version: engine_version.clone(),
            last_opened: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.launch_project(&proj);
        self.config.push_recent(&name, &path, &engine_version);
        let _ = self.config.save();
    }

    /// Kick off the GitHub OAuth Device Flow if not already in
    /// progress. Used both by the topbar Connect button and the
    /// Settings screen.
    pub fn start_github_auth(&mut self) {
        if matches!(self.settings.auth, AuthState::Connecting { .. })
            || self.settings.auth_rx.is_some()
        {
            return;
        }
        self.settings.auth = AuthState::Connecting {
            device_code: None,
            message: "Requesting device code…".to_owned(),
        };
        self.settings.auth_rx = Some(auth::start_device_flow());
    }

    /// Resolve the user's Git options into a [`project::GitInit`]. If
    /// the remote-creation step fails, downgrades to local-only and
    /// surfaces a banner.
    pub fn build_git_init(&mut self) -> project::GitInit {
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

    /// Called once a remote engine has finished downloading during a
    /// "Create Project" flow. Writes the project on disk, registers it
    /// in the config, and launches the editor.
    pub fn finalize_new_project_creation(&mut self, engine: &EngineInstall) {
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
