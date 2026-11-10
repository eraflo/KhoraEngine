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

//! **The editor opens its project behind its film.** The project named on
//! the command line is not walked on the frame: `setup` files an `Open` with
//! the engine's index hub and takes a hold on `Readiness`; `before_frame`
//! lands it once the `Open` landed — the project's VFS over the engine's
//! service, its default scene opened by ticket or created; `before_agents`
//! mounts its scripts and lets the hold go. The film waits on the index, the
//! scene and the scripts, and on the hold.
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

// ─── The tests ──────────────────────────────────────────────────────────────

/// **The project opens off the frame.** `setup` walks nothing: the engine's
/// service indexes no file of the project and the browser lists none; an
/// `Open` is filed, and what needs no index — the folder, the name and the
/// engine version — is filled at once. Before the `Open` lands, nothing
/// lands. Once it has, the browser lists the project's files and folders
/// under a new epoch, and the project's VFS serves the engine's service and
/// files its reindexes with the engine's hub: a file written since is
/// indexed off the frame, and the VFS sees it once that reindex lands.
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

/// **The default scene is opened by ticket.** Once the index lands and
/// holds the default scene, the scene is filed as a load — not loaded on
/// the frame: the world does not hold its entity yet — and `Readiness`
/// waits on it beside the index. Once the engine prepared and committed it,
/// it is in the world and is the editor's open scene, and `Readiness` counts
/// it landed.
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

/// **A project without a default scene gets one.** A bare project — a
/// `project.json`, no `assets/` — gets its `assets/` as it starts opening.
/// Once its index lands, a camera and a light are spawned and saved as the
/// default scene; the save is reindexed off the frame. Nothing waits on a
/// load: no scene load is filed, `Readiness` waits on the index alone, and
/// once the scripts' turn has passed the startup is ready.
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

/// **The film waits for the index, the scene and the scripts.** While the
/// `Open` is in flight, and after it landed until the scripts are filed,
/// the hold keeps `Readiness` waiting — a `before_agents` before the
/// project landed lets nothing go. The scene is waited on from the
/// project's landing, the scripts' compile from their mount, which lets the
/// hold go. `Readiness` is ready only once all three landed.
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

/// **A project that does not open lets the film go.** The editor's `Open`
/// fails — another project's `Open` superseded it. The failure is reported
/// to the Problems under `Load`, the open is dropped, the hold let go, and
/// `Readiness` is ready: the editor runs with no project.
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

/// **A project that does not open is not shown as open.** The editor's
/// `Open` fails; the editor runs with no project, so nothing in its state
/// names one: no folder — which the asset browser takes as a project to
/// list, reveal and create folders in — and no name in the title bar.
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

/// **A default scene on disk is never overwritten by a new one.** The
/// project's default scene is written while its `Open` is walked — after the
/// `loading` family read the folder, before the index landed — so the index
/// does not name it. The file is the user's: the project opening does not
/// spawn a camera and a light and save them over it.
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

/// **An opened project shows its game's version.** What needs no index is
/// filled as the project starts opening, its version with it: the `version`
/// of its `project.json`, or `0.1.0` for a project written before projects
/// had one.
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

/// **An opened project shows the version its game is built at.** What the
/// editor shows as the project's version is what "Build Game" writes and
/// what the editor names its game by (`project_identity`): a `version`
/// written with stray blanks around it is the same version once trimmed,
/// not a different one on screen.
#[test]
fn an_opened_project_shows_the_version_its_game_is_built_at() {
    let project = Project::with("version-blanks", &[]);
    std::fs::write(
        project.root().join("project.json"),
        r#"{ "name": "version-blanks", "version": " 2.3.4 ", "engine_version": "0.9.1" }"#,
    )
    .expect("the project's `project.json`");

    let built = crate::project_identity::project_identity(project.root())
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

/// **A project that does not open shows no version.** The editor's `Open`
/// fails; the editor runs with no project, so the version the project's
/// `project.json` gave as it started opening is no longer shown.
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
