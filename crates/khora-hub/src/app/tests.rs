// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! **The hub asks for trust before it launches a project it never decided
//! of.** `launch_project` reads the trust store the editor shares: a project
//! with no decision is not launched, and the hub names it for its prompt; a
//! project decided of — trusted or restricted — is launched.
//!
//! No test reads or writes the user's home folder: the store is a file under
//! the temp folder, the configuration is built in memory and never saved, and
//! the engine's editor binary does not exist — a launch is seen trying it and
//! failing, so no process is ever spawned.

use std::path::PathBuf;

use khora_sdk::trust::{ProjectTrust, TrustStore};

use super::HubApp;
use crate::services::config::{EngineInstall, HubConfig, RecentProject};
use crate::state::{EngineManagerState, HomeState, NewProjectState, Screen, SettingsState};

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-hub-trust-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }

    /// A project named `name` in this folder, as the hub lists it.
    fn project(&self, name: &str) -> RecentProject {
        let root = self.0.join(name);
        std::fs::create_dir_all(root.join("assets")).expect("the project folder");
        RecentProject {
            name: name.to_owned(),
            path: root.to_string_lossy().into_owned(),
            engine_version: "dev".to_owned(),
            last_opened: 0,
        }
    }

    /// The trust store's file in this folder, as `~/.khora/trust.ron` would
    /// be.
    fn store(&self) -> PathBuf {
        self.0.join("khora").join("trust.ron")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A hub built in memory over the trust store at `store`, knowing one engine
/// whose editor binary does not exist: a launch tries it, and fails.
fn hub(folder: &Folder, store: PathBuf) -> HubApp {
    HubApp {
        config: HubConfig {
            recent_projects: Vec::new(),
            engines: vec![EngineInstall {
                version: "dev".to_owned(),
                editor_binary: folder
                    .0
                    .join("no-editor-here")
                    .to_string_lossy()
                    .into_owned(),
                runtime_binary: None,
                source: "local".to_owned(),
            }],
            local_engine_repo: None,
        },
        screen: Screen::Home,
        banner: None,
        home: HomeState::default(),
        new_project: NewProjectState::new(),
        engine_manager: EngineManagerState::new(String::new()),
        settings: SettingsState::default(),
        splash: None,
        trust_store: TrustStore::new(Some(store)),
        trust_prompt: None,
    }
}

/// Whether the hub tried to launch the editor: the binary does not exist, so
/// a launch tried says it failed.
fn tried_to_launch(hub: &HubApp) -> bool {
    hub.banner
        .as_ref()
        .is_some_and(|banner| banner.message.contains("Failed to launch editor"))
}

/// **An unknown project asks for trust once.** Launched with nothing decided
/// of it, the project is not launched — the editor is not even tried — and
/// the hub names it for its prompt. Once decided, the same launch launches
/// it, and nothing waits for a decision; a hub opened afresh over the same
/// store launches it at once.
#[test]
fn an_unknown_project_asks_for_trust_once() {
    let folder = Folder::new("asks-once");
    let ember = folder.project("ember");
    let mut app = hub(&folder, folder.store());

    app.launch_project(&ember);

    assert!(
        !tried_to_launch(&app),
        "a project never decided of is not launched"
    );
    assert_eq!(
        app.trust_prompt
            .as_ref()
            .map(|project| project.path.clone()),
        Some(ember.path.clone()),
        "the hub names the project waiting for a decision"
    );

    app.trust_store
        .decide(&PathBuf::from(&ember.path), ProjectTrust::Trusted);
    app.trust_prompt = None;
    app.launch_project(&ember);

    assert!(tried_to_launch(&app), "decided of, the project is launched");
    assert!(app.trust_prompt.is_none(), "nothing waits for a decision");

    let mut reopened = hub(&folder, folder.store());
    reopened.launch_project(&ember);
    assert!(
        tried_to_launch(&reopened),
        "the decision is remembered: the hub does not ask again"
    );
    assert!(reopened.trust_prompt.is_none());
}

/// **A project opened restricted is launched too**: the decision is the
/// user's, and the editor reads it from the store; the hub asks nothing.
#[test]
fn a_restricted_project_is_launched() {
    let folder = Folder::new("restricted");
    let lantern = folder.project("lantern");
    let mut app = hub(&folder, folder.store());
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Restricted);

    app.launch_project(&lantern);

    assert!(tried_to_launch(&app), "a restricted project is launched");
    assert!(app.trust_prompt.is_none());
}
