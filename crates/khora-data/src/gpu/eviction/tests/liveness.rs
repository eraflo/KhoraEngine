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

//! Liveness by reference: a GPU copy is kept while a reference names it,
//! and evicted once none does.

use super::*;
use crate::ecs::AssetRef;

/// Returns whether the GPU store of `A` holds a copy under `uuid`.
fn resident<A: khora_core::asset::Stored>(store: &AssetStore, uuid: &AssetUUID) -> bool {
    store.store::<A>().read().unwrap().contains(uuid)
}

/// **A GPU mesh no reference names is evicted.**
///
/// Kept while an entity's `AssetRef<Mesh>` names its uuid; once that last
/// reference is removed, the next tick frees it.
#[test]
fn a_gpu_copy_no_reference_names_is_evicted() {
    let store = AssetStore::new();
    let mut world = World::new();
    let uuid = AssetUUID::new();
    store
        .store::<GpuMesh>()
        .write()
        .unwrap()
        .insert(uuid, AssetHandle::new(mesh_with_buffers(30, 31)));
    let entity = world.spawn(AssetRef::<Mesh>::Asset(uuid));
    let device = DestroyRecorder::default();
    let mut eviction = AssetEviction::new();

    eviction.tick(&store, &world, &device);
    assert!(
        resident::<GpuMesh>(&store, &uuid),
        "a GPU mesh an AssetRef<Mesh> names is kept"
    );

    world
        .remove_component::<AssetRef<Mesh>>(entity)
        .expect("the entity holds an AssetRef<Mesh>");
    eviction.tick(&store, &world, &device);
    assert!(
        !resident::<GpuMesh>(&store, &uuid),
        "no reference names it any more: it is evicted"
    );
    assert_eq!(
        device.buffers(),
        HashSet::from([BufferId(30), BufferId(31)]),
        "its buffers are given back"
    );
}

/// **A GPU material no reference names is evicted.**
#[test]
fn a_gpu_material_no_reference_names_is_evicted() {
    let store = AssetStore::new();
    let mut world = World::new();
    let uuid = AssetUUID::new();
    store
        .store::<GpuMaterial>()
        .write()
        .unwrap()
        .insert(uuid, AssetHandle::new(textured_material(40)));
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(uuid));
    let device = DestroyRecorder::default();
    let mut eviction = AssetEviction::new();

    eviction.tick(&store, &world, &device);
    assert!(
        resident::<GpuMaterial>(&store, &uuid),
        "a GPU material an AssetRef<Box<dyn Material>> names is kept"
    );

    world.despawn(entity);
    eviction.tick(&store, &world, &device);
    assert!(
        !resident::<GpuMaterial>(&store, &uuid),
        "no reference names it any more: it is evicted"
    );
    assert_eq!(device.bind_groups(), vec![BindGroupId(41)]);
}

/// **A GPU mesh still named is kept.**
///
/// One of the two entities naming it goes; the other still names it.
#[test]
fn a_gpu_copy_still_named_is_kept() {
    let store = AssetStore::new();
    let mut world = World::new();
    let uuid = AssetUUID::new();
    cache_mesh(&store, uuid);
    let gone = world.spawn(AssetRef::<Mesh>::Asset(uuid));
    world.spawn(AssetRef::<Mesh>::Asset(uuid));
    world.despawn(gone);
    let device = DestroyRecorder::default();

    AssetEviction::new().tick(&store, &world, &device);

    assert!(
        resident::<GpuMesh>(&store, &uuid),
        "the GPU mesh another entity still names is kept"
    );
    assert_eq!(device.buffer_destroys(), 0, "nothing is given back");
}

/// **A GPU material still named is kept.**
///
/// Named by an inline `AssetRef<Box<dyn Material>>`, whose uuid is its content's.
#[test]
fn a_gpu_material_still_named_is_kept() {
    let store = AssetStore::new();
    let mut world = World::new();
    let reference = AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(
        khora_core::asset::StandardMaterial::default(),
    )));
    let uuid = reference.uuid();
    cache_material(&store, uuid);
    let gone = world.spawn(reference.clone());
    world.spawn(reference);
    world.despawn(gone);
    let device = DestroyRecorder::default();

    AssetEviction::new().tick(&store, &world, &device);

    assert!(
        resident::<GpuMaterial>(&store, &uuid),
        "the GPU material another entity still names is kept"
    );
    assert!(device.bind_groups().is_empty(), "nothing is given back");
}
