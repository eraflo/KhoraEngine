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

//! **A project opens at the trust the user decided of it.** The editor opens
//! a project one way — the `--project` of the command line, a folder picked
//! by File > Open Project — and that way reads what the trust store holds of
//! it: a project never decided of is held, nothing of it filed, until the
//! user decides; one decided of opens at that trust. Trusting a project
//! opened restricted lifts its restricted mode.
//!
//! Every folder here is under the temp folder: no test reads or writes the
//! user's home folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::EditorState;
use khora_sdk::khora_core::asset::IndexRequest;
use khora_sdk::khora_core::event::Channel;
use khora_sdk::trust::{
    Layer, ProjectTrust, ProjectTrustState, QuarantineKey, Refusals, SecurityReason,
    SecurityRefusal, Trust, TrustStore,
};
use khora_sdk::{IndexKind, ReadinessHold};

use crate::app::project_open::{grant_trust, open_project, start_opening_project};
use crate::tests::project_index::IndexingEngine;

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-trust-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A project named `name` under `folder`, with its `project.json` and an
/// `assets/` folder.
fn project_in(folder: &Folder, name: &str) -> PathBuf {
    let root = folder.0.join(name);
    std::fs::create_dir_all(root.join("assets")).expect("the project folder");
    std::fs::write(
        root.join("project.json"),
        format!(r#"{{ "name": "{name}", "engine_version": "0.9.1" }}"#),
    )
    .expect("the project's `project.json`");
    root
}

/// The trust store's file under `folder`, as `~/.khora/trust.ron` would be.
fn store_in(folder: &Folder) -> PathBuf {
    folder.0.join("khora").join("trust.ron")
}

/// What the editor opens a project on: the engine's asset service and index
/// hub, the run's refusals, the trust of the project open (`Trusted` until
/// an open sets it); the editor's state and hold.
struct Editor {
    engine: IndexingEngine,
    refusals: Arc<Refusals>,
    trust: Arc<ProjectTrustState>,
    state: Arc<Mutex<EditorState>>,
    hold: Option<ReadinessHold>,
}

impl Editor {
    fn new() -> Self {
        let mut engine = IndexingEngine::new();
        let refusals = Arc::new(Refusals::new(None, None));
        engine.runtime.resources.insert(Arc::clone(&refusals));
        let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Trusted));
        engine.runtime.resources.insert(Arc::clone(&trust));
        // What the film waits on: an open takes a hold on it.
        engine
            .runtime
            .resources
            .insert(Arc::new(khora_sdk::Readiness::new()));
        Self {
            engine,
            refusals,
            trust,
            state: Arc::new(Mutex::new(EditorState::default())),
            hold: None,
        }
    }

    /// The project at `root` opened as the editor opens one, `store` read.
    fn open(&mut self, root: &Path, store: &TrustStore) -> bool {
        open_project(
            root.to_path_buf(),
            store,
            &self.engine.runtime,
            &self.state,
            &mut self.hold,
        )
        .is_some()
    }

    /// The index requests filed so far, taken off the family's channel.
    fn filed(&self) -> Vec<IndexRequest> {
        self.engine
            .runtime
            .resources
            .get::<Channel<IndexRequest>>()
            .cloned()
            .expect("the runtime holds the index requests")
            .drain()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, EditorState> {
        self.state.lock().expect("the editor state")
    }
}

/// Whether `requests` holds an `Open` of the project at `root`, however its
/// folder is spelled.
fn opens(requests: &[IndexRequest], root: &Path) -> bool {
    let root = std::fs::canonicalize(root).expect("the project exists");
    requests.iter().any(|request| {
        request.kind == IndexKind::Open
            && std::fs::canonicalize(&request.project_root).ok().as_ref() == Some(&root)
    })
}

/// **An unknown project asks for trust once.** Opened with nothing decided
/// of it, the project is held: nothing is filed, nothing of it is shown as
/// open, no hold is taken on the film, and the editor's state names it as
/// waiting for the user's decision. Once decided — here, restricted — the
/// same open opens it: its `Open` is filed, the trust of the project open is
/// the one decided, and nothing waits for a decision any more. A second
/// editor over the same store opens it at once.
#[test]
fn an_unknown_project_asks_for_trust_once() {
    let folder = Folder::new("asks-once");
    let root = project_in(&folder, "ember");
    let store = TrustStore::new(Some(store_in(&folder)));
    let mut editor = Editor::new();

    assert!(!editor.open(&root, &store), "the open is held");

    assert!(editor.filed().is_empty(), "nothing is filed");
    assert!(editor.hold.is_none(), "no hold is taken on the film");
    {
        let state = editor.state();
        assert_eq!(
            state.trust_prompt.as_deref(),
            Some(root.as_path()),
            "the editor's state names the project waiting for a decision"
        );
        assert_eq!(state.project_folder, None, "nothing of it is shown as open");
    }

    store.decide(&root, ProjectTrust::Restricted);
    assert!(editor.open(&root, &store), "decided, the project opens");

    assert!(opens(&editor.filed(), &root), "its `Open` is filed");
    assert_eq!(
        editor.trust.get(),
        ProjectTrust::Restricted,
        "the project open is at the trust decided"
    );
    assert_eq!(
        editor.state().trust_prompt,
        None,
        "nothing waits for a decision any more"
    );

    let mut second = Editor::new();
    assert!(
        second.open(&root, &TrustStore::new(Some(store_in(&folder)))),
        "the decision is remembered: a second editor does not ask again"
    );
    assert_eq!(second.state().trust_prompt, None);
}

/// **The open waits for a decision, and opens at the one given**: with no
/// decision, `start_opening_project` files nothing and names the project as
/// waiting; with one, it files the project's `Open` and sets the trust of
/// the project open to it — restricted, then trusted.
#[test]
fn a_project_opens_at_the_trust_decided() {
    let folder = Folder::new("opens-at-trust");
    let root = project_in(&folder, "lantern");
    let mut editor = Editor::new();

    let held = start_opening_project(
        root.clone(),
        None,
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(held.is_none(), "no decision: the open is held");
    assert!(editor.filed().is_empty(), "nothing is filed");
    assert_eq!(editor.state().trust_prompt.as_deref(), Some(root.as_path()));

    let restricted = start_opening_project(
        root.clone(),
        Some(ProjectTrust::Restricted),
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(restricted.is_some(), "decided: the project opens");
    assert!(opens(&editor.filed(), &root), "its `Open` is filed");
    assert_eq!(editor.trust.get(), ProjectTrust::Restricted);
    assert_eq!(editor.state().trust_prompt, None);

    let trusted = start_opening_project(
        root.clone(),
        Some(ProjectTrust::Trusted),
        &editor.engine.runtime,
        &editor.state,
        &mut editor.hold,
    );
    assert!(trusted.is_some());
    assert_eq!(editor.trust.get(), ProjectTrust::Trusted);
}

/// **File > Open Project goes through the same gate**: a second project
/// picked while one is open, never decided of, is held and named as waiting;
/// the project open stays as it was, at its trust.
#[test]
fn a_project_picked_by_file_open_asks_for_trust_too() {
    let folder = Folder::new("file-open");
    let first = project_in(&folder, "first");
    let picked = project_in(&folder, "picked");
    let store = TrustStore::new(Some(store_in(&folder)));
    store.decide(&first, ProjectTrust::Trusted);
    let mut editor = Editor::new();
    assert!(editor.open(&first, &store), "the first project opens");
    editor.filed();

    assert!(!editor.open(&picked, &store), "the picked project is held");

    assert!(editor.filed().is_empty(), "nothing of it is filed");
    let state = editor.state();
    assert_eq!(state.trust_prompt.as_deref(), Some(picked.as_path()));
    assert_eq!(
        state.project_folder.as_deref().map(PathBuf::from),
        Some(first.clone()),
        "the project open stays shown"
    );
    drop(state);
    assert_eq!(editor.trust.get(), ProjectTrust::Trusted);
}

/// **Granting trust lifts the restricted mode.** A project opened
/// restricted, two of its files refused at the strict trust — one too large,
/// one holding what its type does not — is trusted: the store says so, the
/// trust of the project open is `Trusted`, the file refused for its size is
/// released while the one refused for its content stays, and the project's
/// index is filed anew, so every reader is built at a trusted project's
/// trust.
#[test]
fn granting_trust_lifts_the_restricted_mode() {
    let folder = Folder::new("grant");
    let root = project_in(&folder, "ember");
    let store_path = store_in(&folder);
    let store = TrustStore::new(Some(store_path.clone()));
    store.decide(&root, ProjectTrust::Restricted);
    let mut editor = Editor::new();
    assert!(
        editor.open(&root, &store),
        "the premise: it opens restricted"
    );
    assert_eq!(editor.trust.get(), ProjectTrust::Restricted);
    editor.filed();

    let too_large = SecurityRefusal {
        path: root.join("assets").join("huge.png"),
        trust: Trust::Restricted,
        layer: Layer::File,
        reason: SecurityReason::TooLarge {
            size: 80 << 20,
            cap: 64 << 20,
        },
    };
    let wrong_type = SecurityRefusal {
        path: root.join("assets").join("liar.png"),
        trust: Trust::Restricted,
        layer: Layer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };
    editor.refusals.refuse(
        too_large.clone(),
        Some(QuarantineKey::Stamp {
            len: 80 << 20,
            modified: std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000),
        }),
    );
    editor
        .refusals
        .refuse(wrong_type.clone(), Some(QuarantineKey::Content([3; 32])));

    grant_trust(&editor.engine.runtime, &store, &root);

    assert_eq!(store.decision(&root), Some(ProjectTrust::Trusted));
    assert_eq!(
        TrustStore::new(Some(store_path)).decision(&root),
        Some(ProjectTrust::Trusted),
        "the decision is written to the store"
    );
    assert_eq!(
        editor.trust.get(),
        ProjectTrust::Trusted,
        "the project open is trusted"
    );
    assert_eq!(
        editor.refusals.quarantined(),
        vec![wrong_type],
        "the file refused for its size is released, the one refused for its content stays"
    );
    assert!(
        opens(&editor.filed(), &root),
        "the project's index is filed anew"
    );
}

/// **Trusting a project lifts its own restricted refusals, not another
/// project's.** The quarantine is the user's, shared by every project they
/// open: a file of another project, never trusted, refused over a restricted
/// limit, stays quarantined when this one is trusted — that other project
/// is still read at the strict limits, and its file was never cleared.
#[test]
fn trusting_a_project_keeps_another_projects_restricted_refusals() {
    let folder = Folder::new("grant-own");
    let root = project_in(&folder, "ember");
    let other = project_in(&folder, "other");
    let store = TrustStore::new(Some(store_in(&folder)));
    store.decide(&root, ProjectTrust::Restricted);
    store.decide(&other, ProjectTrust::Restricted);
    let mut editor = Editor::new();
    assert!(
        editor.open(&root, &store),
        "the premise: it opens restricted"
    );
    editor.filed();

    let others = SecurityRefusal {
        path: other.join("assets").join("wide.png"),
        trust: Trust::Restricted,
        layer: Layer::Decoder,
        reason: SecurityReason::OverLimit {
            what: "image width of 16384 pixels".to_owned(),
            limit: 8192,
        },
    };
    editor
        .refusals
        .refuse(others.clone(), Some(QuarantineKey::Content([7; 32])));

    grant_trust(&editor.engine.runtime, &store, &root);

    assert_eq!(
        store.decision(&other),
        Some(ProjectTrust::Restricted),
        "the premise: the other project is still restricted"
    );
    assert_eq!(
        editor.refusals.quarantined(),
        vec![others],
        "the other project's file stays quarantined"
    );
}
