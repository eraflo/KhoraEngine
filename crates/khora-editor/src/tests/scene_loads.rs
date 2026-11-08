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

//! The engine's side of a scene load, played by hand for the editor's
//! tests: the scene load hub installed in a runtime, the `loading` family's
//! run, and the boundary's `scene_intake` that commits what it prepared.

use std::sync::Arc;

use khora_sdk::khora_core::event::Channel;
use khora_sdk::khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_sdk::khora_data::ecs::{DataSystemRegistration, SystemContext, Tick};
use khora_sdk::khora_data::scene::loading::{
    LoadKind, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_sdk::khora_data::scene::{NoPrefabs, PrefabSource};
use khora_sdk::{install_scene_loading, GameWorld, LoadOutcome, Runtime, SceneLoadHub};

/// How many frames a load may take to land here: the family's run, then the
/// boundary that commits it, with room to spare.
pub(crate) const SETTLE_FRAMES: usize = 8;

/// Installs in `runtime` what the engine holds for scene loads: the reserve
/// the background stages into, and the scene load hub.
pub(crate) fn install(runtime: &mut Runtime) {
    if runtime.resources.get::<Reserve>().is_none() {
        runtime.resources.insert(Reserve::new());
    }
    install_scene_loading(runtime);
}

/// A runtime holding only what scene loads need.
pub(crate) fn runtime() -> Runtime {
    let mut runtime = Runtime::new();
    install(&mut runtime);
    runtime
}

/// The runtime's scene load hub.
pub(crate) fn hub(runtime: &Runtime) -> Arc<SceneLoadHub> {
    runtime
        .resources
        .get::<Arc<SceneLoadHub>>()
        .cloned()
        .expect("the runtime holds the scene load hub")
}

/// The boundary's `scene_intake`: commits what the background staged.
pub(crate) fn boundary(runtime: &Runtime, world: &mut GameWorld) {
    let intake = khora_sdk::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "scene_intake")
        .expect("`scene_intake` is registered");
    (intake.run)(
        world.inner_world_mut(),
        runtime,
        &mut OutputDeck::new(),
        SystemContext {
            since: Tick(0),
            resumed: false,
            scope: None,
        },
    );
}

/// One run of the `loading` family: the requests filed, prepared through
/// the runtime's `SceneLoading`, each staged under its lease and received
/// by the reserve, as the next frame start receives them.
pub(crate) fn background(runtime: &Runtime) {
    let requests = runtime
        .resources
        .get::<Channel<SceneLoadRequest>>()
        .cloned()
        .expect("the runtime holds the scene load requests");
    let loading = runtime
        .resources
        .get::<Arc<dyn SceneLoading>>()
        .cloned()
        .expect("the runtime holds the scene loading");
    let reserve = runtime
        .resources
        .get::<Reserve>()
        .cloned()
        .expect("the runtime holds the reserve");
    let mut staging = Staging::new();
    for request in requests.drain() {
        let (prepared, lease) = loading.prepare_leased(request);
        match lease {
            Some(lease) => staging.stage_while(vec![prepared], lease),
            None => staging.stage(vec![prepared]),
        }
    }
    reserve.receive(staging);
}

/// Gives `world` `runtime`'s hub, as the engine gives the world it runs at
/// boot: the editor files and withdraws its loads through the world's hub.
pub(crate) fn wire(runtime: &Runtime, world: &mut GameWorld) {
    world.set_scene_loads(hub(runtime));
}

/// A world the engine runs over `runtime`: empty, and holding its hub.
pub(crate) fn engine_world(runtime: &Runtime) -> GameWorld {
    let mut world = GameWorld::new();
    wire(runtime, &mut world);
    world
}

/// The engine's side of one frame: the boundary commits what landed, then
/// the background prepares what was filed.
pub(crate) fn engine_frame(runtime: &Runtime, world: &mut GameWorld) {
    boundary(runtime, world);
    background(runtime);
}

/// Files `kind` into `world` through `runtime`'s hub, and plays frames until
/// its outcome lands: the outcome. Panics after [`SETTLE_FRAMES`] frames.
pub(crate) fn load(
    runtime: &Runtime,
    world: &mut GameWorld,
    kind: LoadKind,
    prefabs: Arc<dyn PrefabSource + Send + Sync>,
) -> LoadOutcome {
    wire(runtime, world);
    let hub = hub(runtime);
    let ticket = hub.file(world.inner_world_mut(), kind, prefabs);
    for _ in 0..SETTLE_FRAMES {
        engine_frame(runtime, world);
        if let Some(outcome) = hub.take_outcome(ticket) {
            return outcome;
        }
    }
    panic!("the scene load did not land within {SETTLE_FRAMES} frames");
}

/// Restores `world` from the Play snapshot `snapshot` as Stop does: a
/// replace filed with the hub, prepared off the frame and committed at a
/// boundary. Panics when the restore fails.
pub(crate) fn restore(world: &mut GameWorld, snapshot: &[u8]) {
    let runtime = runtime();
    let outcome = load(
        &runtime,
        world,
        LoadKind::Replace(SceneSource::Bytes(Arc::from(snapshot))),
        Arc::new(NoPrefabs),
    );
    if let Err(error) = outcome {
        panic!("the restore failed: {error}");
    }
}
