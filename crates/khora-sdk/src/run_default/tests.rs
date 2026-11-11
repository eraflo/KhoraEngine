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

//! The default runtime: what it reads beside its binary, and how it starts.
//!
//! The engine boots first; the runtime then files its heavy work by ticket —
//! a loose `assets/` directory's open, the default scene, the scripts'
//! compile — and waits on them through the engine's `Readiness`. These tests
//! drive the runtime app through `EngineCore::bootstrap` and `tick`, without a
//! window. Work filed off the frame spans frames, so they wait a bounded
//! number of frames for it, never assume the frame that lands it.

use super::*;
use crate::khora_core::asset::{AssetUUID, IndexTicket};
use crate::khora_core::ecs::entity::EntityId;
use crate::khora_core::renderer::api::gpu_scene::Mesh;
use crate::khora_core::scene::LoadTicket;
use crate::khora_core::script::ScriptValue;
use crate::prelude::ecs::{Name, Transform};
use crate::prelude::math::Vec3;
use crate::scripts::{CompileHub, CompileTicket};
use crate::{
    EngineCore, IndexHub, Readiness, ReadinessProgress, ReadinessState, SceneLoadHub,
    SerializationGoal, SerializationService,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The `runtime.json` the editor's "Build Game" writes for a project named
/// "Khora Demo" with the Release preset. The editor's own test compares
/// what it writes against the same file, so the writer and this reader are
/// held to one schema.
const WRITTEN_BY_THE_EDITOR: &str = include_str!("../../tests/fixtures/runtime.json");

/// One file per asset type the index builder knows. The bytes are not a
/// valid asset of any kind: the probe only asks whether a decoder is
/// registered for the type, not whether it accepts the file.
const PROBES: &[&str] = &[
    "probe.obj",
    "probe.png",
    "probe.wav",
    "probe.wgsl",
    "probe.ttf",
    "probe.kscene",
    "probe.kmat",
    "probe.erg",
    "probe.kprefab",
];

/// The asset types a loose-files project can decode. The editor's
/// `ProjectVfs` test pins the same list.
const DECODABLE: &[&str] = &[
    "audio", "font", "material", "mesh", "prefab", "script", "shader", "texture",
];

/// The most frames a piece of work filed off the frame may take to land: the
/// background run, the frame start that hands it over, the boundary that
/// takes it in. Frames are ~10 ms apart here, so this is seconds — a bound,
/// not the frame that lands it.
const MAX_FRAMES: usize = 500;

/// How many tickets of each hub are looked at when asking whether anything
/// filed is still in flight. A hub numbers its tickets from 1, and a runtime
/// files a handful at start-up.
const TICKETS: u64 = 16;

/// One triangle, as an `.obj`.
const TRIANGLE: &[u8] = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

/// The scene the runtime opens when `runtime.json` names none.
const DEFAULT_SCENE: &str = "scenes/default.kscene";

/// A behaviour counting the frames it ran in `older`.
const COUNTER: &str = "behavior Counter {
    int older = 0;
    void Update(float dt) { older += 1; }
}";

/// The entity of the default scene running [`COUNTER`].
const WATCHMAN: &str = "Watchman";

fn fixture_field(name: &str) -> serde_json::Value {
    let value: serde_json::Value =
        serde_json::from_str(WRITTEN_BY_THE_EDITOR).expect("the fixture is valid JSON");
    value[name].clone()
}

/// A fresh scratch directory for one test, unique across the tests of this
/// process.
fn scratch_dir(tag: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("khora-sdk-{tag}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The asset types among [`PROBES`] that `svc` has a decoder for, sorted.
fn decodable_types(svc: &mut AssetService) -> Result<Vec<String>> {
    let mut decodable = Vec::new();
    for file in PROBES {
        let uuid = AssetUUID::new_v5(file);
        let type_name = svc
            .index()
            .get_metadata(&uuid)
            .ok_or_else(|| anyhow!("{file} is not indexed"))?
            .asset_type_name
            .clone();
        let no_decoder = match svc.load::<Mesh>(&uuid) {
            Ok(_) => false,
            Err(e) => format!("{e:#}").contains("No decoder registered"),
        };
        if !no_decoder {
            decodable.push(type_name);
        }
    }
    decodable.sort();
    Ok(decodable)
}

// ---------------------------------------------------------------------------
// A game beside the runtime binary, and the engine running it
// ---------------------------------------------------------------------------

/// A folder of its own under the system's temporary folder, standing for the
/// directory of the runtime binary — removed when dropped.
struct GameDir(PathBuf);

impl GameDir {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-run-default-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("a scratch game directory");
        GameDir(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `bytes` at `rel` under `<dir>/assets/`.
    fn asset(&self, rel: &str, bytes: &[u8]) {
        let path = self.0.join("assets").join(rel);
        std::fs::create_dir_all(path.parent().expect("an asset has a folder"))
            .expect("the asset's folder");
        std::fs::write(path, bytes).expect("the asset is written");
    }

    /// Packs `<dir>/project/assets/` into `<dir>/exe/`, the packed game's
    /// binary directory, which it returns. Nothing loose sits beside it.
    fn packed(&self, files: &[(&str, &[u8])]) -> PathBuf {
        let assets = self.0.join("project").join("assets");
        for (rel, bytes) in files {
            let path = assets.join(rel);
            std::fs::create_dir_all(path.parent().expect("an asset has a folder"))
                .expect("the asset's folder");
            std::fs::write(path, bytes).expect("the asset is written");
        }
        std::fs::create_dir_all(&assets).expect("the project's assets");
        let exe_dir = self.0.join("exe");
        crate::PackBuilder::new(&assets, &exe_dir)
            .build()
            .expect("the project packs");
        exe_dir
    }
}

impl Drop for GameDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        // The profiles the engines of this test filed, beside the game.
        let _ = std::fs::remove_dir_all(profiles_scratch());
    }
}

/// The folder the engines these tests boot file their machine profiles in,
/// one per test process (each test runs in its own): never the profiles of
/// whoever runs the tests, under their home folder. Removed with the test's
/// game folders.
fn profiles_scratch() -> PathBuf {
    std::env::temp_dir().join(format!(
        "khora-sdk-run-default-profiles-{}",
        std::process::id()
    ))
}

/// The config of a runtime started without `runtime.json`, opening `scene`.
fn opening(scene: &str) -> RuntimeConfig {
    RuntimeConfig {
        default_scene: scene.to_owned(),
        ..RuntimeConfig::defaults()
    }
}

/// The engine the runtime binary in `exe_dir` boots: its assets installed as
/// `first` installs them, then the runtime app set up. Its machine profile
/// is filed in [`profiles_scratch`], named by `KHORA_PROFILES`.
fn engine_with<A: EngineApp>(exe_dir: &Path, app: A) -> EngineCore<A> {
    std::env::set_var(khora_io::profiles::PROFILES_VAR, profiles_scratch());
    let mut runtime = Runtime::new();
    install_game_assets(&mut runtime, exe_dir, false);
    let mut engine = EngineCore::<A>::new();
    engine.bootstrap(app, runtime);
    engine
}

/// The engine running the default runtime app of the game in `exe_dir`.
fn runtime_engine(exe_dir: &Path, config: RuntimeConfig) -> EngineCore<DefaultRuntimeApp> {
    engine_with(
        exe_dir,
        DefaultRuntimeApp::at(exe_dir.to_path_buf(), config),
    )
}

/// One frame through the engine's own loop, then a pause so the background
/// work moves on.
fn frame<A: EngineApp>(engine: &mut EngineCore<A>) {
    engine.tick();
    std::thread::sleep(Duration::from_millis(10));
}

/// Frames until `done`, at most [`MAX_FRAMES`]; panics naming `what`.
fn frames_until<A: EngineApp>(
    engine: &mut EngineCore<A>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<A>) -> bool,
) {
    for _ in 0..MAX_FRAMES {
        frame(engine);
        if done(engine) {
            return;
        }
    }
    panic!("{what} within {MAX_FRAMES} frames");
}

fn resource<A: EngineApp, T: Clone + Send + Sync + 'static>(
    engine: &EngineCore<A>,
    what: &str,
) -> T {
    engine
        .runtime()
        .resources
        .get::<T>()
        .cloned()
        .unwrap_or_else(|| panic!("the engine installs {what} at boot"))
}

/// The asset service the engine serves.
fn served<A: EngineApp>(engine: &EngineCore<A>) -> Arc<Mutex<AssetService>> {
    engine
        .runtime()
        .services
        .get::<Arc<Mutex<AssetService>>>()
        .cloned()
        .expect("the runtime installs an asset service")
}

fn readiness<A: EngineApp>(engine: &EngineCore<A>) -> Arc<Readiness> {
    resource(engine, "Arc<Readiness>")
}

/// The engine's three hubs, read the way the runtime files with them.
#[derive(Clone)]
struct Hubs {
    index: Arc<IndexHub>,
    scene: Arc<SceneLoadHub>,
    compile: Arc<CompileHub>,
}

impl Hubs {
    fn of(runtime: &Runtime) -> Self {
        let get = |what: &str| -> String { format!("the engine installs {what} at boot") };
        Hubs {
            index: runtime
                .resources
                .get::<Arc<IndexHub>>()
                .cloned()
                .unwrap_or_else(|| panic!("{}", get("the index hub"))),
            scene: runtime
                .resources
                .get::<Arc<SceneLoadHub>>()
                .cloned()
                .unwrap_or_else(|| panic!("{}", get("the scene load hub"))),
            compile: runtime
                .resources
                .get::<Arc<CompileHub>>()
                .cloned()
                .unwrap_or_else(|| panic!("{}", get("the compile hub"))),
        }
    }

    /// Whether a project index filed is still in flight.
    fn indexing(&self) -> bool {
        (1..=TICKETS).any(|n| self.index.is_pending(IndexTicket(n)))
    }

    /// Whether a scene load filed is still in flight.
    fn loading(&self) -> bool {
        (1..=TICKETS).any(|n| self.scene.is_pending(LoadTicket(n)))
    }

    /// Whether a script compile filed is still in flight.
    fn compiling(&self) -> bool {
        (1..=TICKETS).any(|n| self.compile.is_pending(CompileTicket(n)))
    }
}

/// The names of the world's named entities, sorted.
fn names<A: EngineApp>(engine: &mut EngineCore<A>) -> Vec<String> {
    let world = engine.game_world_mut().expect("bootstrapped");
    let mut names: Vec<String> = world
        .iter_entities()
        .filter_map(|entity| {
            world
                .get_component::<Name>(entity)
                .map(|name| name.as_str().to_owned())
        })
        .collect();
    names.sort();
    names
}

/// The entity named `name`.
fn named<A: EngineApp>(engine: &mut EngineCore<A>, name: &str) -> Option<EntityId> {
    let world = engine.game_world_mut()?;
    world.iter_entities().find(|entity| {
        world
            .get_component::<Name>(*entity)
            .is_some_and(|n| n.as_str() == name)
    })
}

/// Field `field` of `entity`'s `behavior` row, as the World holds it.
fn int<A: EngineApp>(
    engine: &mut EngineCore<A>,
    entity: EntityId,
    behavior: &str,
    field: &str,
) -> Option<i64> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(behavior)?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            (name, ScriptValue::Int(value)) if name == field => Some(value),
            _ => None,
        })
}

/// The bytes of a scene holding one authored entity per name.
fn scene_bytes(entities: &[&str]) -> Vec<u8> {
    let mut world = GameWorld::new();
    for (i, name) in entities.iter().enumerate() {
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)),
            Name::new(*name),
        ));
        world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("a scene's entities are authored");
    }
    SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes()
}

/// The bytes of a scene holding [`WATCHMAN`], running the `Counter` its
/// scripts under `scripts` declare. The scene carries the declaration.
fn scene_with_a_counter(scripts: &Path) -> Vec<u8> {
    use khora_io::script::declarations::{apply_declarations, DeclaredComponents};
    use khora_io::script::project_compiler::ProjectCompiler;

    let compiled = ProjectCompiler::default().compile_all(scripts);
    let mut world = GameWorld::new();
    let report = apply_declarations(
        world.inner_world_mut(),
        &mut DeclaredComponents::default(),
        &compiled.declarations,
    );
    assert!(
        report.registered.contains(&"Counter".to_owned()),
        "the counter's module declares `Counter`: {report:?}"
    );
    let entity = world.spawn((Transform::identity(), Name::new(WATCHMAN)));
    world
        .add_declared_component(entity, "Counter", &[])
        .expect("`Counter` attaches");
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("the watchman is authored");
    SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes()
}

/// A loose game whose default scene holds [`WATCHMAN`] running [`COUNTER`],
/// from `assets/scripts/counter.erg`.
fn loose_game_with_a_counter(tag: &str) -> GameDir {
    let game = GameDir::new(tag);
    game.asset("scripts/counter.erg", COUNTER.as_bytes());
    let scene = scene_with_a_counter(&game.path().join("assets").join("scripts"));
    game.asset(DEFAULT_SCENE, &scene);
    game
}

// ---------------------------------------------------------------------------
// runtime.json
// ---------------------------------------------------------------------------

#[test]
fn runtime_json_written_by_the_editor_reads_back_field_for_field() {
    let cfg: RuntimeConfig =
        serde_json::from_str(WRITTEN_BY_THE_EDITOR).expect("the runtime reads it");

    assert_eq!(cfg.project_name, "Khora Demo");
    assert_eq!(cfg.default_scene, "scenes/default.kscene");
    assert_eq!(cfg.window_title.as_deref(), Some("Khora Demo"));
    assert_eq!(cfg.window_title(), "Khora Demo");
    assert_eq!(cfg.preset.as_deref(), Some("release"));
    assert!(cfg.verify_integrity);
}

/// A `runtime.json` written before the window title existed still boots,
/// and the window is titled after the project.
#[test]
fn runtime_json_without_a_window_title_titles_the_window_after_the_project() {
    let cfg: RuntimeConfig = serde_json::from_str(
        r#"{ "project_name": "Old Build", "default_scene": "scenes/a.kscene",
             "preset": "debug", "verify_integrity": false }"#,
    )
    .expect("an older runtime.json still reads");

    assert_eq!(cfg.window_title, None);
    assert_eq!(cfg.window_title(), "Old Build");
}

/// The scene the runtime loads when `runtime.json` does not name one is the
/// scene the editor saves by default.
#[test]
fn default_scene_path_is_the_one_the_editor_writes() {
    let from_empty: RuntimeConfig = serde_json::from_str("{}").expect("every field has a default");
    let written = fixture_field("default_scene");

    assert_eq!(
        from_empty.default_scene,
        written.as_str().unwrap_or_default()
    );
    assert_eq!(
        RuntimeConfig::defaults().default_scene,
        written.as_str().unwrap_or_default()
    );
}

// ---------------------------------------------------------------------------
// The asset service a runtime starts on
// ---------------------------------------------------------------------------

/// **A loose-files runtime starts on an empty service.** Its `assets/` is
/// walked off the frame once the engine runs, so the service built before
/// the boot indexes nothing and serves no project yet.
#[test]
fn a_loose_files_runtime_starts_on_an_empty_service() {
    let game = GameDir::new("loose-empty-start");
    for file in PROBES {
        game.asset(file, b"not a real asset");
    }

    let svc = build_asset_service(game.path(), Arc::new(MetricsRegistry::new()), false)
        .expect("a loose-files runtime starts");

    assert_eq!(
        svc.index().asset_count(),
        0,
        "the loose files were walked before the engine ran"
    );
    assert_eq!(svc.project_root(), None, "a project is served already");
}

/// **A loose-files runtime registers every decoder**, once its directory's
/// open has landed: every type a loose project holds decodes.
#[test]
fn a_loose_files_runtime_registers_every_decoder() -> Result<()> {
    let game = GameDir::new("loose-decoders");
    for file in PROBES {
        game.asset(file, b"not a real asset");
    }
    let mut engine = runtime_engine(game.path(), opening(DEFAULT_SCENE));
    let svc = served(&engine);
    frames_until(&mut engine, "the directory's open did not land", |_| {
        svc.lock()
            .is_ok_and(|svc| svc.project_root() == Some(game.path()))
    });

    let decodable = decodable_types(&mut svc.lock().expect("the service's lock"))?;

    assert_eq!(decodable, DECODABLE);
    Ok(())
}

/// The runtime reads the asset-identity registry the editor writes: a scene
/// renamed in the editor keeps the UUID every reference to it stored, so a
/// loose-files runtime, once its directory's open has landed, finds it under
/// that UUID, not under the one its new path would derive. The registry sits
/// in `.khora/` beside `assets/`, as in a project.
#[test]
fn a_loose_files_runtime_resolves_the_uuids_the_registry_froze() -> Result<()> {
    let game = GameDir::new("loose-registry");
    game.asset("scenes/renamed.kscene", b"scene bytes");
    let frozen = AssetUUID::new_v5("scenes/original.kscene");
    let mut registry = crate::AssetIdRegistry::load(game.path());
    registry.freeze("scenes/renamed.kscene", frozen);
    registry.save()?;

    let mut engine = runtime_engine(game.path(), opening(DEFAULT_SCENE));
    let svc = served(&engine);
    frames_until(&mut engine, "the directory's open did not land", |_| {
        svc.lock()
            .is_ok_and(|svc| svc.project_root() == Some(game.path()))
    });
    let mut svc = svc.lock().expect("the service's lock");
    let by_frozen = svc.load_raw(&frozen).ok();
    let by_path = svc
        .index()
        .get_metadata(&AssetUUID::new_v5("scenes/renamed.kscene"))
        .is_some();

    assert_eq!(
        by_frozen.as_deref(),
        Some(&b"scene bytes"[..]),
        "the frozen UUID does not resolve to the renamed scene"
    );
    assert!(
        !by_path,
        "the renamed scene is indexed under its path-derived UUID"
    );
    Ok(())
}

/// The start-up error a user sees when nothing is next to the binary reads
/// as one sentence: the source line break is a `\` continuation, not a run
/// of spaces inside the message.
#[test]
fn the_nothing_to_load_error_has_no_run_of_spaces() -> Result<()> {
    let dir = scratch_dir("nothing-to-load")?;
    let err = build_asset_service(&dir, Arc::new(MetricsRegistry::new()), false)
        .err()
        .map(|e| format!("{e:#}"));
    let _ = std::fs::remove_dir_all(&dir);

    let message = err.expect("an empty directory cannot start");
    assert!(
        !message.contains("  "),
        "the message carries the source indentation: {message:?}"
    );
    assert!(
        message.contains("and no `assets/` next to the binary"),
        "{message:?}"
    );
    Ok(())
}

/// A packed runtime registers the same decoders as a loose-files one, and
/// with `verify_integrity` it checks every asset against `manifest.bin`;
/// without it, it does not.
#[test]
fn a_packed_runtime_registers_every_decoder_and_honours_the_manifest() -> Result<()> {
    let dir = scratch_dir("packed")?;
    let assets = dir.join("project").join("assets");
    let exe_dir = dir.join("exe");
    std::fs::create_dir_all(&assets)?;
    for file in PROBES {
        std::fs::write(assets.join(file), b"not a real asset")?;
    }
    crate::PackBuilder::new(&assets, &exe_dir)
        .with_manifest(true)
        .build()?;

    let outcome = (|| -> Result<_> {
        let mut svc = build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), true)?;
        let decodable = decodable_types(&mut svc)?;

        // A manifest that no longer matches the pack: every asset is
        // refused when verifying, and served when not.
        let probe = AssetUUID::new_v5("probe.kscene");
        let mut forged = PackManifest::new();
        forged.insert(probe, b"other bytes");
        std::fs::write(exe_dir.join("manifest.bin"), forged.encode()?)?;
        let verified = build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), true)?
            .load_raw(&probe)
            .is_ok();
        let unverified = build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), false)?
            .load_raw(&probe)
            .is_ok();
        Ok((decodable, verified, unverified))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let (decodable, verified, unverified) = outcome?;

    assert_eq!(decodable, DECODABLE);
    assert!(!verified, "a forged manifest did not stop the load");
    assert!(
        unverified,
        "the manifest was applied without verify_integrity"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Starting by ticket
// ---------------------------------------------------------------------------

/// **A loose runtime is indexed off the frame.** Once `setup` returned, the
/// engine serves an empty service and the directory's open is in flight;
/// the startup waits. Frames later the open lands behind the same handle:
/// the service serves the directory, its files under the ids the registry
/// gives them.
#[test]
fn a_loose_runtime_is_indexed_off_the_frame() {
    let game = GameDir::new("indexed-off-the-frame");
    game.asset("meshes/hero.obj", TRIANGLE);
    game.asset("meshes/renamed.obj", TRIANGLE);
    let frozen = AssetUUID::new_v5("meshes/original.obj");
    let mut registry = crate::AssetIdRegistry::load(game.path());
    registry.freeze("meshes/renamed.obj", frozen);
    registry.save().expect("the registry saves");

    let mut engine = runtime_engine(game.path(), opening(DEFAULT_SCENE));
    let svc = served(&engine);
    let hubs = Hubs::of(engine.runtime());

    {
        let svc = svc.lock().expect("the service's lock");
        assert_eq!(
            svc.index().asset_count(),
            0,
            "setup walked the directory on the frame"
        );
        assert_eq!(svc.project_root(), None, "a project is served already");
    }
    assert!(hubs.indexing(), "setup filed no open of the directory");
    assert!(
        matches!(
            readiness(&engine).poll(engine.runtime(), Instant::now()),
            ReadinessState::Waiting(_)
        ),
        "the startup does not wait on the open"
    );

    frames_until(&mut engine, "the directory's open did not land", |_| {
        !hubs.indexing()
    });

    assert!(
        Arc::ptr_eq(&served(&engine), &svc),
        "the open did not land behind the handle the engine serves by"
    );
    let svc = svc.lock().expect("the service's lock");
    assert_eq!(
        svc.project_root(),
        Some(game.path()),
        "the open landed, and the engine does not serve the directory"
    );
    assert!(
        svc.index()
            .get_metadata(&AssetUUID::new_v5("meshes/hero.obj"))
            .is_some(),
        "the directory's mesh is not served"
    );
    assert!(
        svc.index().get_metadata(&frozen).is_some(),
        "the renamed mesh is not served under the id the registry froze"
    );
    assert!(
        svc.index()
            .get_metadata(&AssetUUID::new_v5("meshes/renamed.obj"))
            .is_none(),
        "the renamed mesh is served under its path-derived id"
    );
}

/// **A loose runtime opens its default scene by ticket**, once its
/// directory's open landed. Once `setup` returned, the world holds no
/// entity and no scene load is filed yet — the scene is not indexed. Frames
/// later the scene's entities are in the world.
#[test]
fn the_runtime_opens_its_default_scene_by_ticket_from_loose_files() {
    let game = GameDir::new("scene-loose");
    game.asset(DEFAULT_SCENE, &scene_bytes(&["Hero", "Door"]));

    let mut engine = runtime_engine(game.path(), opening(DEFAULT_SCENE));
    let hubs = Hubs::of(engine.runtime());

    assert!(
        names(&mut engine).is_empty(),
        "setup loaded the scene on the frame: {:?}",
        names(&mut engine)
    );
    assert!(
        hubs.indexing(),
        "the directory's open is not in flight after setup"
    );
    assert!(
        !hubs.loading(),
        "the scene was filed before the directory's open landed"
    );

    frames_until(&mut engine, "the default scene did not land", |engine| {
        names(engine) == ["Door", "Hero"]
    });
}

/// **A packed runtime opens its default scene by ticket**, filed in `setup`:
/// the pack's index is read before the boot, so nothing is indexed and the
/// load is in flight once `setup` returned, the world still empty. Frames
/// later the scene's entities are in the world.
#[test]
fn the_runtime_opens_its_default_scene_by_ticket_from_a_pack() {
    let game = GameDir::new("scene-packed");
    let scene = scene_bytes(&["Hero", "Door"]);
    let exe_dir = game.packed(&[(DEFAULT_SCENE, scene.as_slice())]);

    let mut engine = runtime_engine(&exe_dir, opening(DEFAULT_SCENE));
    let hubs = Hubs::of(engine.runtime());

    assert!(
        names(&mut engine).is_empty(),
        "setup loaded the scene on the frame: {:?}",
        names(&mut engine)
    );
    assert!(!hubs.indexing(), "a pack filed an index");
    assert!(hubs.loading(), "setup filed no load of the scene");

    frames_until(&mut engine, "the default scene did not land", |engine| {
        names(engine) == ["Door", "Hero"]
    });
}

/// **A runtime without its default scene starts empty**, loose or packed:
/// nothing is filed for the scene, the startup waits on nothing but the
/// directory's open, and is ready with the world empty.
#[test]
fn a_runtime_without_its_default_scene_starts_empty() {
    let game = GameDir::new("no-scene");
    game.asset("meshes/hero.obj", TRIANGLE);
    let exe_dir = game.packed(&[("meshes/hero.obj", TRIANGLE)]);

    for (kind, dir, waited) in [("loose", game.path(), 1), ("packed", exe_dir.as_path(), 0)] {
        let mut engine = runtime_engine(dir, opening("scenes/missing.kscene"));
        let hubs = Hubs::of(engine.runtime());
        frames_until(
            &mut engine,
            &format!("the {kind} runtime was not ready"),
            |engine| {
                readiness(engine).poll(engine.runtime(), Instant::now()) == ReadinessState::Ready
            },
        );
        for _ in 0..5 {
            frame(&mut engine);
        }

        assert!(!hubs.loading(), "{kind}: a scene load is in flight");
        assert_eq!(
            readiness(&engine).progress(engine.runtime()),
            ReadinessProgress {
                landed: waited,
                waited
            },
            "{kind}: the startup waited on something else than the directory's open"
        );
        assert!(
            names(&mut engine).is_empty(),
            "{kind}: the world is not empty: {:?}",
            names(&mut engine)
        );
        assert_eq!(
            engine
                .game_world_mut()
                .expect("bootstrapped")
                .iter_entities()
                .count(),
            0,
            "{kind}: the world is not empty"
        );
    }
}

/// **The runtime compiles its scripts by ticket.** Once `setup` returned,
/// the compile of `assets/scripts/` is in flight. Frames
/// later it has landed, and the counter of the default scene — loaded with
/// its declaration before its program — runs.
#[test]
fn the_runtime_compiles_its_scripts_by_ticket() {
    let game = loose_game_with_a_counter("scripts-by-ticket");

    let mut engine = runtime_engine(game.path(), opening(DEFAULT_SCENE));
    let hubs = Hubs::of(engine.runtime());

    assert!(
        hubs.compiling(),
        "setup returned with no compile in flight: the scripts were compiled \
         before it, or not filed"
    );

    let mut watchman = None;
    frames_until(&mut engine, "the counter did not run", |engine| {
        if watchman.is_none() {
            watchman = named(engine, WATCHMAN);
        }
        watchman.is_some_and(|entity| {
            int(engine, entity, "Counter", "older").is_some_and(|count| count >= 1)
        })
    });
    assert!(
        !hubs.compiling(),
        "the counter ran before its compile landed"
    );
}

/// **The prefabs a loose game's scripts spawn are the ones its directory
/// holds**, once its open landed: the catalog a script's `Spawn` is checked
/// against was filled at boot from the asset service as it was then — empty
/// for loose files — and must follow the service the open swaps in, as it
/// holds a packed game's prefabs from the start.
#[test]
fn a_loose_runtime_spawns_the_prefabs_its_directory_holds() {
    use crate::khora_data::scene::PrefabCatalog;

    const PREFAB: &str = "prefabs/crate.kprefab";
    let game = GameDir::new("prefab-catalog");
    game.asset(PREFAB, b"prefab bytes");
    game.asset(DEFAULT_SCENE, &scene_bytes(&["Hero"]));
    let exe_dir = game.packed(&[
        (PREFAB, b"prefab bytes".as_slice()),
        (DEFAULT_SCENE, scene_bytes(&["Hero"]).as_slice()),
    ]);

    for (kind, dir) in [("packed", exe_dir.as_path()), ("loose", game.path())] {
        let mut engine = runtime_engine(dir, opening(DEFAULT_SCENE));
        frames_until(
            &mut engine,
            &format!("the {kind} runtime was not ready"),
            |engine| {
                readiness(engine).poll(engine.runtime(), Instant::now()) == ReadinessState::Ready
            },
        );
        assert!(
            served(&engine)
                .lock()
                .expect("the service's lock")
                .index()
                .get_metadata(&AssetUUID::new_v5(PREFAB))
                .is_some(),
            "{kind}: the service does not index the prefab"
        );

        let catalog: PrefabCatalog = resource(&engine, "the prefab catalog");
        assert!(
            catalog.holds(AssetUUID::new_v5(PREFAB)),
            "{kind}: the service serves the prefab, and a script may not spawn it"
        );
    }
}

/// The bytes of a prefab holding one authored entity named `name`.
fn prefab_bytes(name: &str) -> Vec<u8> {
    use crate::khora_data::scene::{capture_subtree, write_scene_file, CompactEncoding};

    let mut world = GameWorld::new();
    let entity = world.spawn((Transform::identity(), Name::new(name)));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("a prefab's entities are authored");
    let record = capture_subtree(world.inner_world(), entity).expect("the prefab captures");
    write_scene_file(&record, &CompactEncoding)
        .expect("the prefab writes")
        .to_bytes()
}

/// The bytes of a scene holding one instance of the prefab at `prefab`, whose
/// bytes are `bytes`.
fn scene_with_an_instance(prefab: &str, bytes: &[u8]) -> Vec<u8> {
    use crate::khora_data::scene::{instantiate_prefab, read_scene_file, PrefabSource};

    struct One(AssetUUID, crate::khora_data::scene::SceneRecord);
    impl PrefabSource for One {
        fn prefab(&self, id: AssetUUID) -> Result<crate::khora_data::scene::SceneRecord, String> {
            (id == self.0)
                .then(|| self.1.clone())
                .ok_or_else(|| format!("no prefab {id:?}"))
        }
    }
    let file = crate::SceneFile::from_bytes(bytes).expect("the prefab reads");
    let record = read_scene_file(&file).expect("the prefab's record");
    let prefabs = Arc::new(One(AssetUUID::new_v5(prefab), record));

    let mut world = GameWorld::new();
    instantiate_prefab(
        world.inner_world_mut(),
        AssetUUID::new_v5(prefab),
        prefabs.as_ref(),
    )
    .expect("the prefab instantiates");
    SerializationService::with_prefabs(prefabs)
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes()
}

/// **The default scene's prefab instances are read from the game's
/// prefabs**, loose or packed: a loose game's scene is filed once its open
/// landed, so its instances are expanded from the directory the open swapped
/// in, not from the empty service the engine booted with.
#[test]
fn the_default_scene_expands_its_prefab_instances_from_the_games_prefabs() {
    const PREFAB: &str = "prefabs/crate.kprefab";
    let game = GameDir::new("scene-prefabs");
    let prefab = prefab_bytes("Crate");
    let scene = scene_with_an_instance(PREFAB, &prefab);
    game.asset(PREFAB, &prefab);
    game.asset(DEFAULT_SCENE, &scene);
    let exe_dir = game.packed(&[
        (PREFAB, prefab.as_slice()),
        (DEFAULT_SCENE, scene.as_slice()),
    ]);

    for (kind, dir) in [("loose", game.path()), ("packed", exe_dir.as_path())] {
        let mut engine = runtime_engine(dir, opening(DEFAULT_SCENE));
        frames_until(
            &mut engine,
            &format!("the {kind} scene's instance did not land"),
            |engine| names(engine) == ["Crate"],
        );
    }
}

/// The runtime app, watched: whether a ticket was in flight each time the
/// engine let its startup go.
struct Watched {
    app: DefaultRuntimeApp,
    hubs: Option<Hubs>,
    /// One entry per `startup_ready` call: whether an index, a scene load or
    /// a compile was still in flight then.
    ready: Vec<bool>,
}

impl Watched {
    fn over(app: DefaultRuntimeApp) -> Self {
        Watched {
            app,
            hubs: None,
            ready: Vec::new(),
        }
    }
}

impl PhaseProvider for Watched {}
impl EngineApp for Watched {
    fn window_config() -> WindowConfig {
        DefaultRuntimeApp::window_config()
    }
    fn new() -> Self {
        Watched::over(DefaultRuntimeApp::new())
    }
    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        self.app.setup(world, runtime);
        self.hubs = Some(Hubs::of(runtime));
    }
    fn provide(&self, runtime: &mut Runtime) {
        self.app.provide(runtime);
    }
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        self.app.update(world, inputs);
    }
    fn startup_ready(&mut self) {
        let in_flight = self
            .hubs
            .as_ref()
            .is_some_and(|hubs| hubs.indexing() || hubs.loading() || hubs.compiling());
        self.ready.push(in_flight);
        self.app.startup_ready();
    }
}

/// **The runtime is ready once its scene and its scripts landed.** A loose
/// game with a default scene and a script: the engine lets its startup go
/// once, on the frame the last of the three — the directory's open, the
/// scene load, the compile — has landed, with the scene's entity in the
/// world and its behaviour known; never before the scene was filed, never
/// again after.
#[test]
fn the_runtime_is_ready_once_its_scene_and_scripts_landed() {
    let game = loose_game_with_a_counter("ready");
    let app = Watched::over(DefaultRuntimeApp::at(
        game.path().to_path_buf(),
        opening(DEFAULT_SCENE),
    ));
    let mut engine = engine_with(game.path(), app);
    let calls =
        |engine: &mut EngineCore<Watched>| engine.app_mut().expect("bootstrapped").ready.len();
    assert_eq!(calls(&mut engine), 0, "the bootstrap let the startup go");

    let mut landed_frame = None;
    for at in 0..MAX_FRAMES {
        frame(&mut engine);
        if calls(&mut engine) > 0 {
            landed_frame = Some(at);
            break;
        }
    }
    assert!(
        landed_frame.is_some(),
        "the startup was not let go within {MAX_FRAMES} frames"
    );

    let in_flight = engine.app_mut().expect("bootstrapped").ready[0];
    assert!(
        !in_flight,
        "the startup was let go with an index, a scene load or a compile in flight"
    );
    assert_eq!(
        readiness(&engine).progress(engine.runtime()),
        ReadinessProgress {
            landed: 3,
            waited: 3
        },
        "the startup did not wait on the directory's open, the scene and the compile, \
         or was let go before the scene was filed"
    );
    assert_eq!(
        names(&mut engine),
        [WATCHMAN],
        "the scene's entity is not in the world once the startup was let go"
    );
    assert!(
        engine
            .game_world_mut()
            .expect("bootstrapped")
            .inner_world()
            .components()
            .key_named("Counter")
            .is_some(),
        "the scripts' behaviour is not known once the startup was let go"
    );

    for _ in 0..10 {
        frame(&mut engine);
    }
    assert_eq!(
        calls(&mut engine),
        1,
        "the startup was let go more than once"
    );
}

/// **The runtime names its game from its config.** A `runtime.json` written
/// before games had a version reads `0.1.0`; one that names it keeps it. The
/// game a config runs is its `project_name` at its `game_version` — and a
/// runtime started without `runtime.json` is the default config's game, not
/// its window title at the engine's version.
#[test]
fn the_runtime_names_its_game_from_its_config() {
    let older: RuntimeConfig = serde_json::from_str(
        r#"{ "project_name": "Khora Demo", "default_scene": "scenes/default.kscene" }"#,
    )
    .expect("a runtime.json written before games had a version reads");
    assert_eq!(older.game_version, "0.1.0", "a file without a game version");

    let built: RuntimeConfig =
        serde_json::from_str(WRITTEN_BY_THE_EDITOR).expect("the editor's runtime.json reads");
    assert_eq!(built.game_version, "0.1.0", "the version Build Game writes");

    let versioned: RuntimeConfig =
        serde_json::from_str(r#"{ "project_name": "Khora Demo", "game_version": "2.3.4" }"#)
            .expect("a runtime.json naming its version reads");
    assert_eq!(
        versioned.game_version, "2.3.4",
        "the version written is kept"
    );
    assert_eq!(
        versioned.game_identity(),
        crate::GameIdentity {
            name: "Khora Demo".to_owned(),
            version: "2.3.4".to_owned(),
        }
    );

    assert_eq!(
        RuntimeConfig::defaults().game_version,
        "0.1.0",
        "a runtime without runtime.json"
    );
    assert_eq!(
        <DefaultRuntimeApp as EngineApp>::game_identity(),
        crate::GameIdentity {
            name: RuntimeConfig::defaults().project_name,
            version: "0.1.0".to_owned(),
        },
        "the runtime app's game is its config's"
    );
}

/// **A blank game version in the runtime config is the default one.** A
/// `runtime.json` whose `game_version` is empty or blank names no version,
/// as a project's `project.json` with a blank `version` names none: the
/// game runs at `0.1.0`, the version the editor gives that same project,
/// never at an empty one.
#[test]
fn a_blank_game_version_in_the_runtime_config_is_the_default_one() {
    for blank in ["", "   "] {
        let cfg: RuntimeConfig = serde_json::from_str(&format!(
            r#"{{ "project_name": "Khora Demo", "game_version": "{blank}" }}"#
        ))
        .expect("a runtime.json with a blank game version reads");
        assert_eq!(
            cfg.game_identity().version,
            crate::DEFAULT_GAME_VERSION,
            "the game version {blank:?}"
        );
    }
}

/// **The runtime these tests boot reads no profile from the user's home.**
/// The default runtime app keeps a machine profile, at a developer's path
/// under the home folder unless `KHORA_PROFILES` names another. Every test
/// here boots it through [`runtime_engine`]: it must not read, nor file, the
/// profile of whoever runs the tests. A profile of this machine running the
/// runtime's game, every family's rows learnt at 7, sits in a home folder of
/// the test's own (`USERPROFILE`, `HOME`), and `KHORA_PROFILES` is not set
/// before the boot: the boot helper files the profiles elsewhere. The runtime
/// booted as the other tests boot it plans its first round at its quotes, not
/// at 7.
#[test]
fn the_runtime_these_tests_boot_reads_no_profile_from_the_home_folder() {
    use crate::khora_core::agent::gorna::{CostProfile, ProfileRow, StrategyId};
    use khora_io::profiles::{profile_path, save, ProfilePlace, PROFILES_VAR};
    let home = GameDir::new("profile-home");
    std::env::remove_var(PROFILES_VAR);
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());
    std::env::set_var("KHORA_RECORD_DECISIONS", home.path().join("journal.json"));
    let machine = khora_infra::platform::machine::read_machine(None);
    let game_identity = <DefaultRuntimeApp as EngineApp>::game_identity();
    let path = profile_path(ProfilePlace::Developer, &machine, &game_identity, &|key| {
        std::env::var_os(key)
    })
    .expect("a developer's path under the home folder");
    assert!(path.starts_with(home.path()), "{path:?}");
    let rows = (khora_agents::registered_agent::registered_agents().iter())
        .flat_map(|registration| {
            [
                StrategyId::LowPower,
                StrategyId::Balanced,
                StrategyId::HighPerformance,
            ]
            .map(|row| ProfileRow {
                agent: registration.agent.name().to_owned(),
                row,
                factor: 7.0,
                spread: 0.2,
                runs: 50,
            })
        })
        .collect();
    save(
        &path,
        &CostProfile {
            engine_version: crate::ENGINE_VERSION.to_owned(),
            machine,
            game: game_identity,
            rows,
        },
    )
    .expect("a profile in the home folder");

    let game = GameDir::new("profile-game");
    let engine = runtime_engine(game.path(), opening("scenes/missing.kscene"));
    let dcc = engine.dcc().expect("the engine booted its DCC");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut first = None;
    while first.is_none() && Instant::now() < deadline {
        first = dcc.recorded_decisions().epochs.first().cloned();
        std::thread::sleep(Duration::from_millis(10));
    }
    let first = first.expect("the DCC opened its first epoch");

    let seeded = !first.corrections.is_empty()
        && (first.corrections.iter()).all(|(_, _, c)| (c.factor - 7.0).abs() < 1e-6);
    assert!(
        !seeded,
        "the profile in the home folder seeded the runtime: {:?}",
        first.corrections
    );
}
