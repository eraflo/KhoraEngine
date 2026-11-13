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

//! **The editor shows the trust of the project open, as the engine holds
//! it.** Each frame the app mirrors the runtime's `ProjectTrustState` into
//! its state, shows the restricted bar while the project is restricted,
//! fills the trust confirmation with what trusting the project changes,
//! mirrors the run's quarantine and releases what the user releases.
//! Trusting the project clears what its restricted mode listed.
//!
//! The editor runs over a shell that records the panels it holds; every
//! folder is under the temp folder — no test reads or writes the user's home
//! folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use khora_sdk::editor_ui::{
    EditorPanel, EditorShell, EditorState, PanelLocation, QuarantinedAsset, StatusBarData,
    TrustSummary,
};
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_sdk::khora_core::ui::UiTheme;
use khora_sdk::trust::{
    ProjectTrust, ProjectTrustState, QuarantineKey, Refusals, SecurityReason, SecurityRefusal,
    Trust, TrustLayer, TrustStore,
};
use khora_sdk::EngineApp;

use crate::app::project_open::grant_trust;
use crate::app::EditorApp;
use crate::tests::project_index::IndexingEngine;

// ─── A shell that records its panels ────────────────────────────────────────

/// The panels a [`RecordingShell`] holds: id and place, in registration
/// order.
type Held = Arc<Mutex<Vec<(String, PanelLocation)>>>;

/// A shell that holds no window: it records the panels registered with it,
/// and forgets those removed.
struct RecordingShell(Held);

impl EditorShell for RecordingShell {
    fn register_panel(&mut self, location: PanelLocation, panel: Box<dyn EditorPanel>) {
        self.0
            .lock()
            .expect("the panels")
            .push((panel.id().to_owned(), location));
    }

    fn remove_panel(&mut self, id: &str) -> bool {
        let mut held = self.0.lock().expect("the panels");
        let before = held.len();
        held.retain(|(held_id, _)| held_id != id);
        held.len() != before
    }

    fn set_theme(&mut self, _theme: UiTheme) {}

    fn set_status(&mut self, _data: StatusBarData) {}

    fn set_editor_state(&mut self, _state: Arc<Mutex<EditorState>>) {}

    fn show_frame(&mut self) {}
}

// ─── A project on disk ──────────────────────────────────────────────────────

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-trust-view-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }

    /// Writes `bytes` at `rel` under this folder; returns its path.
    fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the file's folder");
        std::fs::write(&path, bytes).expect("the file");
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A project `cave-demo` in `folder`: four scripts under `assets/scripts`
/// (one in a subfolder), a file beside them that is no script, a script
/// misplaced outside them, an overlay of `khora::lighting`, a `Cargo.toml`.
fn cave_demo(folder: &Folder) -> PathBuf {
    let root = folder.0.join("cave-demo");
    for rel in [
        "assets/scripts/player.erg",
        "assets/scripts/door.erg",
        "assets/scripts/torch.erg",
        "assets/scripts/ai/bat.erg",
    ] {
        std::fs::create_dir_all(root.join(rel).parent().expect("a parent"))
            .expect("the scripts folder");
        std::fs::write(root.join(rel), b"behavior B { int n = 0; }").expect("a script");
    }
    std::fs::write(root.join("assets/scripts/notes.txt"), b"not a script").expect("a note");
    std::fs::write(root.join("assets/misplaced.erg"), b"behavior M { }").expect("a stray");
    std::fs::create_dir_all(root.join("assets/shaders")).expect("the shaders folder");
    std::fs::write(root.join("assets/shaders/lighting.wgsl"), b"// lighting").expect("an overlay");
    std::fs::write(
        root.join("Cargo.toml"),
        b"[package]\nname = \"cave-demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("a manifest");
    std::fs::write(
        root.join("project.json"),
        br#"{ "name": "cave-demo", "engine_version": "0.9.1" }"#,
    )
    .expect("the project's `project.json`");
    root
}

fn too_large(path: PathBuf, trust: Trust) -> SecurityRefusal {
    SecurityRefusal {
        path,
        trust,
        layer: TrustLayer::File,
        reason: SecurityReason::TooLarge {
            size: 80 << 20,
            cap: 64 << 20,
        },
    }
}

fn over_limit(path: PathBuf) -> SecurityRefusal {
    SecurityRefusal {
        path,
        trust: Trust::Restricted,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::OverLimit {
            what: "image width of 16384 pixels".to_owned(),
            limit: 8192,
        },
    }
}

fn wrong_type(path: PathBuf) -> SecurityRefusal {
    SecurityRefusal {
        path,
        trust: Trust::Project,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    }
}

fn stamp() -> QuarantineKey {
    QuarantineKey::Stamp {
        len: 80 << 20,
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    }
}

// ─── The editor over an engine ──────────────────────────────────────────────

/// The editor app over an engine holding the trust of the project open, the
/// problems, the run's refusals and a recording shell, its panels
/// registered.
struct Editor {
    app: EditorApp,
    engine: IndexingEngine,
    trust: Arc<ProjectTrustState>,
    problems: Arc<Problems>,
    refusals: Arc<Refusals>,
    panels: Held,
    _store: Folder,
}

impl Editor {
    fn new(trust: ProjectTrust) -> Self {
        let mut engine = IndexingEngine::new();
        let trust = Arc::new(ProjectTrustState::new(trust));
        engine.runtime.resources.insert(Arc::clone(&trust));
        let problems = Arc::new(Problems::default());
        engine.runtime.resources.insert(Arc::clone(&problems));
        let refusals = Arc::new(Refusals::new(Some(Arc::clone(&problems)), None));
        engine.runtime.resources.insert(Arc::clone(&refusals));
        let panels: Held = Arc::default();
        let shell: Box<dyn EditorShell> = Box::new(RecordingShell(Arc::clone(&panels)));
        engine.runtime.backends.insert(Arc::new(Mutex::new(shell)));

        let store = Folder::new("store");
        let mut app = <EditorApp as EngineApp>::new();
        // Never the user's own store.
        app.trust_store = Some(TrustStore::new(Some(store.0.join("trust.ron"))));
        app.cache_services(&engine.runtime);
        app.register_panels(&engine.runtime);
        Self {
            app,
            engine,
            trust,
            problems,
            refusals,
            panels,
            _store: store,
        }
    }

    /// One frame of the editor's view of the project's trust.
    fn frame(&mut self) {
        self.app.follow_project_trust(&self.engine.runtime);
    }

    fn state(&self) -> std::sync::MutexGuard<'_, EditorState> {
        self.app.editor_state.lock().expect("the editor state")
    }

    /// The panels the shell holds with `id`, by place.
    fn held(&self, id: &str) -> Vec<PanelLocation> {
        self.panels
            .lock()
            .expect("the panels")
            .iter()
            .filter(|(held, _)| held == id)
            .map(|(_, location)| *location)
            .collect()
    }

    /// The project at `root` shown as open.
    fn open(&self, root: &Path) {
        self.state().project_folder = Some(root.to_string_lossy().into_owned());
    }
}

const TRUST_PANEL: &str = "khora.editor.trust";
const RESTRICTED_BAR: &str = "khora.editor.restricted_bar";

/// **The editor's state says whether the project open is restricted**, as
/// the engine holds it, frame by frame.
#[test]
fn the_app_mirrors_the_trust_of_the_project_open() {
    let mut editor = Editor::new(ProjectTrust::Restricted);

    editor.frame();
    assert!(
        editor.state().restricted,
        "restricted, as the engine holds it"
    );

    editor.trust.set(ProjectTrust::Trusted);
    editor.frame();
    assert!(!editor.state().restricted, "trusted once the engine is");
}

/// **The trust panel is always there**: registered with the panels, a
/// floating overlay, whatever the trust — it paints only when a question is
/// open.
#[test]
fn the_trust_panel_is_always_registered() {
    let editor = Editor::new(ProjectTrust::Trusted);

    let held = editor.held(TRUST_PANEL);
    assert_eq!(held.len(), 1, "registered once: {held:?}");
    assert!(
        matches!(held[0], PanelLocation::Floating(_)),
        "a floating overlay: {held:?}"
    );
}

/// **The restricted bar shows while the project is restricted**: registered
/// as a top strip once the project is restricted — once, however many
/// frames — and removed once it is trusted.
#[test]
fn the_restricted_bar_shows_while_the_project_is_restricted() {
    let mut editor = Editor::new(ProjectTrust::Trusted);
    editor.frame();
    assert!(
        editor.held(RESTRICTED_BAR).is_empty(),
        "no bar for a trusted project"
    );

    editor.trust.set(ProjectTrust::Restricted);
    editor.frame();
    editor.frame();
    assert_eq!(
        editor.held(RESTRICTED_BAR),
        vec![PanelLocation::TopBar],
        "one bar, at the top, while restricted"
    );

    editor.trust.set(ProjectTrust::Trusted);
    editor.frame();
    assert!(
        editor.held(RESTRICTED_BAR).is_empty(),
        "the bar goes once the project is trusted"
    );
}

/// **`Trust this project…` opens the confirmation with what trusting
/// changes**, read from the project's folder: its 4 scripts under
/// `assets/scripts` (not the note beside them, not the script misplaced
/// outside them), its overlay of `khora::lighting`, its `Cargo.toml`, and
/// its 2 files refused at the restricted limits for their size or a limit —
/// not a file refused for its content, not another project's.
#[test]
fn trust_this_project_opens_the_confirmation_with_what_trusting_changes() {
    let folder = Folder::new("confirm");
    let root = cave_demo(&folder);
    let other = folder.0.join("other");
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    editor.refusals.refuse(
        too_large(
            folder.write("cave-demo/assets/textures/huge.png", b"png"),
            Trust::Restricted,
        ),
        Some(stamp()),
    );
    editor.refusals.refuse(
        over_limit(folder.write("cave-demo/assets/textures/wide.png", b"png")),
        Some(QuarantineKey::Content([4; 32])),
    );
    editor.refusals.refuse(
        wrong_type(folder.write("cave-demo/assets/liar.png", b"text")),
        Some(QuarantineKey::Content([5; 32])),
    );
    editor.refusals.refuse(
        too_large(
            folder.write("other/assets/huge.png", b"png"),
            Trust::Restricted,
        ),
        Some(stamp()),
    );
    assert!(
        other.exists(),
        "the premise: another project's file is refused"
    );
    editor.state().pending_menu_action = Some("confirm_trust".to_owned());

    editor.frame();

    let state = editor.state();
    assert_eq!(
        state.trust_confirm,
        Some(TrustSummary {
            scripts: 4,
            overlays: vec!["khora::lighting".to_owned()],
            builds_code: true,
            refused_for_size: 2,
        }),
        "what trusting cave-demo changes"
    );
    assert_eq!(state.pending_menu_action, None, "the action is taken");
}

/// **The quarantine is mirrored, each file by its path under the assets**,
/// with what the asset browser shows of it: the reason and the remedy, what
/// refused it, the trust it was read at, what holds it.
#[test]
fn the_quarantine_is_mirrored_with_paths_under_the_assets() {
    let folder = Folder::new("mirror");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    let huge = too_large(
        folder.write("cave-demo/assets/textures/huge.png", b"png"),
        Trust::Restricted,
    );
    let liar = wrong_type(folder.write("cave-demo/assets/liar.png", b"text"));
    editor.refusals.refuse(huge.clone(), Some(stamp()));
    editor
        .refusals
        .refuse(liar.clone(), Some(QuarantineKey::Content([0xab; 32])));

    editor.frame();

    let mut quarantined = editor.state().quarantined.clone();
    quarantined.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    assert_eq!(quarantined.len(), 2, "both files: {quarantined:?}");
    let (liar_shown, huge_shown) = (&quarantined[0], &quarantined[1]);

    assert_eq!(
        huge_shown,
        &QuarantinedAsset {
            rel_path: "textures/huge.png".to_owned(),
            reason: huge.reason.to_string(),
            remedy: huge.reason.remedy().to_owned(),
            refused_by: "file guard".to_owned(),
            read_as: "restricted project".to_owned(),
            content: "size and date".to_owned(),
        }
    );
    assert_eq!(liar_shown.rel_path, "liar.png");
    assert_eq!(liar_shown.reason, liar.reason.to_string());
    assert_eq!(liar_shown.remedy, liar.reason.remedy());
    assert_eq!(liar_shown.refused_by, "type check");
    assert_eq!(liar_shown.read_as, "trusted project");
    assert!(
        liar_shown.content.starts_with("blake3 ")
            && liar_shown.content.contains("abab\u{2026}abab"),
        "held by its content's hash, shortened: {:?}",
        liar_shown.content
    );
}

/// **A file released is freed from the quarantine**: the release the asset
/// browser asked is taken, and the run's refusals no longer hold the file.
#[test]
fn a_released_file_leaves_the_quarantine() {
    let folder = Folder::new("release");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    let huge = too_large(
        folder.write("cave-demo/assets/textures/huge.png", b"png"),
        Trust::Restricted,
    );
    editor.refusals.refuse(huge, Some(stamp()));
    editor.state().pending_release = Some("textures/huge.png".to_owned());

    editor.frame();

    assert!(
        editor.refusals.quarantined().is_empty(),
        "the file is released: {:?}",
        editor.refusals.quarantined()
    );
    assert_eq!(editor.state().pending_release, None, "the release is taken");
}

/// **Trusting the project clears what its restricted mode listed**: the
/// problems under `ProblemSource::Restricted` — an overlay ignored — are
/// gone once it is trusted.
#[test]
fn granting_trust_clears_what_the_restricted_mode_listed() {
    let folder = Folder::new("grant-clears");
    let root = cave_demo(&folder);
    let editor = Editor::new(ProjectTrust::Restricted);
    editor.problems.report(
        ProblemSource::Restricted,
        vec![Problem {
            severity: ProblemSeverity::Warning,
            module: Some("shaders/lighting.wgsl".to_owned()),
            line: 0,
            column: 0,
            message: "Shader overlay ignored".to_owned(),
            note: Some("Trust the project to use it.".to_owned()),
            entity: None,
            behavior: None,
            component: None,
            field: None,
        }],
    );
    let store = TrustStore::new(Some(folder.0.join("trust.ron")));
    store.decide(&root, ProjectTrust::Restricted);

    grant_trust(&editor.engine.runtime, &store, &root);

    assert!(
        editor
            .problems
            .of_source(&ProblemSource::Restricted)
            .is_empty(),
        "nothing restricted is listed any more: {:?}",
        editor.problems.of_source(&ProblemSource::Restricted)
    );
}

/// Every uuid the asset services the editor reaches announced as changed:
/// the engine's (the runtime's service) and the editor's own.
fn announced(editor: &Editor) -> Vec<khora_sdk::khora_core::asset::AssetUUID> {
    let mut announced = Vec::new();
    for service in [&editor.engine.shared, &editor.app.shared_assets] {
        let service = service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        announced.extend(service.changes().drain().into_iter().map(|change| change.0));
    }
    announced
}

/// **A file released is read again**: releasing it from the quarantine
/// invalidates its asset, so the asset service forgets the load the
/// quarantine failed and announces the asset changed — the next load reads
/// the file, rather than the failure being remembered until something else
/// touches it.
#[test]
fn a_released_file_is_read_again() {
    let folder = Folder::new("release-reads");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    editor.refusals.refuse(
        too_large(
            folder.write("cave-demo/assets/textures/huge.png", b"png"),
            Trust::Restricted,
        ),
        Some(stamp()),
    );
    let _ = announced(&editor);
    editor.state().pending_release = Some("textures/huge.png".to_owned());

    editor.frame();

    let huge = khora_sdk::khora_core::asset::AssetUUID::new_v5(
        &khora_sdk::khora_core::asset::asset_key(Path::new("textures/huge.png")),
    );
    let announced = announced(&editor);
    assert!(
        announced.contains(&huge),
        "the released asset is invalidated, to be read again: {announced:?}"
    );
}

/// **A release reaches only a file of the project open**: a path climbing
/// out of its assets with `..` names nothing of the project, and another
/// project's quarantined file stays quarantined.
#[test]
fn a_release_climbing_out_of_the_assets_releases_nothing() {
    let folder = Folder::new("release-climbs");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    let other = folder.write("other/assets/huge.png", b"png");
    editor
        .refusals
        .refuse(too_large(other, Trust::Restricted), Some(stamp()));
    editor.state().pending_release = Some("../../other/assets/huge.png".to_owned());

    editor.frame();

    assert_eq!(
        editor.refusals.quarantined().len(),
        1,
        "another project's file stays quarantined: {:?}",
        editor.refusals.quarantined()
    );
}

/// **A release reaches only a file of the project open**: an absolute path
/// names nothing under its assets, and the file it names stays quarantined.
#[test]
fn a_release_of_an_absolute_path_releases_nothing() {
    let folder = Folder::new("release-absolute");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    let other = folder.write("other/assets/huge.png", b"png");
    editor
        .refusals
        .refuse(too_large(other.clone(), Trust::Restricted), Some(stamp()));
    editor.state().pending_release = Some(
        other
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    );

    editor.frame();

    assert_eq!(
        editor.refusals.quarantined().len(),
        1,
        "a file outside the project stays quarantined: {:?}",
        editor.refusals.quarantined()
    );
}

/// **The confirmation names every overlay trusting would use**: the shader
/// reload takes any `.wgsl` under the assets as an overlay of the module its
/// path names (`logical_name_for_shader_path`) — and the restricted mode
/// lists each as ignored — so one outside `assets/shaders` is used once the
/// project is trusted, and the confirmation says so.
#[test]
fn the_confirmation_names_every_overlay_the_shader_reload_would_use() {
    let folder = Folder::new("confirm-overlays");
    let root = cave_demo(&folder);
    folder.write("cave-demo/assets/materials/water.wgsl", b"// water");
    folder.write("cave-demo/assets/vfx/shaders/fire.wgsl", b"// fire");
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    editor.state().pending_menu_action = Some("confirm_trust".to_owned());

    editor.frame();

    let mut expected: Vec<String> = ["materials/water.wgsl", "vfx/shaders/fire.wgsl"]
        .iter()
        .filter_map(|rel| khora_sdk::logical_name_for_shader_path(rel))
        .collect();
    expected.push("khora::lighting".to_owned());
    expected.sort();
    let overlays = (editor.state().trust_confirm.clone())
        .map(|summary| summary.overlays)
        .unwrap_or_default();
    assert_eq!(
        overlays, expected,
        "every `.wgsl` the shader reload would use is named"
    );
}

/// **The quarantine shown follows the project open**: once another project
/// is open, the files of the one before are no longer shown — even when
/// nothing was refused meanwhile.
#[test]
fn the_quarantine_shown_follows_the_project_open() {
    let folder = Folder::new("mirror-switch");
    let root = cave_demo(&folder);
    let other = folder.0.join("other");
    std::fs::create_dir_all(other.join("assets")).expect("another project");
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root);
    editor.refusals.refuse(
        too_large(
            folder.write("cave-demo/assets/textures/huge.png", b"png"),
            Trust::Restricted,
        ),
        Some(stamp()),
    );
    editor.frame();
    assert_eq!(editor.state().quarantined.len(), 1, "the premise: shown");

    editor.open(&other);
    editor.frame();

    assert!(
        editor.state().quarantined.is_empty(),
        "cave-demo's file is not another project's: {:?}",
        editor.state().quarantined
    );
}

/// **A project opened by its canonical path shows its quarantine**: the
/// folder spelled `\?\…` on Windows, the refused file by the path its
/// guard joined — the same file, shown by its path under the assets.
#[test]
fn a_project_open_by_its_canonical_path_shows_its_quarantine() {
    let folder = Folder::new("mirror-canonical");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.open(&root.canonicalize().expect("the project's real path"));
    editor.refusals.refuse(
        too_large(
            folder.write("cave-demo/assets/textures/huge.png", b"png"),
            Trust::Restricted,
        ),
        Some(stamp()),
    );

    editor.frame();

    let shown: Vec<String> = (editor.state().quarantined.iter())
        .map(|asset| asset.rel_path.clone())
        .collect();
    assert_eq!(shown, vec!["textures/huge.png".to_owned()]);
}

/// **A project opened trusted holds none of its restricted refusals**: the
/// quarantine outlives a run, so a file the restricted limits refused for a
/// limit — held by its content — is still there when the project is next
/// opened, trusted from elsewhere (the hub's `Trust this project…`).
/// Opened trusted, it is read again at a trusted project's limits, as
/// trusting it in the editor (`grant_trust`) would have.
#[test]
fn a_project_opened_trusted_holds_none_of_its_restricted_refusals() {
    let folder = Folder::new("open-trusted");
    let root = cave_demo(&folder);
    let mut editor = Editor::new(ProjectTrust::Restricted);
    editor.refusals.refuse(
        over_limit(folder.write("cave-demo/assets/textures/wide.png", b"png")),
        Some(QuarantineKey::Content([4; 32])),
    );
    // Trusted outside this editor: the hub's confirmation writes the store.
    let store = editor.app.trust_store.as_ref().expect("the test's store");
    store.decide(&root, ProjectTrust::Trusted);

    editor
        .app
        .open_at_its_trust(root.clone(), &editor.engine.runtime);

    assert_eq!(editor.trust.get(), ProjectTrust::Trusted, "the premise");
    assert!(
        editor.refusals.quarantined().is_empty(),
        "what the restricted limits refused is read again: {:?}",
        editor.refusals.quarantined()
    );
}
