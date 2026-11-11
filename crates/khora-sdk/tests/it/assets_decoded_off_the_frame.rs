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

//! **A game's mesh asset is decoded off the frame, and resolves a few
//! frames after its entity appears.** The engine boots the `loading` family;
//! the asset resolver asks the asset service for the mesh, the family's lane
//! decodes it at the background, and the decode lands at a frame's boundary,
//! where the intake installs it. The frame that asks never decodes on its
//! own thread, a slow decode never lengthens a frame, and a mesh whose file
//! changes while its decode is in flight never shows its old bytes.
//!
//! The decode spans frames, so these tests wait a bounded number of frames
//! for it, never assume the frame that asks.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use khora_data::ecs::{HandleComponent, MeshRef};
use khora_io::asset::{AssetDecoder, AssetIdRegistry, AssetService, MeshDispatcher};
use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};
use khora_telemetry::MetricsRegistry;

/// The family that decodes assets off the frame.
const LOADING: AgentId = AgentId::new("loading");

/// The most frames a mesh may take to resolve: the background run, the
/// frame start that hands it over. Frames are ~10 ms apart here, so this is
/// seconds — a bound, not the frame that asks.
const MAX_FRAMES: usize = 500;

/// How long the held decoder waits to be released before it decodes anyway:
/// far longer than a frame, so a frame that waits on it shows.
const HOLD: Duration = Duration::from_secs(3);

/// A frame that waited on a held decode lasts at least [`HOLD`]; one that
/// did not lasts a few milliseconds.
const LONG_FRAME: Duration = Duration::from_millis(1_500);

/// One triangle.
const TRIANGLE_OBJ: &str = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";

/// A quad, two triangles over four corners: told from the triangle by its
/// vertex count — six, as the OBJ decoder gives each triangle its own
/// vertices.
const QUAD_OBJ: &str =
    "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 1.0 1.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\nf 1 3 4\n";

const MESH: &str = "meshes/shape.obj";

/// A game: every family on from the first frame.
struct Game;

impl PhaseProvider for Game {}
impl EngineApp for Game {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// What the watched mesh decoder saw: the threads it decoded on, how many
/// decodes it made, and — when it holds one of its decodes — whether that one
/// started and whether the test released it.
#[derive(Default)]
struct DecodeLog {
    threads: Mutex<Vec<ThreadId>>,
    calls: AtomicUsize,
    /// The decode held, counted from 0, if any.
    hold: Mutex<Option<usize>>,
    entered: AtomicBool,
    released: AtomicBool,
}

impl DecodeLog {
    fn holding_the_first() -> Arc<Self> {
        let log = Arc::new(Self::default());
        log.hold_the_next();
        log
    }

    /// Holds the next decode the decoder makes.
    fn hold_the_next(&self) {
        *self
            .hold
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(self.calls.load(Ordering::SeqCst));
    }

    fn threads(&self) -> Vec<ThreadId> {
        self.threads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
    }
}

/// The engine's mesh decoder, watched: it records the thread of every
/// decode, and holds the one it was asked to until released (at most
/// [`HOLD`]).
struct WatchedMeshes {
    inner: MeshDispatcher,
    log: Arc<DecodeLog>,
}

impl AssetDecoder<Mesh> for WatchedMeshes {
    fn sniff(&self, head: &[u8]) -> bool {
        self.inner.sniff(head)
    }

    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        self.load_at(bytes, Path::new(""))
    }

    fn load_at(
        &self,
        bytes: &[u8],
        source_path: &Path,
    ) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        self.log
            .threads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(std::thread::current().id());
        let call = self.log.calls.fetch_add(1, Ordering::SeqCst);
        let held = *self
            .log
            .hold
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if Some(call) == held {
            self.log.entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + HOLD;
            while !self.log.released.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.inner.load_at(bytes, source_path)
    }
}

/// A project of its own under the system's temporary folder, holding the
/// mesh `MESH` as `obj` — removed when dropped.
struct Project(PathBuf);

impl Project {
    fn with_mesh(name: &str, obj: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-decoded-off-the-frame-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        let project = Project(root);
        project.save(obj);
        project
    }

    /// Writes the mesh file as `obj`, as an author's tool does.
    fn save(&self, obj: &str) {
        let path = self.0.join("assets").join(MESH);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the mesh folder");
        std::fs::write(path, obj).expect("the mesh is written");
    }

    /// The project's asset service, its meshes decoded by the watched
    /// decoder logging into `log`.
    fn service(&self, log: &Arc<DecodeLog>) -> Arc<Mutex<AssetService>> {
        let mut service = AssetService::open_loose_files(
            &self.0,
            &AssetIdRegistry::load(&self.0),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        service.register_decoder::<Mesh>(
            "mesh",
            WatchedMeshes {
                inner: MeshDispatcher::default(),
                log: Arc::clone(log),
            },
        );
        Arc::new(Mutex::new(service))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mesh_uuid() -> AssetUUID {
    AssetUUID::new_v5(MESH)
}

/// A game engine over `assets`, as a game's launcher installs its service.
fn game_engine(assets: &Arc<Mutex<AssetService>>) -> EngineCore<Game> {
    let mut runtime = Runtime::new();
    runtime.services.insert(Arc::clone(assets));
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    engine
}

/// One frame in the order a game's loop runs it.
fn frame(engine: &mut EngineCore<Game>) {
    let runtime = Arc::clone(engine.runtime());
    let inputs = engine.drain_inputs();
    engine.run_app_update(&inputs);
    let presents = engine.begin_render_frame(&runtime);
    engine.run_scheduler(&runtime);
    engine.end_render_frame(presents);
    engine.run_maintenance();
    engine.close_frame();
    std::thread::sleep(Duration::from_millis(10));
}

/// Frames until `done` holds, at most [`MAX_FRAMES`]; returns how many it
/// took, or panics naming `what`.
fn frames_until(
    engine: &mut EngineCore<Game>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Game>) -> bool,
) -> usize {
    for taken in 1..=MAX_FRAMES {
        frame(engine);
        if done(engine) {
            return taken;
        }
    }
    panic!("{what} did not happen within {MAX_FRAMES} frames");
}

fn spawn_mesh(engine: &mut EngineCore<Game>) -> EntityId {
    engine
        .game_world_mut()
        .expect("bootstrapped")
        .spawn(MeshRef::Asset(mesh_uuid()))
}

/// How many vertices the mesh resolved onto `entity` holds, if one is.
fn resolved_vertices(engine: &mut EngineCore<Game>, entity: EntityId) -> Option<usize> {
    engine
        .game_world_mut()?
        .inner_world()
        .get::<HandleComponent<Mesh>>(entity)
        .map(|resolved| resolved.handle.positions.len())
}

// ─── The family ─────────────────────────────────────────────────────────────

/// **The engine boots the `loading` family**, on from the first frame.
#[test]
fn the_engine_boots_the_loading_family() {
    let project = Project::with_mesh("boots", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let mut engine = game_engine(&project.service(&log));
    frame(&mut engine);

    let families = engine
        .dcc()
        .expect("bootstrapped")
        .agent_registry()
        .lock()
        .expect("the registry lock")
        .all_ids();
    assert!(families.contains(&LOADING), "booted families: {families:?}");
}

// ─── A mesh asset ───────────────────────────────────────────────────────────

/// **A mesh asset resolves within a bounded number of frames after its
/// entity appears, and not on the frame that asks**, to the mesh a
/// synchronous load gives.
#[test]
fn a_mesh_asset_resolves_a_few_frames_after_its_entity_appears() {
    let project = Project::with_mesh("resolves", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let mut engine = game_engine(&project.service(&log));
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);

    frame(&mut engine);
    assert_eq!(
        resolved_vertices(&mut engine, entity),
        None,
        "the frame that asks does not resolve the mesh"
    );
    frames_until(&mut engine, "the mesh to resolve", |engine| {
        resolved_vertices(engine, entity).is_some()
    });

    assert_eq!(
        resolved_vertices(&mut engine, entity),
        Some(3),
        "the triangle the file holds"
    );
}

/// **The frame that asks for a mesh never decodes it on the frame's
/// thread**: every decode the mesh decoder made ran on another thread than
/// the one running the frames.
#[test]
fn the_frame_that_asks_for_a_mesh_never_decodes_it_on_the_frame_thread() {
    let project = Project::with_mesh("off-thread", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let mut engine = game_engine(&project.service(&log));
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);

    frames_until(&mut engine, "the mesh to resolve", |engine| {
        resolved_vertices(engine, entity).is_some()
    });

    let frame_thread = std::thread::current().id();
    let threads = log.threads();
    assert!(!threads.is_empty(), "the mesh was decoded");
    assert!(
        threads.iter().all(|thread| *thread != frame_thread),
        "no decode ran on the frame's thread: {threads:?} (frames on {frame_thread:?})"
    );
}

/// **A slow decode never lengthens a frame**: while the mesh decoder holds
/// its decode, the frames go on at their pace.
#[test]
fn a_slow_decode_never_lengthens_a_frame() {
    let project = Project::with_mesh("slow", TRIANGLE_OBJ);
    let log = DecodeLog::holding_the_first();
    let mut engine = game_engine(&project.service(&log));
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);

    // Every frame from the one that asks, until the decode was held for
    // five more frames, is timed.
    let mut longest = Duration::ZERO;
    let mut after_entered = 0;
    for _ in 0..MAX_FRAMES {
        let started = Instant::now();
        frame(&mut engine);
        longest = longest.max(started.elapsed());
        if log.entered.load(Ordering::SeqCst) {
            after_entered += 1;
            if after_entered > 5 {
                break;
            }
        }
    }
    assert!(after_entered > 0, "the held decode started");
    let unresolved_while_held = resolved_vertices(&mut engine, entity).is_none();
    log.release();

    assert!(
        longest < LONG_FRAME,
        "a frame lasted {longest:?} while the decode was held"
    );
    assert!(
        unresolved_while_held,
        "the mesh waits for its decode, the frames do not"
    );
    frames_until(&mut engine, "the mesh to resolve once released", |engine| {
        resolved_vertices(engine, entity).is_some()
    });
}

/// **A mesh asked for while the `loading` family is off waits, and resolves
/// once the family is on again**: its request is held, not dropped.
#[test]
fn a_mesh_asked_for_while_the_loading_family_is_off_resolves_once_it_is_on() {
    let project = Project::with_mesh("family-off", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let mut engine = game_engine(&project.service(&log));
    let activation = engine
        .runtime()
        .resources
        .get::<Arc<khora_sdk::khora_core::agent::Activation>>()
        .cloned()
        .expect("the engine's activation");
    activation
        .deactivate(LOADING)
        .expect("the loading family is known");
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);

    for _ in 0..20 {
        frame(&mut engine);
    }
    assert_eq!(
        resolved_vertices(&mut engine, entity),
        None,
        "nothing is decoded while the family is off"
    );
    assert_eq!(log.calls.load(Ordering::SeqCst), 0, "no decode ran");

    activation
        .activate(LOADING)
        .expect("the loading family is known");
    frames_until(
        &mut engine,
        "the mesh to resolve once the family is on",
        |engine| resolved_vertices(engine, entity).is_some(),
    );
}

/// A tool: it hands the engine its asset service in `provide`, as the
/// editor does (`EditorApp::provide`).
struct Tool {
    assets: Option<Arc<Mutex<AssetService>>>,
}

impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool { assets: None }
    }
    fn provide(&self, runtime: &mut Runtime) {
        if let Some(assets) = &self.assets {
            runtime.services.insert(Arc::clone(assets));
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **A mesh asset resolves when the application hands the engine its asset
/// service in `provide`**, after the engine's own entries were
/// installed — the editor's way: the `loading` family decodes from that service.
#[test]
fn a_mesh_asset_resolves_when_the_service_is_handed_over_at_registration() {
    let project = Project::with_mesh("registered", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(
        Tool {
            assets: Some(project.service(&log)),
        },
        Runtime::new(),
    );
    let runtime = Arc::clone(engine.runtime());
    let entity = engine
        .game_world_mut()
        .expect("bootstrapped")
        .spawn(MeshRef::Asset(mesh_uuid()));

    for _ in 0..MAX_FRAMES {
        let inputs = engine.drain_inputs();
        engine.run_app_update(&inputs);
        let presents = engine.begin_render_frame(&runtime);
        engine.run_scheduler(&runtime);
        engine.end_render_frame(presents);
        engine.run_maintenance();
        engine.close_frame();
        let resolved = engine.game_world_mut().and_then(|world| {
            world
                .inner_world()
                .get::<HandleComponent<Mesh>>(entity)
                .map(|mesh| mesh.handle.positions.len())
        });
        if resolved.is_some() {
            assert_eq!(resolved, Some(3), "the triangle the file holds");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the mesh never resolved within {MAX_FRAMES} frames ({} decodes made)",
        log.calls.load(Ordering::SeqCst)
    );
}

/// **A mesh whose file changes while its decode is in flight never shows
/// its old bytes.** The first decode reads the triangle and is held; the
/// file becomes a quad and the asset is invalidated, as the editor's hot
/// reload does; the held decode is released. The entity resolves to the
/// quad, and no frame ever shows the triangle.
#[test]
fn a_mesh_changed_while_its_decode_is_in_flight_never_installs_the_stale_one() {
    let project = Project::with_mesh("changed-in-flight", TRIANGLE_OBJ);
    let log = DecodeLog::holding_the_first();
    let assets = project.service(&log);
    let mut engine = game_engine(&assets);
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);

    frames_until(&mut engine, "the first decode to start", |_| {
        log.entered.load(Ordering::SeqCst)
    });
    assert_eq!(
        resolved_vertices(&mut engine, entity),
        None,
        "the triangle, still being decoded, is not shown"
    );
    project.save(QUAD_OBJ);
    assets
        .lock()
        .expect("the service's lock")
        .invalidate(&mesh_uuid());
    log.release();

    frames_until(&mut engine, "the quad to resolve", |engine| {
        let now = resolved_vertices(engine, entity);
        assert_ne!(
            now,
            Some(3),
            "the triangle decoded before the change is never shown"
        );
        now == Some(6)
    });
}

// ─── A mesh changed on disk ─────────────────────────────────────────────────

/// **A mesh whose file changes is decoded again off the frame's thread, and
/// its entity shows the last good mesh until the new one lands.** The
/// triangle resolves; the file becomes a quad and the asset is invalidated,
/// as the editor's hot reload does. Every decode made since ran on another
/// thread than the frames', and no frame showed the entity without a mesh.
#[test]
fn a_mesh_changed_on_disk_is_decoded_again_off_the_frame_thread() {
    let project = Project::with_mesh("reloaded-off-thread", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let assets = project.service(&log);
    let mut engine = game_engine(&assets);
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);
    frames_until(&mut engine, "the triangle to resolve", |engine| {
        resolved_vertices(engine, entity) == Some(3)
    });
    let before = log.threads().len();

    project.save(QUAD_OBJ);
    assets
        .lock()
        .expect("the service's lock")
        .invalidate(&mesh_uuid());
    frames_until(&mut engine, "the quad to resolve", |engine| {
        let now = resolved_vertices(engine, entity);
        assert!(
            now == Some(3) || now == Some(6),
            "the entity shows the last good mesh until the new one lands: {now:?}"
        );
        now == Some(6)
    });

    let frame_thread = std::thread::current().id();
    let reloads = log.threads()[before..].to_vec();
    assert!(!reloads.is_empty(), "the changed mesh was decoded again");
    assert!(
        reloads.iter().all(|thread| *thread != frame_thread),
        "no decode of the change ran on the frame's thread: {reloads:?} (frames on \
         {frame_thread:?})"
    );
}

/// **A slow decode of a changed mesh never lengthens a frame**: while the
/// decoder holds the decode of the change, the frames go on at their pace and
/// show the last good mesh; once released, the new mesh lands.
#[test]
fn a_slow_reload_never_lengthens_a_frame_and_the_old_mesh_shows_meanwhile() {
    let project = Project::with_mesh("slow-reload", TRIANGLE_OBJ);
    let log = Arc::new(DecodeLog::default());
    let assets = project.service(&log);
    let mut engine = game_engine(&assets);
    frame(&mut engine);
    let entity = spawn_mesh(&mut engine);
    frames_until(&mut engine, "the triangle to resolve", |engine| {
        resolved_vertices(engine, entity) == Some(3)
    });

    log.hold_the_next();
    project.save(QUAD_OBJ);
    assets
        .lock()
        .expect("the service's lock")
        .invalidate(&mesh_uuid());

    // Every frame from the change, until the held decode was out for five
    // more frames, is timed.
    let mut longest = Duration::ZERO;
    let mut after_entered = 0;
    let mut shown_while_held = Vec::new();
    for _ in 0..MAX_FRAMES {
        let started = Instant::now();
        frame(&mut engine);
        longest = longest.max(started.elapsed());
        if log.entered.load(Ordering::SeqCst) {
            shown_while_held.push(resolved_vertices(&mut engine, entity));
            after_entered += 1;
            if after_entered > 5 {
                break;
            }
        }
    }
    log.release();

    assert!(after_entered > 0, "the held decode started");
    assert!(
        longest < LONG_FRAME,
        "a frame lasted {longest:?} while the decode of the change was held"
    );
    assert!(
        shown_while_held.iter().all(|shown| *shown == Some(3)),
        "the last good triangle shows while the change decodes: {shown_while_held:?}"
    );
    frames_until(&mut engine, "the quad to resolve once released", |engine| {
        resolved_vertices(engine, entity) == Some(6)
    });
}
