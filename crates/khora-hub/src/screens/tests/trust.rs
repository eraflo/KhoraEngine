// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! **The hub asks about a project it never decided of, and lets the user
//! change their mind.** The home screen paints the trust prompt over its
//! area while a project waits (`HubApp::trust_prompt`), and its answer is
//! `HubApp::answer_trust`; a card decided restricted wears a `Restricted`
//! chip; a card's `More` menu offers to trust a restricted project — after a
//! confirmation — or to restrict a trusted one, at once.
//!
//! No test reads or writes the user's home folder: the store is a file under
//! the temp folder, the configuration is built in memory and never saved,
//! and the engine's editor binary does not exist — a launch is seen trying
//! it and failing, so no process is ever spawned.

use std::path::PathBuf;

use khora_sdk::trust::{ProjectTrust, TrustStore};
use khora_tool_ui::testing::RecordingUiBuilder;

use crate::app::HubApp;
use crate::screens::show_home;
use crate::services::config::{EngineInstall, HubConfig, RecentProject};
use crate::state::{EngineManagerState, HomeState, NewProjectState, Screen, SettingsState};

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-hub-trust-screen-{name}-{}-{}",
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

/// A hub built in memory over the trust store in `folder`, listing
/// `projects`, knowing one engine whose editor binary does not exist: a
/// launch tries it, and fails.
fn hub(folder: &Folder, projects: &[&RecentProject]) -> HubApp {
    HubApp {
        config: HubConfig {
            recent_projects: projects.iter().map(|&project| project.clone()).collect(),
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
        trust_store: TrustStore::new(Some(folder.store())),
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

/// What the store holds of `project`.
fn decision(hub: &HubApp, project: &RecentProject) -> Option<ProjectTrust> {
    hub.trust_store.decision(&PathBuf::from(&project.path))
}

/// One frame of the home screen over a 1100 × 720 window, `ui` scripted.
fn home(app: &mut HubApp, ui: RecordingUiBuilder) -> RecordingUiBuilder {
    let mut ui = ui;
    show_home(app, &mut ui);
    ui
}

fn window() -> RecordingUiBuilder {
    RecordingUiBuilder::new(1100.0, 720.0)
}

const TITLE: &str = "Do you trust \u{201c}ember\u{201d}?";

// ─── The prompt ─────────────────────────────────────────────────────────────

/// **The home screen asks about the project waiting**: its title names the
/// project, and it offers `Cancel`, `Open restricted` and `Trust and open`.
/// With no project waiting, nothing is asked.
#[test]
fn the_home_screen_asks_about_the_project_waiting_for_trust() {
    let folder = Folder::new("prompt");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);

    let quiet = home(&mut app, window());
    assert!(
        quiet.painted_text("ember") && !quiet.painted_text("Trust and open"),
        "the premise: the card is painted, nothing is asked: {:?}",
        quiet.texts()
    );

    app.trust_prompt = Some(ember.clone());
    let ui = home(&mut app, window());
    for painted in [TITLE, "Cancel", "Open restricted", "Trust and open"] {
        assert!(
            ui.painted_text(painted),
            "`{painted}` is painted: {:?}",
            ui.texts()
        );
    }
}

/// **`Trust and open` trusts the project and launches it**, and the prompt
/// closes.
#[test]
fn trust_and_open_trusts_the_project_and_launches_it() {
    let folder = Folder::new("trust-and-open");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);
    app.trust_prompt = Some(ember.clone());

    home(&mut app, window().with_click("hp-trust-choice-2"));

    assert_eq!(decision(&app, &ember), Some(ProjectTrust::Trusted));
    assert!(tried_to_launch(&app), "the project is launched");
    assert!(app.trust_prompt.is_none(), "the prompt is closed");
}

/// **`Open restricted` restricts the project and launches it.**
#[test]
fn open_restricted_restricts_the_project_and_launches_it() {
    let folder = Folder::new("open-restricted");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);
    app.trust_prompt = Some(ember.clone());

    home(&mut app, window().with_click("hp-trust-choice-1"));

    assert_eq!(decision(&app, &ember), Some(ProjectTrust::Restricted));
    assert!(tried_to_launch(&app), "the project is launched");
    assert!(app.trust_prompt.is_none(), "the prompt is closed");
}

/// **`Cancel` lets the project go**: nothing is decided, nothing launched,
/// the prompt closes.
#[test]
fn cancel_lets_the_project_go() {
    let folder = Folder::new("cancel");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);
    app.trust_prompt = Some(ember.clone());

    home(&mut app, window().with_click("hp-trust-choice-0"));

    assert!(app.trust_prompt.is_none(), "the prompt is closed");
    assert_eq!(decision(&app, &ember), None, "nothing is decided");
    assert!(!tried_to_launch(&app), "nothing is launched");
}

/// **An answer decides and launches; no answer lets go** — `answer_trust`
/// itself, as the prompt calls it.
#[test]
fn answering_trust_decides_and_launches_or_lets_go() {
    let folder = Folder::new("answer");
    let ember = folder.project("ember");
    let lantern = folder.project("lantern");
    let mut app = hub(&folder, &[&ember, &lantern]);

    app.trust_prompt = Some(ember.clone());
    app.answer_trust(Some(ProjectTrust::Trusted));
    assert_eq!(decision(&app, &ember), Some(ProjectTrust::Trusted));
    assert!(tried_to_launch(&app), "a decision launches the project");
    assert!(app.trust_prompt.is_none());

    app.banner = None;
    app.trust_prompt = Some(lantern.clone());
    app.answer_trust(None);
    assert!(app.trust_prompt.is_none(), "no answer closes the prompt");
    assert_eq!(decision(&app, &lantern), None, "and decides nothing");
    assert!(!tried_to_launch(&app), "and launches nothing");
}

// ─── The cards ──────────────────────────────────────────────────────────────

/// **A card decided restricted wears a `Restricted` chip**, in the warning
/// tone; a trusted card, and one never decided of, wear none.
#[test]
fn a_restricted_projects_card_wears_a_restricted_chip() {
    let folder = Folder::new("chip");
    let lantern = folder.project("lantern");
    let ember = folder.project("ember");
    let moss = folder.project("moss");
    let mut app = hub(&folder, &[&lantern, &ember, &moss]);
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Restricted);
    app.trust_store
        .decide(&PathBuf::from(&ember.path), ProjectTrust::Trusted);

    let ui = home(&mut app, window());

    let chips = ui
        .texts()
        .into_iter()
        .filter(|text| *text == "Restricted")
        .count();
    assert_eq!(
        chips,
        1,
        "one chip, the restricted card's: {:?}",
        ui.texts()
    );
    assert_eq!(
        ui.text_color("Restricted"),
        Some(crate::ui::theme().warning),
        "in the warning tone"
    );
}

/// The `More` menu of the first card, opened: the pointer on the card, a
/// click on its `More` button.
fn more_menu_of_first_card(app: &mut HubApp) -> RecordingUiBuilder {
    home(
        app,
        window().with_hover("hp-card-0").with_click("hp-more-0"),
    )
}

/// **A restricted project's `More` menu offers to trust it**, and to show
/// its folder; not to restrict it.
#[test]
fn the_more_menu_offers_to_trust_a_restricted_project() {
    let folder = Folder::new("more-restricted");
    let lantern = folder.project("lantern");
    let mut app = hub(&folder, &[&lantern]);
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Restricted);

    let ui = more_menu_of_first_card(&mut app);

    assert!(
        ui.painted_text("Trust this project\u{2026}"),
        "it offers to trust it: {:?}",
        ui.texts()
    );
    assert!(ui.painted_text("Show in folder"), "{:?}", ui.texts());
    assert!(!ui.painted_text("Restrict this project"));
}

/// **A trusted project's `More` menu offers to restrict it**, and to show
/// its folder; not to trust it.
#[test]
fn the_more_menu_offers_to_restrict_a_trusted_project() {
    let folder = Folder::new("more-trusted");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);
    app.trust_store
        .decide(&PathBuf::from(&ember.path), ProjectTrust::Trusted);

    let ui = more_menu_of_first_card(&mut app);

    assert!(
        ui.painted_text("Restrict this project"),
        "it offers to restrict it: {:?}",
        ui.texts()
    );
    assert!(ui.painted_text("Show in folder"), "{:?}", ui.texts());
    assert!(!ui.painted_text("Trust this project"));
}

/// **Restricting a project is decided at once**, written to the store, and
/// a banner says so; nothing is launched.
#[test]
fn restricting_a_project_is_decided_at_once_and_said() {
    let folder = Folder::new("restrict");
    let ember = folder.project("ember");
    let mut app = hub(&folder, &[&ember]);
    app.trust_store
        .decide(&PathBuf::from(&ember.path), ProjectTrust::Trusted);

    app.restrict_project(&ember);

    assert_eq!(decision(&app, &ember), Some(ProjectTrust::Restricted));
    assert_eq!(
        TrustStore::new(Some(folder.store())).decision(&PathBuf::from(&ember.path)),
        Some(ProjectTrust::Restricted),
        "written to the store"
    );
    let banner = app.banner.as_ref().expect("a banner says so");
    assert!(!banner.is_error, "news, not an error: {}", banner.message);
    assert!(
        banner.message.contains("ember"),
        "it names the project: {}",
        banner.message
    );
    assert!(!tried_to_launch(&app), "nothing is launched");
}

/// **The confirmation trusts a restricted project**: open for it, it offers
/// `Keep restricted` and `Trust`; `Trust` decides it trusted and closes;
/// `Keep restricted` closes and decides nothing. Neither launches it.
#[test]
fn the_trust_confirmation_trusts_a_restricted_project() {
    let folder = Folder::new("confirm");
    let lantern = folder.project("lantern");
    let mut app = hub(&folder, &[&lantern]);
    app.trust_store
        .decide(&PathBuf::from(&lantern.path), ProjectTrust::Restricted);
    app.home.trust_confirm = Some(lantern.clone());

    let ui = home(&mut app, window().with_click("hp-trust-confirm-choice-0"));
    for painted in ["Keep restricted", "Trust"] {
        assert!(
            ui.painted_text(painted),
            "`{painted}` is painted: {:?}",
            ui.texts()
        );
    }
    assert!(app.home.trust_confirm.is_none(), "kept restricted, closed");
    assert_eq!(decision(&app, &lantern), Some(ProjectTrust::Restricted));

    app.home.trust_confirm = Some(lantern.clone());
    home(&mut app, window().with_click("hp-trust-confirm-choice-1"));
    assert!(app.home.trust_confirm.is_none(), "trusted, closed");
    assert_eq!(decision(&app, &lantern), Some(ProjectTrust::Trusted));
    assert!(!tried_to_launch(&app), "nothing is launched");
}
