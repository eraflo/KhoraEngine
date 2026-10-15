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

//! The open project's scripts follow the project: mounted when it opens,
//! unmounted when another replaces it or it closes — and the project's VFS
//! reads its changes from the engine's one watch.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::{
    AgentProvider, AssetChangeEvent, AssetWatch, DccService, EngineApp, EngineCore, EngineMode,
    GameWorld, InputEvent, MetricsRegistry, PhaseProvider, Runtime, WindowConfig,
};

use super::follow_open_project;
use crate::project_vfs::ProjectVfs;

/// An application that edits and does nothing else: the engine the editor
/// runs on, without the editor.
struct Editing;

impl AgentProvider for Editing {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Editing {}
impl EngineApp for Editing {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Editing
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        EngineMode::Custom("editor".to_owned())
    }
}

/// A headless engine in the editor's mode.
fn engine() -> EngineCore<Editing> {
    let mut engine = EngineCore::<Editing>::new();
    engine.bootstrap(Editing, Runtime::new());
    engine
}

/// A project on disk holding `assets/scripts/<module>` for each of
/// `modules`, removed when dropped.
struct Project(PathBuf);

impl Project {
    fn with(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        let scripts = root.join("assets").join("scripts");
        std::fs::create_dir_all(&scripts).expect("the project's script folder");
        for (module, source) in modules {
            std::fs::write(scripts.join(module), source).expect("the script is written");
        }
        Project(root)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn assets(&self) -> PathBuf {
        self.0.join("assets")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const GREETER: &str = "behavior Greeter { int greeted = 0; void OnSpawn() { greeted += 1; } }";
const LANTERN: &str = "component Lantern { float glow = 1.0; }";

/// The roots the engine's watch arms, in the OS's spelling.
fn watched(engine: &EngineCore<Editing>) -> Vec<PathBuf> {
    engine
        .runtime()
        .resources
        .get::<Arc<AssetWatch>>()
        .expect("the engine holds an AssetWatch")
        .roots()
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("the path exists")
}

/// The module the World's registry says declares `name` now; `None` when no
/// module does, or `name` is not registered.
fn declared_by(engine: &mut EngineCore<Editing>, name: &str) -> Option<String> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(name)?;
    world.components().vtable(key)?.declared_by.clone()
}

/// Whether the World's registry lists `name`.
fn registered(engine: &mut EngineCore<Editing>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
fn tick_until(
    engine: &mut EngineCore<Editing>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Editing>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        engine.tick();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

// ─── The mounted project follows the open one ───────────────────────────────

/// **Opening a project mounts its scripts**: `<project>/assets` is watched,
/// and while the editor edits, what the project's scripts declare registers.
#[test]
fn opening_a_project_mounts_its_assets_and_registers_its_scripts() {
    let project = Project::with("open-mounts", &[("greeter.erg", GREETER)]);
    let mut engine = engine();
    let mut mounted = None;

    assert_eq!(
        follow_open_project(engine.runtime(), &mut mounted, Some(project.root())),
        Some(1),
        "the project's one module compiled"
    );

    assert_eq!(watched(&engine), vec![canonical(&project.assets())]);
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        declared_by(engine, "Greeter").as_deref() == Some("greeter.erg")
    });
}

/// **The project already mounted is not mounted again.** Called every frame,
/// the editor recompiles nothing while the same project stays open.
#[test]
fn the_project_already_mounted_is_not_mounted_again() {
    let project = Project::with("open-once", &[("greeter.erg", GREETER)]);
    let engine = engine();
    let mut mounted = None;
    follow_open_project(engine.runtime(), &mut mounted, Some(project.root()));

    assert_eq!(
        follow_open_project(engine.runtime(), &mut mounted, Some(project.root())),
        None
    );
    assert_eq!(watched(&engine), vec![canonical(&project.assets())]);
}

/// **Opening another project unmounts the first and mounts the second**:
/// only the second is watched, its scripts register, and what the first
/// declared is orphaned — kept with its data, declared by no module.
#[test]
fn opening_another_project_unmounts_the_first_and_mounts_the_second() {
    let first = Project::with("switch-from", &[("greeter.erg", GREETER)]);
    let second = Project::with("switch-to", &[("lantern.erg", LANTERN)]);
    let mut engine = engine();
    let mut mounted = None;
    follow_open_project(engine.runtime(), &mut mounted, Some(first.root()));
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        declared_by(engine, "Greeter").is_some()
    });

    assert_eq!(
        follow_open_project(engine.runtime(), &mut mounted, Some(second.root())),
        Some(1),
        "the second project's one module compiled"
    );

    assert_eq!(watched(&engine), vec![canonical(&second.assets())]);
    tick_until(
        &mut engine,
        "`Lantern` to register and `Greeter` to be orphaned",
        |engine| {
            declared_by(engine, "Lantern").as_deref() == Some("lantern.erg")
                && declared_by(engine, "Greeter").is_none()
        },
    );
    assert!(registered(&mut engine, "Greeter"), "kept, with its data");
}

/// **Closing the project unmounts it**: nothing is watched any more.
#[test]
fn closing_the_project_unmounts_it() {
    let project = Project::with("close-unmounts", &[("greeter.erg", GREETER)]);
    let engine = engine();
    let mut mounted = None;
    follow_open_project(engine.runtime(), &mut mounted, Some(project.root()));

    assert_eq!(
        follow_open_project(engine.runtime(), &mut mounted, None),
        None
    );
    assert!(watched(&engine).is_empty(), "{:?}", watched(&engine));
}

// ─── The project's VFS is one subscriber of the engine's watch ──────────────

/// Polls `poll` until `done` holds over everything it read, or a few seconds
/// passed; returns everything it read.
fn poll_until(
    mut poll: impl FnMut() -> Vec<AssetChangeEvent>,
    mut done: impl FnMut(&[AssetChangeEvent]) -> bool,
) -> Vec<AssetChangeEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        events.extend(poll());
        if done(&events) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    events
}

fn mentions(events: &[AssetChangeEvent], rel_path: &str) -> bool {
    events.iter().any(|event| event.rel_path == rel_path)
}

/// **The VFS reads its project's changes from the engine's watch, without
/// taking them**: the script pump, reading the same watch under its own
/// name, still hears the same save.
#[test]
fn the_project_vfs_reads_changes_from_the_shared_watch_without_taking_them() {
    let project = Project::with("vfs-shared", &[]);
    let watch = Arc::new(AssetWatch::default());
    let vfs = ProjectVfs::open_watched(
        project.root().to_path_buf(),
        Arc::new(MetricsRegistry::new()),
        Arc::clone(&watch),
    )
    .expect("the project opens");
    watch.watch(&project.assets()).expect("watches the project");
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(project.assets().join("wood.png"), b"PNG").expect("writes");
    let read = poll_until(|| vfs.poll_changes(), |events| mentions(events, "wood.png"));
    assert!(
        mentions(&read, "wood.png"),
        "the VFS heard the save: {read:?}"
    );

    let pump = poll_until(
        || watch.poll_for("script_hot_reload"),
        |events| mentions(events, "wood.png"),
    );
    assert!(
        mentions(&pump, "wood.png"),
        "another subscriber heard it too: {pump:?}"
    );
}

/// **The VFS reports only its own project's changes.** The engine's watch
/// may arm other roots — a shader tree — and a change under one of them is
/// not a change to the project's assets, which the asset browser would
/// reindex under the wrong path.
#[test]
fn the_project_vfs_reports_only_its_own_projects_changes() {
    let project = Project::with("vfs-own", &[]);
    let other = Project::with("vfs-other", &[]);
    let watch = Arc::new(AssetWatch::default());
    let vfs = ProjectVfs::open_watched(
        project.root().to_path_buf(),
        Arc::new(MetricsRegistry::new()),
        Arc::clone(&watch),
    )
    .expect("the project opens");
    watch.watch(&project.assets()).expect("watches the project");
    watch.watch(&other.assets()).expect("watches another root");
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(other.assets().join("stray.png"), b"PNG").expect("writes");
    std::fs::write(project.assets().join("wood.png"), b"PNG").expect("writes");
    let heard = poll_until(
        || watch.poll_for("probe"),
        |events| mentions(events, "stray.png"),
    );
    assert!(
        mentions(&heard, "stray.png"),
        "the premise: the other root reports: {heard:?}"
    );

    let mut read = poll_until(|| vfs.poll_changes(), |events| mentions(events, "wood.png"));
    std::thread::sleep(Duration::from_millis(500));
    read.extend(vfs.poll_changes());
    assert!(
        mentions(&read, "wood.png"),
        "the VFS heard its own: {read:?}"
    );
    assert!(
        !mentions(&read, "stray.png"),
        "the VFS reported another root's change: {read:?}"
    );
}
