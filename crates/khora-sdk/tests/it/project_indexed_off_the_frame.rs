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

//! **A project is indexed off the frame and lands a few frames later, end to
//! end.** The engine boots the `loading` family and its index hub; a host
//! files the open of a project, the family's lane indexes it at the
//! background, and the boundary swaps the new asset service into the
//! engine's. Its assets then resolve through the engine.
//!
//! An index spans frames, so this test waits a bounded number of frames for
//! it, never assumes the frame that files it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_io::asset::{AssetIdRegistry, AssetService, Requested};
use khora_sdk::khora_core::asset::{AssetUUID, IndexKind};
use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, IndexHub, IndexOutcome, InputEvent, PhaseProvider, Runtime,
    WindowConfig,
};
use khora_telemetry::MetricsRegistry;

/// The most frames an index or a decode may take to land: the background
/// run, the frame start that hands it over, the boundary that takes it in.
/// Frames are ~10 ms apart here, so this is seconds — a bound, not the frame
/// that files.
const MAX_FRAMES: usize = 500;

/// One triangle.
const TRIANGLE_OBJ: &str = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";

const MESH: &str = "meshes/hero.obj";

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

/// A project of its own under the system's temporary folder, holding
/// `files` under its `assets/` — removed when dropped.
struct Project(PathBuf);

impl Project {
    fn with(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-indexed-off-the-frame-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(root.join("assets")).expect("the assets folder");
        for (rel, contents) in files {
            let path = root.join("assets").join(rel);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the asset's folder");
            std::fs::write(path, contents).expect("the asset is written");
        }
        Project(root)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    /// The asset service a launcher opens over the project.
    fn service(&self) -> Arc<Mutex<AssetService>> {
        let service = AssetService::open_loose_files(
            &self.0,
            &AssetIdRegistry::load(&self.0),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        Arc::new(Mutex::new(service))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A game engine over `assets`, as a launcher installs its service.
fn engine_serving(assets: &Arc<Mutex<AssetService>>) -> EngineCore<Game> {
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

/// Frames until the index filed as `ticket` lands, at most [`MAX_FRAMES`]:
/// its outcome.
fn frames_until_indexed(
    engine: &mut EngineCore<Game>,
    hub: &IndexHub,
    ticket: khora_sdk::khora_core::asset::IndexTicket,
) -> IndexOutcome {
    for _ in 0..MAX_FRAMES {
        frame(engine);
        if let Some(outcome) = hub.take_outcome(ticket) {
            return outcome;
        }
    }
    panic!("the index did not land within {MAX_FRAMES} frames");
}

/// **The engine installs the index hub**, and a project opened through it
/// lands off the frame. The engine serves an empty project; after boot its
/// runtime holds the `IndexHub`. The open of another project is filed: its
/// ticket lands `Ok` within a bounded number of frames, the engine's own
/// asset service now indexes that project's mesh, and the mesh then
/// resolves — decoded off the frame through the service that moved in.
#[test]
fn the_engine_installs_the_index_hub() {
    let empty = Project::with("empty", &[]);
    let opened = Project::with("opened", &[(MESH, TRIANGLE_OBJ)]);
    let assets = empty.service();
    let mut engine = engine_serving(&assets);
    frame(&mut engine);

    let hub = engine
        .runtime()
        .resources
        .get::<Arc<IndexHub>>()
        .cloned()
        .expect("the engine installs the index hub at boot");
    let ticket = hub.file(opened.root(), IndexKind::Open);
    let outcome = frames_until_indexed(&mut engine, &hub, ticket);

    assert_eq!(outcome, Ok(()), "the open lands");
    let served = engine
        .runtime()
        .services
        .get::<Arc<Mutex<AssetService>>>()
        .cloned()
        .expect("the engine holds an asset service");
    assert!(
        Arc::ptr_eq(&served, &assets),
        "the open moved into the engine's own service"
    );
    let mesh = AssetUUID::new_v5(MESH);
    assert!(
        served
            .lock()
            .expect("the service's lock")
            .index()
            .get_metadata(&mesh)
            .is_some(),
        "the opened project's mesh is indexed"
    );
    let mut resolved = false;
    for _ in 0..MAX_FRAMES {
        let requested = served
            .lock()
            .expect("the service's lock")
            .request::<Mesh>(&mesh);
        match requested {
            Requested::Ready(_) => {
                resolved = true;
                break;
            }
            Requested::Failed => panic!("the opened project's mesh failed to decode"),
            Requested::Pending => frame(&mut engine),
        }
    }
    assert!(
        resolved,
        "the opened project's mesh resolves within {MAX_FRAMES} frames"
    );
}
