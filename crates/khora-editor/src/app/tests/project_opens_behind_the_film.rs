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

//! **The editor opens its project behind its film.**
//!
//! The project named on the command line is not walked on the frame: `setup`
//! files an `Open` with the engine's index hub and takes a hold on `Readiness`;
//! `before_frame` lands it once the `Open` landed — the project's VFS over the
//! engine's service, its default scene opened by ticket or created;
//! `before_agents` mounts its scripts and lets the hold go. The film waits on
//! the index, the scene and the scripts, and on the hold.
//!
//! The engine's side of the frames is played by hand: the index built and
//! landed ([`IndexingEngine`]), the scene loads prepared and committed
//! ([`scene_loads`]), the script compiles compiled and landed (here).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_script::toolchain::{CompileDone, CompileRequest, ScriptCompiler};
use khora_sdk::editor_ui::EditorState;
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::event::Channel;
use khora_sdk::khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_sdk::khora_core::problems::{ProblemSeverity, ProblemSource, Problems};
use khora_sdk::khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, World};
use khora_sdk::khora_data::scene::loading::SceneLoadRequest;
use khora_sdk::prelude::ecs::*;
use khora_sdk::{
    AssetWatch, GameWorld, IndexKind, Readiness, ReadinessHold, ReadinessProgress, ReadinessState,
    SerializationGoal, SerializationService, DEFAULT_SCENE_REL_PATH,
};

use crate::app::project_open::{
    land_opened_project, mount_opened_scripts, start_opening_project, OpeningProject,
};
use crate::commands::land_scene_loads;
use crate::project_vfs::ProjectVfs;
use crate::tests::project_index::IndexingEngine;
use crate::tests::scene_loads::{self, SETTLE_FRAMES};

// ─── A project on disk ──────────────────────────────────────────────────────

/// A project folder under the temp folder, removed when dropped.
struct Project(PathBuf);

impl Project {
    /// A project named `name` in its `project.json`, holding `files` under
    /// `assets/` — none means no `assets/` folder at all.
    fn with(name: &str, files: &[(&str, &[u8])]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-opens-behind-the-film-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the project folder");
        std::fs::write(
            root.join("project.json"),
            format!(r#"{{ "name": "{name}", "engine_version": "0.9.1" }}"#),
        )
        .expect("the project's `project.json`");
        for (rel, bytes) in files {
            let path = root.join("assets").join(rel);
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).expect("the asset's folder");
            }
            std::fs::write(path, bytes).expect("the asset is written");
        }
        Project(root)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    /// The absolute path of the asset at `rel`.
    fn asset(&self, rel: &str) -> PathBuf {
        self.0.join("assets").join(rel)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The bytes of a scene holding one authored entity named `name`.
fn scene_holding(name: &str) -> Vec<u8> {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), Name::new(name)));
    world.mark_authored(entity).expect("authored");
    SerializationService::new()
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes()
}

const GREETER: &[u8] = b"behavior Greeter { int greeted = 0; void OnSpawn() { greeted += 1; } }";

// ─── The engine the editor opens on ─────────────────────────────────────────

/// What the editor opens its project on: the engine's asset service, index
/// hub, scene load hub, compile hub and `Readiness`, in one runtime; the
/// editor's state, hold and memory of what it mounted; the world the engine
/// runs.
struct Opening {
    engine: IndexingEngine,
    readiness: Arc<Readiness>,
    watch: Arc<AssetWatch>,
    state: Arc<Mutex<EditorState>>,
    world: GameWorld,
    hold: Option<ReadinessHold>,
    opening: Option<OpeningProject>,
    mounted: Option<PathBuf>,
}

impl Opening {
    fn new() -> Self {
        let mut engine = IndexingEngine::new();
        scene_loads::install(&mut engine.runtime);
        khora_sdk::scripts::install(&mut engine.runtime);
        let readiness = Arc::new(Readiness::new());
        engine.runtime.resources.insert(Arc::clone(&readiness));
        let watch = engine
            .runtime
            .resources
            .get::<Arc<AssetWatch>>()
            .cloned()
            .expect("`scripts::install` installs the watch");
        let world = scene_loads::engine_world(&engine.runtime);
        Self {
            engine,
            readiness,
            watch,
            state: Arc::new(Mutex::new(EditorState::default())),
            world,
            hold: None,
            opening: None,
            mounted: None,
        }
    }

    /// `setup`: the project at `root` starts opening.
    fn start(&mut self, root: &Path) {
        self.opening = start_opening_project(
            root.to_path_buf(),
            Some(khora_sdk::trust::ProjectTrust::Trusted),
            &self.engine.runtime,
            &self.state,
            &mut self.hold,
        );
    }

    /// `before_frame`: the project lands, once its `Open` did.
    fn land(&mut self) -> Option<Arc<Mutex<ProjectVfs>>> {
        land_opened_project(
            &mut self.opening,
            &mut self.world,
            &self.engine.runtime,
            &self.engine.shared,
            &self.watch,
            &self.state,
            &mut self.hold,
        )
    }

    /// `before_agents`: the open project's scripts are mounted.
    fn mount_scripts(&mut self, project: Option<&Path>) {
        mount_opened_scripts(
            &self.engine.runtime,
            &mut self.mounted,
            project,
            &mut self.hold,
        );
    }

    /// Where the startup stands now.
    fn poll(&self) -> ReadinessState {
        self.readiness.poll(&self.engine.runtime, Instant::now())
    }

    fn progress(&self) -> ReadinessProgress {
        self.readiness.progress(&self.engine.runtime)
    }

    /// The engine's frames for scene loads, and the editor's landing of them
    /// in `before_frame`, until one replaced the world.
    fn settle_scenes(&mut self) {
        let hub = scene_loads::hub(&self.engine.runtime);
        for _ in 0..SETTLE_FRAMES {
            scene_loads::engine_frame(&self.engine.runtime, &mut self.world);
            if land_scene_loads(&mut self.world, &self.state, &hub) {
                return;
            }
        }
        panic!("no scene load landed within {SETTLE_FRAMES} frames");
    }

    /// The `loading` family's run over the script compiles filed, then the
    /// boundary's `script_compile_intake`, which files their outcomes.
    fn compile_frame(&self) {
        let runtime = &self.engine.runtime;
        let requests = runtime
            .resources
            .get::<Channel<CompileRequest>>()
            .cloned()
            .expect("the runtime holds the compile requests");
        let compiler = runtime
            .resources
            .get::<Arc<dyn ScriptCompiler>>()
            .cloned()
            .expect("the runtime holds the script compiler");
        let reserve = runtime
            .resources
            .get::<Reserve>()
            .cloned()
            .expect("the runtime holds the reserve");
        let mut staging = Staging::new();
        for request in requests.drain() {
            let (compiled, lease) = compiler.compile_leased(&request);
            let done = CompileDone {
                root: request.root.clone(),
                tickets: request.tickets.clone(),
                outcome: Ok(compiled.programs()),
            };
            match lease {
                Some(lease) => staging.stage_while(vec![done], lease),
                None => staging.stage(vec![done]),
            }
        }
        reserve.receive(staging);
        let intake = khora_sdk::inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == "script_compile_intake")
            .expect("`script_compile_intake` is registered");
        (intake.run)(
            &mut World::new(),
            runtime,
            &mut OutputDeck::new(),
            SystemContext {
                since: Tick(0),
                resumed: false,
                scope: None,
            },
        );
    }

    /// How many scene loads wait to be prepared.
    fn scene_loads_filed(&self) -> usize {
        self.engine
            .runtime
            .resources
            .get::<Channel<SceneLoadRequest>>()
            .map_or(0, |requests| requests.len())
    }

    /// Whether the world holds an entity named `name`.
    fn holds(&self, name: &str) -> bool {
        self.world.iter_entities().any(|entity| {
            self.world
                .get_component::<Name>(entity)
                .is_some_and(|named| named.0 == name)
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, EditorState> {
        self.state.lock().expect("the editor state")
    }
}

fn waiting(landed: usize, waited: usize) -> ReadinessState {
    ReadinessState::Waiting(ReadinessProgress { landed, waited })
}

/// The engine's frames for scene loads, and the editor's landing of them, for as long as a load may take to land.
fn run_scene_frames(opening: &mut Opening) {
    let hub = scene_loads::hub(&opening.engine.runtime);
    for _ in 0..SETTLE_FRAMES {
        scene_loads::engine_frame(&opening.engine.runtime, &mut opening.world);
        land_scene_loads(&mut opening.world, &opening.state, &hub);
    }
}

// ─── The tests ──────────────────────────────────────────────────────────────

/// **The project opens off the frame.**
///
/// `setup` walks nothing: the engine's service indexes no file of the project
/// and the browser lists none; an `Open` is filed, and what needs no index —
/// the folder, the name and the engine version — is filled at once. Before the
/// `Open` lands, nothing lands. Once it has, the browser lists the project's
/// files and folders under a new epoch, and the project's VFS serves the
/// engine's service and files its reindexes with the engine's hub: a file
/// written since is indexed off the frame, and the VFS sees it once that
/// reindex lands.
#[test]
fn the_project_opens_off_the_frame() {
    let scene = scene_holding("Lantern");
    let project = Project::with(
        "opens-off-the-frame",
        &[
            ("textures/crate.png", b"PNG"),
            (DEFAULT_SCENE_REL_PATH, scene.as_slice()),
        ],
    );
    let mut opening = Opening::new();
    let crate_png = AssetUUID::new_v5("textures/crate.png");

    opening.start(project.root());

    assert!(opening.opening.is_some(), "the open was not filed");
    assert!(
        !opening.engine.indexes(&crate_png),
        "the project was indexed on the frame"
    );
    assert_eq!(opening.engine.waiting(), 1, "one `Open` is filed");
    let epoch = {
        let state = opening.state();
        assert!(
            state.asset_entries.is_empty(),
            "the browser lists files before the index landed"
        );
        assert_eq!(
            state.project_folder.as_deref(),
            Some(&*project.root().to_string_lossy())
        );
        assert_eq!(state.project_name.as_deref(), Some("opens-off-the-frame"));
        assert_eq!(state.project_engine_version.as_deref(), Some("0.9.1"));
        state.asset_epoch
    };

    // A `before_frame` before the engine built the index: nothing lands.
    assert!(
        opening.land().is_none(),
        "a project landed before its index"
    );
    assert!(opening.opening.is_some(), "the open in flight was dropped");

    opening.engine.frame();
    let vfs = opening
        .land()
        .expect("the project lands once its `Open` did");

    assert!(opening.opening.is_none(), "the open is still in flight");
    {
        let state = opening.state();
        assert!(
            state
                .asset_entries
                .iter()
                .any(|entry| entry.source_path == "textures/crate.png"),
            "the browser does not list the project's file: {:?}",
            state.asset_entries
        );
        assert!(
            state.asset_dirs.iter().any(|dir| dir == "textures"),
            "the browser does not list the project's folder: {:?}",
            state.asset_dirs
        );
        assert_ne!(
            state.asset_epoch, epoch,
            "the browser is not told to rescan"
        );
    }
    assert!(
        vfs.lock()
            .expect("the project")
            .assets()
            .index()
            .get_metadata(&crate_png)
            .is_some(),
        "the project's VFS does not read the index the `Open` landed"
    );

    // A file written since, reindexed: filed with the engine's hub, landed in
    // the engine's service, seen through the project's VFS.
    std::fs::write(project.asset("textures/rust.png"), b"PNG").expect("a new file");
    let rust_png = AssetUUID::new_v5("textures/rust.png");
    let filed = opening.engine.filed();
    vfs.lock()
        .expect("the project")
        .reindex()
        .expect("the reindex is filed");
    assert_eq!(
        opening.engine.filed(),
        filed + 1,
        "the project's reindex was not filed with the engine's hub"
    );
    assert!(
        vfs.lock()
            .expect("the project")
            .assets()
            .index()
            .get_metadata(&rust_png)
            .is_none(),
        "the reindex ran on the frame"
    );
    opening.engine.frame();
    assert!(
        vfs.lock()
            .expect("the project")
            .assets()
            .index()
            .get_metadata(&rust_png)
            .is_some(),
        "the project's VFS does not serve the engine's service"
    );
}

/// **The default scene is opened by ticket.**
///
/// Once the index lands and holds the default scene, the scene is filed as a
/// load — not loaded on the frame: the world does not hold its entity yet — and
/// `Readiness` waits on it beside the index. Once the engine prepared and
/// committed it, it is in the world and is the editor's open scene, and
/// `Readiness` counts it landed.
#[test]
fn the_default_scene_is_opened_by_ticket() {
    let scene = scene_holding("Lantern");
    let project = Project::with(
        "default-scene-by-ticket",
        &[(DEFAULT_SCENE_REL_PATH, scene.as_slice())],
    );
    let mut opening = Opening::new();
    opening.start(project.root());
    opening.engine.frame();

    opening.land().expect("the project lands");

    assert!(
        !opening.holds("Lantern"),
        "the default scene was loaded on the frame"
    );
    assert_eq!(opening.scene_loads_filed(), 1, "no scene load was filed");
    assert_eq!(
        opening.progress(),
        ReadinessProgress {
            landed: 1,
            waited: 2
        },
        "`Readiness` does not wait on the scene beside the index"
    );

    opening.settle_scenes();

    assert!(
        opening.holds("Lantern"),
        "the default scene did not land into the world"
    );
    let current = opening.state().current_scene_path.clone();
    assert!(
        current
            .as_deref()
            .is_some_and(|path| Path::new(path).ends_with(DEFAULT_SCENE_REL_PATH)),
        "the default scene is not the editor's open scene: {current:?}"
    );
    assert_eq!(
        opening.progress(),
        ReadinessProgress {
            landed: 2,
            waited: 2
        }
    );
}

/// **A project without a default scene gets one.**
///
/// A bare project — a `project.json`, no `assets/` — gets its `assets/` as it
/// starts opening. Once its index lands, a camera and a light are spawned and
/// saved as the default scene; the save is reindexed off the frame. Nothing
/// waits on a load: no scene load is filed, `Readiness` waits on the index
/// alone, and once the scripts' turn has passed the startup is ready.
#[test]
fn a_project_without_a_default_scene_gets_one() {
    let project = Project::with("default-scene-created", &[]);
    let mut opening = Opening::new();

    opening.start(project.root());

    assert!(
        project.root().join("assets").is_dir(),
        "the project's `assets/` was not created as it started opening"
    );
    opening.engine.frame();
    opening.land().expect("the project lands");

    let camera = opening
        .world
        .iter_entities()
        .find(|entity| opening.world.get_component::<Camera>(*entity).is_some());
    assert!(camera.is_some(), "no camera was spawned");
    let light = opening
        .world
        .iter_entities()
        .find(|entity| opening.world.get_component::<Light>(*entity).is_some());
    assert!(light.is_some(), "no light was spawned");
    assert!(
        project.asset(DEFAULT_SCENE_REL_PATH).is_file(),
        "the default scene was not saved"
    );
    assert_eq!(
        opening.engine.waiting(),
        1,
        "the save's reindex was not filed with the engine's hub"
    );
    assert!(
        !opening
            .engine
            .indexes(&AssetUUID::new_v5(DEFAULT_SCENE_REL_PATH)),
        "the save was reindexed on the frame"
    );
    assert_eq!(opening.scene_loads_filed(), 0, "a scene load was filed");
    assert!(
        opening.state().opening.is_empty(),
        "the editor waits on a scene opening"
    );
    assert_eq!(
        opening.progress(),
        ReadinessProgress {
            landed: 1,
            waited: 1
        },
        "`Readiness` waits on something beside the index"
    );

    opening.mount_scripts(Some(project.root()));

    assert!(
        opening.hold.is_none(),
        "the hold is kept after the scripts' turn"
    );
    assert_eq!(opening.poll(), ReadinessState::Ready);
}

/// **The film waits for the index, the scene and the scripts.**
///
/// While the `Open` is in flight, and after it landed until the scripts are
/// filed, the hold keeps `Readiness` waiting — a `before_agents` before the
/// project landed lets nothing go. The scene is waited on from the project's
/// landing, the scripts' compile from their mount, which lets the hold go.
/// `Readiness` is ready only once all three landed.
#[test]
fn the_film_waits_for_the_index_the_scene_and_the_scripts() {
    let scene = scene_holding("Lantern");
    let project = Project::with(
        "film-waits-for-three",
        &[
            (DEFAULT_SCENE_REL_PATH, scene.as_slice()),
            ("scripts/Greeter.erg", GREETER),
        ],
    );
    let mut opening = Opening::new();

    opening.start(project.root());

    assert!(opening.hold.is_some(), "no hold was taken on `Readiness`");
    assert_eq!(opening.poll(), waiting(0, 1), "the index is not waited on");

    // A `before_agents` while the `Open` is in flight: no project yet.
    opening.mount_scripts(None);
    assert!(
        opening.hold.is_some(),
        "the hold was let go before the project landed"
    );

    opening.engine.frame();
    assert_eq!(
        opening.poll(),
        waiting(1, 1),
        "with the index landed, the hold keeps the film up"
    );

    let vfs = opening.land().expect("the project lands");
    assert!(
        opening.hold.is_some(),
        "the hold was let go before the scripts"
    );
    assert_eq!(opening.poll(), waiting(1, 2), "the scene is not waited on");

    let root = vfs.lock().expect("the project").root.clone();
    opening.mount_scripts(Some(&root));
    assert!(
        opening.hold.is_none(),
        "the hold is kept once everything is filed"
    );
    assert_eq!(
        opening.poll(),
        waiting(1, 3),
        "the scripts are not waited on"
    );

    opening.settle_scenes();
    assert_eq!(
        opening.poll(),
        waiting(2, 3),
        "ready before the scripts compiled"
    );

    opening.compile_frame();
    assert_eq!(opening.poll(), ReadinessState::Ready);
}

/// **A project that does not open lets the film go.**
///
/// The editor's `Open` fails — another project's `Open` superseded it. The
/// failure is reported to the Problems under `Load`, the open is dropped, the
/// hold let go, and `Readiness` is ready: the editor runs with no project.
#[test]
fn a_project_that_does_not_open_lets_the_film_go() {
    let project = Project::with("does-not-open", &[("textures/crate.png", b"PNG")]);
    let elsewhere = Project::with("supersedes", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    assert!(opening.hold.is_some(), "no hold was taken on `Readiness`");

    opening
        .engine
        .hub
        .file(elsewhere.root().to_path_buf(), IndexKind::Open);
    opening.engine.frame();

    assert!(
        opening.land().is_none(),
        "a project whose `Open` failed was opened"
    );
    assert!(opening.opening.is_none(), "the failed open is still kept");
    assert!(opening.hold.is_none(), "the hold is kept after the failure");
    let problems = opening
        .engine
        .runtime
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("the runtime holds the Problems");
    let reported = problems.of_source(&ProblemSource::Load);
    assert!(
        reported
            .iter()
            .any(|problem| problem.severity == ProblemSeverity::Error),
        "the failure was not reported under `Load`: {reported:?}"
    );
    assert_eq!(opening.poll(), ReadinessState::Ready);
}

/// **A project that does not open is not shown as open.**
///
/// The editor's `Open` fails; the editor runs with no project, so nothing in
/// its state names one: no folder — which the asset browser takes as a project
/// to list, reveal and create folders in — and no name in the title bar.
#[test]
fn a_project_that_does_not_open_is_not_shown_as_open() {
    let project = Project::with("not-shown-as-open", &[("textures/crate.png", b"PNG")]);
    let elsewhere = Project::with("not-shown-supersedes", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    opening
        .engine
        .hub
        .file(elsewhere.root().to_path_buf(), IndexKind::Open);
    opening.engine.frame();

    assert!(opening.land().is_none(), "the premise: the open fails");

    let state = opening.state();
    assert_eq!(
        state.project_folder, None,
        "the editor runs with no project, yet its state names the folder"
    );
    assert_eq!(
        state.project_name, None,
        "the editor runs with no project, yet its title names one"
    );
}

/// **A default scene on disk is never overwritten by a new one.**
///
/// The project's default scene is written while its `Open` is walked — after
/// the `loading` family read the folder, before the index landed — so the index
/// does not name it. The file is the user's: the project opening does not spawn
/// a camera and a light and save them over it.
#[test]
fn a_default_scene_on_disk_is_never_overwritten_by_a_new_one() {
    let project = Project::with("default-scene-not-overwritten", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());

    opening.engine.build();
    let scene = scene_holding("Lantern");
    let path = project.asset(DEFAULT_SCENE_REL_PATH);
    std::fs::create_dir_all(path.parent().expect("the scenes folder"))
        .expect("the scenes folder is made");
    std::fs::write(&path, &scene).expect("the default scene is written");
    opening.engine.intake();

    opening.land().expect("the project lands");

    assert!(
        std::fs::read(&path).expect("the default scene is still there") == scene,
        "the user's default scene was overwritten by a new one"
    );
}

/// **An opened project shows its game's version.**
///
/// What needs no index is filled as the project starts opening, its version
/// with it: the `version` of its `project.json`, or `0.1.0` for a project
/// written before projects had one.
#[test]
fn an_opened_project_shows_its_games_version() {
    let older = Project::with("version-older", &[]);
    let versioned = Project::with("version-given", &[]);
    std::fs::write(
        versioned.root().join("project.json"),
        r#"{ "name": "version-given", "version": "2.3.4", "engine_version": "0.9.1" }"#,
    )
    .expect("the project's `project.json`");

    for (project, version) in [(&older, "0.1.0"), (&versioned, "2.3.4")] {
        let mut opening = Opening::new();
        opening.start(project.root());
        assert_eq!(
            opening.state().project_version.as_deref(),
            Some(version),
            "{}",
            project.root().display()
        );
    }
}

/// **An opened project shows the version its game is built at.**
///
/// What the editor shows as the project's version is what "Build Game" writes
/// and what the editor names its game by (`project_identity`): a `version`
/// written with stray blanks around it is the same version once trimmed, not a
/// different one on screen.
#[test]
fn an_opened_project_shows_the_version_its_game_is_built_at() {
    let project = Project::with("version-blanks", &[]);
    std::fs::write(
        project.root().join("project.json"),
        r#"{ "name": "version-blanks", "version": " 2.3.4 ", "engine_version": "0.9.1" }"#,
    )
    .expect("the project's `project.json`");

    let built = crate::project_identity::project_identity(
        &crate::project_vfs::ProjectReads::default().guard(project.root()),
    )
    .expect("the project names its game")
    .version;
    let mut opening = Opening::new();
    opening.start(project.root());

    assert_eq!(built, "2.3.4", "the premise: the game is built at 2.3.4");
    assert_eq!(
        opening.state().project_version.as_deref(),
        Some(built.as_str()),
        "the editor shows another version than the one its game is built at"
    );
}

/// **A project that does not open shows no version.**
///
/// The editor's `Open` fails; the editor runs with no project, so the version
/// the project's `project.json` gave as it started opening is hidden.
#[test]
fn a_project_that_does_not_open_shows_no_version() {
    let project = Project::with("no-version-shown", &[("textures/crate.png", b"PNG")]);
    std::fs::write(
        project.root().join("project.json"),
        r#"{ "name": "no-version-shown", "version": "2.3.4", "engine_version": "0.9.1" }"#,
    )
    .expect("the project's `project.json`");
    let elsewhere = Project::with("no-version-supersedes", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    assert_eq!(
        opening.state().project_version.as_deref(),
        Some("2.3.4"),
        "the premise: the version is shown as the project starts opening"
    );
    opening
        .engine
        .hub
        .file(elsewhere.root().to_path_buf(), IndexKind::Open);
    opening.engine.frame();

    assert!(opening.land().is_none(), "the premise: the open fails");
    assert_eq!(
        opening.state().project_version,
        None,
        "the editor runs with no project, yet shows its version"
    );
}

/// **A project opened while playing opens in editing.**
///
/// Play ends with the project it played: the scene it would restore at Stop
/// is the former project's, so it goes, and the new project is edited.
#[test]
fn a_project_opened_while_playing_opens_in_editing() {
    let project = Project::with("opened-while-playing", &[]);
    let mut opening = Opening::new();
    {
        let mut state = opening.state();
        state.play_mode = khora_sdk::PlayMode::Playing;
        state.scene_snapshot = Some(b"the former project's scene".to_vec());
    }

    opening.start(project.root());
    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");

    let state = opening.state();
    assert_eq!(state.play_mode, khora_sdk::PlayMode::Editing);
    assert!(
        state.scene_snapshot.is_none(),
        "the former project's scene would be restored at Stop"
    );
}

/// **A landed project reads through the run's refusals, at the trust of the
/// project open**: its VFS reports to the refusals the runtime holds, and a
/// trust set since is the one its next read is guarded at.
#[test]
fn a_landed_project_reads_through_the_runs_refusals_at_its_trust() {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};

    let project = Project::with("reads-through-the-run", &[]);
    let mut opening = Opening::new();
    let refusals = Arc::new(Refusals::new(None, None));
    opening
        .engine
        .runtime
        .resources
        .insert(Arc::clone(&refusals));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    opening.engine.runtime.resources.insert(Arc::clone(&trust));

    opening.start(project.root());
    opening.engine.frame();
    let pvfs = opening.land().expect("the project lands");

    let pvfs = pvfs.lock().expect("the project");
    let assets = project.root().join("assets");
    assert!(
        Arc::ptr_eq(&pvfs.reads().refusals(), &refusals),
        "the project reports to the run's refusals"
    );
    assert_eq!(
        pvfs.reads().guard(&assets).trust(),
        Trust::Project,
        "the project opened trusted is read as a trusted project's"
    );
    trust.set_for(project.root(), ProjectTrust::Restricted);
    assert_eq!(
        pvfs.reads().guard(&assets).trust(),
        Trust::Restricted,
        "a trust set since guards the next read"
    );
}

/// The `Leftover` problems the runtime of `opening` holds.
fn leftovers(opening: &Opening) -> Vec<khora_sdk::khora_core::problems::Problem> {
    opening
        .engine
        .runtime
        .resources
        .get::<Arc<Problems>>()
        .expect("the runtime holds the Problems")
        .of_source(&ProblemSource::Leftover)
}

/// **What a rename cut short left aside under a taken name is reported as the project opens**, as a warning, and cleared once fixed and the project opened again.
#[test]
fn an_aside_left_at_the_open_is_reported_and_cleared_by_the_next_open() {
    let project = Project::with("aside-left", &[("scenes/level.kscene", b"kept")]);
    let aside = project.asset("scenes/level.kscene.1.2.aside.tmp");
    std::fs::write(&aside, b"aside").expect("the aside is written");
    let mut opening = Opening::new();

    opening.start(project.root());
    opening.engine.frame();
    opening.land().expect("the project lands");

    let left = leftovers(&opening);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(left[0].severity, ProblemSeverity::Warning);
    assert_eq!(std::fs::read(&aside).expect("the aside is kept"), b"aside");

    std::fs::remove_file(&aside).expect("the author removes the aside");
    opening.start(project.root());
    opening.engine.frame();
    opening.land().expect("the project lands again");

    assert_eq!(
        leftovers(&opening),
        Vec::new(),
        "the fixed aside is still reported"
    );
}

/// **What a project left aside is not reported once another open leaves the editor with no project.**
///
/// The next open fails: nothing of the former project is shown as open, its
/// leftovers included.
#[test]
fn an_aside_reported_is_cleared_when_the_next_open_fails() {
    let project = Project::with("aside-then-fail", &[("scenes/level.kscene", b"kept")]);
    std::fs::write(project.asset("scenes/level.kscene.1.2.aside.tmp"), b"aside")
        .expect("the aside is written");
    let next = Project::with("aside-next-fails", &[]);
    let elsewhere = Project::with("aside-next-superseded", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    opening.engine.frame();
    opening.land().expect("the project lands");
    assert_eq!(
        leftovers(&opening).len(),
        1,
        "the premise: one aside reported"
    );

    opening.start(next.root());
    opening
        .engine
        .hub
        .file(elsewhere.root().to_path_buf(), IndexKind::Open);
    opening.engine.frame();
    assert!(opening.land().is_none(), "the premise: the next open fails");

    assert_eq!(
        opening.state().project_folder,
        None,
        "the premise: no project is open"
    );
    assert_eq!(
        leftovers(&opening),
        Vec::new(),
        "the closed project's aside is still reported"
    );
}

/// **What the editor filed of the project it held leaves with it**: a save's and a load's problems are cleared as the next open resolves, and the failure is what is reported.
#[test]
fn the_problems_filed_of_a_project_leave_when_the_next_open_fails() {
    use khora_sdk::khora_core::problems::Problem;

    let project = Project::with("problems-then-fail", &[]);
    let next = Project::with("problems-next-fails", &[]);
    let elsewhere = Project::with("problems-next-superseded", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    opening.engine.frame();
    opening.land().expect("the project lands");
    let problems = Arc::clone(
        opening
            .engine
            .runtime
            .resources
            .get::<Arc<Problems>>()
            .expect("the runtime holds the Problems"),
    );
    let left_out = Problem::new(ProblemSeverity::Warning, "a mesh made in code is not saved");
    problems.report(ProblemSource::Save, vec![left_out]);

    opening.start(next.root());
    opening
        .engine
        .hub
        .file(elsewhere.root().to_path_buf(), IndexKind::Open);
    opening.engine.frame();
    assert!(opening.land().is_none(), "the premise: the next open fails");

    assert_eq!(problems.of_source(&ProblemSource::Save), Vec::new());
    let load = problems.of_source(&ProblemSource::Load);
    assert_eq!(load.len(), 1, "{load:?}");
    assert!(load[0].message.contains("could not be opened"), "{load:?}");
}

/// **A scene of the former project still opening lands nothing once another project opened.**
///
/// The former project's default scene was filed, and had not landed, when the
/// next project — one without a default scene — opened and got one. The
/// editor holds the new project: the former one's scene does not replace its
/// scene, nor become the scene a save writes to.
#[test]
fn a_scene_of_the_former_project_still_opening_lands_nothing_once_another_opened() {
    let scene = scene_holding("Lantern");
    let former = Project::with(
        "former-scene-in-flight",
        &[(DEFAULT_SCENE_REL_PATH, scene.as_slice())],
    );
    let next = Project::with("next-without-scene", &[]);
    let mut opening = Opening::new();
    opening.start(former.root());
    opening.engine.frame();
    opening.land().expect("the former project lands");
    assert_eq!(
        opening.scene_loads_filed(),
        1,
        "the premise: its scene is filed"
    );

    opening.start(next.root());
    opening.engine.frame();
    opening.land().expect("the next project lands");
    assert!(
        next.asset(DEFAULT_SCENE_REL_PATH).is_file(),
        "the premise: the next project got its default scene"
    );
    run_scene_frames(&mut opening);

    assert!(
        !opening.holds("Lantern"),
        "the former project's scene replaced the next project's"
    );
    let current = opening.state().current_scene_path.clone();
    assert!(
        current
            .as_deref()
            .is_none_or(|path| !Path::new(path).starts_with(former.root())),
        "the open scene is the former project's file: {current:?}"
    );
}

/// **A project without a default scene gets one holding none of the former project's entities**, and that scene is the one the editor saves to.
#[test]
fn a_default_scene_made_after_another_project_holds_none_of_its_entities() {
    let scene = scene_holding("Lantern");
    let former = Project::with(
        "former-scene-landed",
        &[(DEFAULT_SCENE_REL_PATH, scene.as_slice())],
    );
    let next = Project::with("next-gets-a-scene", &[]);
    let mut opening = Opening::new();
    opening.start(former.root());
    opening.engine.frame();
    opening.land().expect("the former project lands");
    opening.settle_scenes();
    assert!(opening.holds("Lantern"), "the premise: its scene is open");

    opening.start(next.root());
    opening.engine.frame();
    opening.land().expect("the next project lands");

    assert!(
        !opening.holds("Lantern"),
        "the next project's new default scene holds the former project's entities"
    );
    let current = opening.state().current_scene_path.clone();
    assert!(
        current
            .as_deref()
            .is_some_and(|path| Path::new(path).starts_with(next.root())),
        "the scene a save writes to is not the next project's: {current:?}"
    );
}

/// **A project opened while playing opens in editing, a scene chosen as it opened kept**: the scene stays, the play and the scene it would restore at Stop go with the former project.
#[test]
fn a_project_opened_while_playing_opens_in_editing_with_a_scene_chosen_meanwhile() {
    let scene = scene_holding("Chosen");
    let chosen = Project::with(
        "chosen-while-playing",
        &[("chosen.kscene", scene.as_slice())],
    );
    let project = Project::with("opened-while-playing-chosen", &[]);
    let mut opening = Opening::new();
    {
        let mut state = opening.state();
        state.play_mode = khora_sdk::PlayMode::Playing;
        state.scene_snapshot = Some(b"the former project's scene".to_vec());
    }

    opening.start(project.root());
    let hub = scene_loads::hub(&opening.engine.runtime);
    let reads = crate::project_vfs::ProjectReads::of(&opening.engine.runtime);
    crate::commands::open_scene_dispatch(
        None,
        &mut opening.world,
        &opening.state,
        &hub,
        &reads,
        &chosen.asset("chosen.kscene"),
    )
    .expect("the chosen scene is filed");
    opening.settle_scenes();
    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");

    assert!(
        opening.holds("Chosen"),
        "the premise: the chosen scene stays"
    );
    let state = opening.state();
    assert_eq!(state.play_mode, khora_sdk::PlayMode::Editing);
    assert!(
        state.scene_snapshot.is_none(),
        "the former project's scene would be restored at Stop"
    );
}

/// **A scene chosen as the project opened, then played, is kept as it was chosen**: the play ends as Stop ends it, and what the play did to the scene goes with the play.
#[test]
fn a_scene_chosen_then_played_as_the_project_opens_is_kept_as_chosen() {
    let scene = scene_holding("Chosen");
    let chosen = Project::with("played-chosen", &[("chosen.kscene", scene.as_slice())]);
    let project = Project::with("opened-over-a-played-scene", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    let hub = scene_loads::hub(&opening.engine.runtime);
    let reads = crate::project_vfs::ProjectReads::of(&opening.engine.runtime);
    crate::commands::open_scene_dispatch(
        None,
        &mut opening.world,
        &opening.state,
        &hub,
        &reads,
        &chosen.asset("chosen.kscene"),
    )
    .expect("the chosen scene is filed");
    opening.settle_scenes();
    // Play, as the toolbar starts it; the play then changes the scene.
    opening
        .state()
        .push_request(khora_sdk::editor_ui::EditorRequest::Transport(
            khora_sdk::editor_ui::Transport::Play,
        ));
    crate::commands::process_transport(&mut opening.world, &opening.state, &opening.engine.runtime);
    assert_eq!(
        opening.state().play_mode,
        khora_sdk::PlayMode::Playing,
        "the premise: the chosen scene is played"
    );
    let played: Vec<_> = opening.world.iter_entities().collect();
    for entity in played {
        opening.world.despawn(entity);
    }
    opening
        .world
        .spawn((Transform::identity(), Name::new("Spawned in play")));

    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");
    run_scene_frames(&mut opening);

    assert_eq!(opening.state().play_mode, khora_sdk::PlayMode::Editing);
    assert!(
        !opening.holds("Spawned in play"),
        "the scene kept is the play's, not the one chosen"
    );
    assert!(
        opening.holds("Chosen"),
        "the chosen scene went with the play"
    );
}

/// **A scene chosen as the project opened keeps its load problems**: the scene stays open, so what its load adapted is still reported.
#[test]
fn a_scene_chosen_as_the_project_opened_keeps_its_load_problems() {
    let chosen = Project::with("chosen-with-problems", &[]);
    let path = chosen.root().join("broken.kscene");
    let mut authored = GameWorld::new();
    // A camera whose near plane sits at its eye: its load reports it.
    let camera = authored.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Camera {
            z_near: 0.0,
            ..Camera::default_perspective()
        },
        Name::new("Main Camera"),
    ));
    authored
        .inner_world_mut()
        .mark_authored(camera)
        .expect("authored");
    crate::tests::scene_files::write_scene(&authored, &path);
    let project = Project::with("opened-with-a-scene-chosen", &[]);
    let mut opening = Opening::new();
    let problems = Arc::clone(
        opening
            .engine
            .runtime
            .resources
            .get::<Arc<Problems>>()
            .expect("the runtime holds the Problems"),
    );

    opening.start(project.root());
    let hub = scene_loads::hub(&opening.engine.runtime);
    let reads = crate::project_vfs::ProjectReads::of(&opening.engine.runtime);
    crate::commands::open_scene_dispatch(
        None,
        &mut opening.world,
        &opening.state,
        &hub,
        &reads,
        &path,
    )
    .expect("the chosen scene is filed");
    opening.settle_scenes();
    // What the app does at each frame's start: the load's problems reported.
    let reported = opening.state().take_requests(|request| match request {
        khora_sdk::editor_ui::EditorRequest::ReportLoadProblems { problems } => Ok(problems),
        other => Err(other),
    });
    for load in reported {
        problems.report(ProblemSource::Load, load);
    }
    assert!(
        !problems.of_source(&ProblemSource::Load).is_empty(),
        "the premise: the chosen scene's load reports its camera"
    );
    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");

    assert!(
        opening.holds("Main Camera"),
        "the premise: the chosen scene stays"
    );
    assert!(
        !problems.of_source(&ProblemSource::Load).is_empty(),
        "the open scene's load problems are no longer reported"
    );
}

/// **A Stop filed as the project opens restores nothing of the former project**: the play ends with its project, the restore in flight is withdrawn, and the scene chosen meanwhile stays.
#[test]
fn a_stop_filed_as_the_project_opens_restores_nothing_of_the_former_project() {
    use khora_sdk::khora_data::scene::loading::{LoadKind, SceneSource};

    let scene = scene_holding("Chosen");
    let chosen = Project::with("stop-chosen", &[("chosen.kscene", scene.as_slice())]);
    let project = Project::with("stop-while-opening", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    let hub = scene_loads::hub(&opening.engine.runtime);
    let reads = crate::project_vfs::ProjectReads::of(&opening.engine.runtime);
    crate::commands::open_scene_dispatch(
        None,
        &mut opening.world,
        &opening.state,
        &hub,
        &reads,
        &chosen.asset("chosen.kscene"),
    )
    .expect("the chosen scene is filed");
    opening.settle_scenes();
    // The restore a Stop files, of a scene the former project played.
    let ticket = hub.file(
        opening.world.inner_world_mut(),
        LoadKind::Replace(SceneSource::Bytes(Arc::from(scene_holding("Former")))),
        Arc::new(khora_sdk::khora_data::scene::NoPrefabs),
    );
    opening.state().restoring = Some(ticket);

    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");
    run_scene_frames(&mut opening);

    assert!(
        !opening.holds("Former"),
        "Stop restored a scene of the former project"
    );
    assert!(opening.holds("Chosen"), "the chosen scene went");
    assert_eq!(opening.state().restoring, None);
}

/// **A Play asked as the project opens waits for its scene**: the project's
/// default scene is still opening when the transport runs, so Play is
/// refused; asked once the scene landed, it plays that scene.
#[test]
fn a_play_asked_as_the_project_opens_waits_for_its_scene() {
    use khora_sdk::editor_ui::{EditorRequest, Transport};

    let scene = scene_holding("Lantern");
    let project = Project::with(
        "play-as-it-opens",
        &[(DEFAULT_SCENE_REL_PATH, scene.as_slice())],
    );
    let mut opening = Opening::new();
    opening.start(project.root());
    opening
        .state()
        .push_request(EditorRequest::Transport(Transport::Play));

    // `before_frame`: the project lands, then the transport runs.
    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");
    crate::commands::process_transport(&mut opening.world, &opening.state, &opening.engine.runtime);

    assert_eq!(
        opening.state().play_mode,
        khora_sdk::PlayMode::Editing,
        "Play started before the project's scene landed"
    );
    assert!(
        opening.state().scene_snapshot.is_none(),
        "a refused Play keeps a snapshot"
    );

    opening.settle_scenes();
    opening
        .state()
        .push_request(EditorRequest::Transport(Transport::Play));
    crate::commands::process_transport(&mut opening.world, &opening.state, &opening.engine.runtime);

    assert_eq!(opening.state().play_mode, khora_sdk::PlayMode::Playing);
    assert!(
        opening.holds("Lantern"),
        "the scene played is not the project's"
    );
}

/// **A Stop filed as the project opens, of the scene chosen meanwhile, is kept**: it restores the scene as it was chosen, and the play's changes go.
#[test]
fn a_stop_of_the_scene_chosen_as_the_project_opens_restores_it_as_chosen() {
    use khora_sdk::editor_ui::{EditorRequest, Transport};

    let scene = scene_holding("Chosen");
    let chosen = Project::with("stopped-chosen", &[("chosen.kscene", scene.as_slice())]);
    let project = Project::with("opened-over-a-stopped-scene", &[]);
    let mut opening = Opening::new();
    opening.start(project.root());
    let hub = scene_loads::hub(&opening.engine.runtime);
    let reads = crate::project_vfs::ProjectReads::of(&opening.engine.runtime);
    crate::commands::open_scene_dispatch(
        None,
        &mut opening.world,
        &opening.state,
        &hub,
        &reads,
        &chosen.asset("chosen.kscene"),
    )
    .expect("the chosen scene is filed");
    opening.settle_scenes();
    for transport in [Transport::Play, Transport::Stop] {
        opening
            .state()
            .push_request(EditorRequest::Transport(transport));
        crate::commands::process_transport(
            &mut opening.world,
            &opening.state,
            &opening.engine.runtime,
        );
        if transport == Transport::Play {
            opening
                .world
                .spawn((Transform::identity(), Name::new("Spawned in play")));
        }
    }
    assert!(
        opening.state().restoring.is_some(),
        "the premise: Stop's restore is in flight"
    );

    opening.engine.frame();
    assert!(opening.land().is_some(), "the project lands");
    assert!(
        opening.state().restoring.is_some(),
        "the restore of the scene chosen is withdrawn"
    );
    run_scene_frames(&mut opening);

    assert_eq!(opening.state().restoring, None);
    assert!(
        !opening.holds("Spawned in play"),
        "the scene kept is the play's, not the one chosen"
    );
    assert!(opening.holds("Chosen"), "the chosen scene went");
}
