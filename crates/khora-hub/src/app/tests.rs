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

/// **A refused `project.json` stays shown beside what the launch said.**
///
/// Opened from disk, a project whose `project.json` is over the strict cap is
/// still launched, by its folder's name; the banner says the file was refused
/// and that the launch failed, and stays until the user dismisses it. Never
/// decided of, the project waits for its trust prompt, the refusal shown alone.
#[test]
fn a_refused_project_json_stays_shown_beside_the_launch() {
    let folder = Folder::new("refused-json");
    let ember = folder.project("ember");
    let root = PathBuf::from(&ember.path);
    std::fs::File::create(root.join("project.json"))
        .and_then(|file| file.set_len((256 << 10) + 1))
        .expect("a project.json over the strict cap");
    let mut app = hub(&folder, folder.store());

    let held = app.launch_existing_project(&root);

    assert_eq!(held.name, "ember", "named by its folder");
    assert!(app.trust_prompt.is_some(), "never decided of, it waits");
    let banner = app.banner.as_ref().expect("the refusal is shown");
    assert!(
        banner.message.contains("project.json is not read"),
        "{}",
        banner.message
    );
    assert!(!banner.is_expired() && banner.expires_at.is_none());

    app.trust_prompt = None;
    app.banner = None;
    app.trust_store.decide(&root, ProjectTrust::Trusted);
    app.launch_existing_project(&root);

    let banner = app.banner.as_ref().expect("the refusal is shown");
    assert!(
        banner.message.contains("project.json is not read"),
        "{}",
        banner.message
    );
    assert!(
        banner.message.contains("Failed to launch editor"),
        "{}",
        banner.message
    );
    assert!(banner.is_error);
    assert!(banner.expires_at.is_none(), "it stays until dismissed");
}

/// **A refused `project.json` stays shown once the trust prompt is answered.**
///
/// Never decided of, the project waits for its prompt, the refusal shown; the
/// launch the answer starts says what it did beside the refusal, never in its place.
#[test]
fn a_refusal_stays_shown_once_the_trust_prompt_is_answered() {
    let folder = Folder::new("refused-then-answered");
    let ember = folder.project("ember");
    let root = PathBuf::from(&ember.path);
    std::fs::File::create(root.join("project.json"))
        .and_then(|file| file.set_len((256 << 10) + 1))
        .expect("a project.json over the strict cap");
    let mut app = hub(&folder, folder.store());
    app.launch_existing_project(&root);
    assert!(app.trust_prompt.is_some(), "the premise: it waits");

    app.answer_trust(Some(ProjectTrust::Trusted));

    assert!(tried_to_launch(&app), "the answer launches it");
    let banner = app.banner.as_ref().expect("a banner is shown");
    assert!(
        banner.message.contains("project.json is not read"),
        "the refusal is gone before it was dismissed: {}",
        banner.message
    );
}

/// **A refusal is shown with what its own launch said, never with an earlier one's.**
///
/// A banner an earlier launch left is not this launch's: a refused
/// `project.json` of a project waiting for its trust prompt is shown alone.
#[test]
fn a_refusal_is_never_shown_with_what_an_earlier_launch_said() {
    let folder = Folder::new("refused-after-another");
    let lantern = folder.project("lantern");
    let ember = folder.project("ember");
    let root = PathBuf::from(&ember.path);
    std::fs::File::create(root.join("project.json"))
        .and_then(|file| file.set_len((256 << 10) + 1))
        .expect("a project.json over the strict cap");
    let mut app = hub(&folder, folder.store());
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Trusted);
    app.launch_project(&lantern);
    assert!(
        tried_to_launch(&app),
        "the premise: lantern's launch said so"
    );

    app.launch_existing_project(&root);

    assert!(app.trust_prompt.is_some(), "never decided of, ember waits");
    let banner = app.banner.as_ref().expect("the refusal is shown");
    assert!(
        !banner.message.contains("Failed to launch editor"),
        "ember's refusal is shown with lantern's launch: {}",
        banner.message
    );
}

/// **A refusal stays listed with its project until it is dismissed.**
///
/// Another launch's banner takes the place of ember's refused `project.json`,
/// which stays listed with its project; ember launched again shows it once,
/// and dismissing that banner dismisses it.
#[test]
fn a_refusal_stays_listed_with_its_project_until_dismissed() {
    let folder = Folder::new("refusal-listed");
    let lantern = folder.project("lantern");
    let ember = folder.project("ember");
    let root = PathBuf::from(&ember.path);
    std::fs::File::create(root.join("project.json"))
        .and_then(|file| file.set_len((256 << 10) + 1))
        .expect("a project.json over the strict cap");
    let mut app = hub(&folder, folder.store());
    app.launch_existing_project(&root);
    app.trust_prompt = None;
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Trusted);

    app.launch_project(&lantern);

    let banner = app.banner.as_ref().expect("lantern's launch said so");
    assert!(
        !banner.message.contains("project.json is not read"),
        "{}",
        banner.message
    );
    let listed: Vec<&str> = app
        .home
        .refusals
        .of(&root)
        .map(|refusal| refusal.name.as_str())
        .collect();
    assert_eq!(
        listed,
        ["ember"],
        "the refusal stays listed, with its project"
    );

    app.launch_existing_project(&root);
    let banner = app.banner.as_ref().expect("ember's refusal is shown");
    assert_eq!(
        banner.message.matches("project.json is not read").count(),
        1,
        "{}",
        banner.message
    );

    app.dismiss_banner();

    assert!(app.banner.is_none());
    assert_eq!(
        app.home.refusals.iter().count(),
        0,
        "dismissed with its banner"
    );
}
