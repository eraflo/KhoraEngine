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

//! Tests of the asset resolver, by behaviour; the helpers they share live
//! here: the store's arrivals and copies, and the resolver run as the
//! scheduler runs it.

mod material_textures;
mod reference_forms;
mod ui_assets;
mod watched_references;

use khora_core::asset::{InlineMaterial, ProceduralMesh, StandardMaterial};
use khora_core::math::LinearRgba;
use khora_data::assets::{Arrival, ArrivalKind, ArrivalsRead};
use khora_data::ecs::{HandleComponent, Observer};

use super::*;

/// The arrivals reader these tests read the store as.
const WATCHER: &str = "asset_resolver_tests";

/// Starts reading the arrivals of `Assets<A>`: [`arrived`] returns what lands after this call.
fn watch<A: Stored>(store: &AssetStore) {
    let _ = store.store::<A>().read().unwrap().arrivals(WATCHER);
}

/// The arrivals of `Assets<A>` since the last [`watch`] or call.
fn arrived<A: Stored>(store: &AssetStore) -> Vec<Arrival> {
    match store.store::<A>().read().unwrap().arrivals(WATCHER) {
        ArrivalsRead::Since(arrivals) => arrivals,
        ArrivalsRead::Resync => panic!("the store is watched first"),
    }
}

/// The copy `store` holds under `uuid`, if any.
fn stored<A: Stored>(store: &AssetStore, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
    store.store::<A>().read().unwrap().get(uuid).cloned()
}

/// The uuid `entity`'s `AssetRef<Box<dyn Material>>` names.
fn material_uuid(world: &World, entity: EntityId) -> AssetUUID {
    world
        .get::<AssetRef<Box<dyn Material>>>(entity)
        .expect("the entity has an AssetRef<Box<dyn Material>>")
        .uuid()
}

/// The uuid `entity`'s `AssetRef<Mesh>` names.
fn mesh_uuid(world: &World, entity: EntityId) -> AssetUUID {
    world
        .get::<AssetRef<Mesh>>(entity)
        .expect("the entity has an AssetRef<Mesh>")
        .uuid()
}

/// The `StandardMaterial` `store` holds under `uuid`.
fn stored_standard(store: &AssetStore, uuid: &AssetUUID) -> StandardMaterial {
    let material = stored::<Box<dyn Material>>(store, uuid).expect("the material is stored");
    let material: &dyn Material = &**material;
    material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .expect("a StandardMaterial")
        .clone()
}

/// Runs the registered data system `name` once, as the scheduler's runner does.
///
/// It is handed the tick of its last run; its writes carry this one.
fn run_system(world: &mut World, runtime: &Runtime, name: &'static str) {
    let observer = Observer::DataSystem(name);
    let ctx = SystemContext {
        since: world.observer_last_seen(observer),
        resumed: world.observer_resumes(observer),
        scope: None,
    };
    let tick = world.begin_run();
    let system = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"));
    (system.run)(world, runtime, &mut OutputDeck::default(), ctx);
    world.note_observer_run(observer, tick);
    world.end_run();
}

/// Runs the resolver once, as the scheduler's runner does.
fn resolve_once(world: &mut World, runtime: &Runtime) {
    run_system(world, runtime, ASSET_RESOLVER);
}

/// Runs frames of the resolver until what it asked for is decoded and taken in.
fn resolve_settled(world: &mut World, runtime: &Runtime) {
    crate::systems::asset_intake::tests::settle(runtime, world, |world| {
        resolve_once(world, runtime);
    });
}

/// A runtime holding `store`.
fn runtime_with(store: &AssetStore) -> Runtime {
    let mut runtime = Runtime::default();
    runtime.resources.insert(store.clone());
    runtime
}
