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

//! Retirement: a retired GPU copy is freed on the next tick, beyond the
//! orphan budget, and only for its own kind.

use super::*;

/// The retired projection is freed even though entities still name its
/// uuid: they wait for a fresh projection, not for the old resources.
#[test]
fn tick_frees_the_retired_resources() {
    let store = AssetStore::new();
    let mut world = World::new();
    let uuid = AssetUUID::new();
    store
        .store::<GpuMaterial>()
        .write()
        .unwrap()
        .insert(uuid, AssetHandle::new(textured_material(10)));
    store
        .store::<GpuMesh>()
        .write()
        .unwrap()
        .insert(uuid, AssetHandle::new(mesh_with_buffers(20, 21)));
    spawn_material_ref(&mut world, uuid);
    spawn_mesh_ref(&mut world, uuid);
    let mut eviction = AssetEviction::new();
    let device = DestroyRecorder::default();

    eviction.retire_material(&store, &uuid);
    eviction.retire_mesh(&store, &uuid);
    eviction.tick(&store, &world, &device);

    assert_eq!(
        device.buffers(),
        HashSet::from([BufferId(10), BufferId(20), BufferId(21)]),
        "the material's uniform buffer and the mesh's two buffers"
    );
    assert_eq!(device.bind_groups(), vec![BindGroupId(11)]);
    assert_eq!(device.textures(), vec![TextureId(12)]);
    assert_eq!(device.views(), vec![TextureViewId(13)]);
    assert_eq!(eviction.retired_count(), 0, "nothing is freed twice");
    assert_eq!(eviction.last_evicted_count(), 2);
}

#[test]
fn retired_resources_are_freed_beyond_the_orphan_budget() {
    let store = AssetStore::new();
    let world = World::new();
    let mut eviction = AssetEviction::with_budget(1);
    let uuids: Vec<AssetUUID> = (0..3).map(|_| AssetUUID::new()).collect();
    for (i, uuid) in uuids.iter().enumerate() {
        store.store::<GpuMesh>().write().unwrap().insert(
            *uuid,
            AssetHandle::new(mesh_with_buffers(100 + 2 * i, 101 + 2 * i)),
        );
    }
    for uuid in &uuids {
        eviction.retire_mesh(&store, uuid);
    }
    let device = DestroyRecorder::default();

    eviction.tick(&store, &world, &device);

    assert_eq!(
        device.buffer_destroys(),
        6,
        "the three retired meshes are all freed in one tick"
    );
    assert_eq!(eviction.retired_count(), 0);
    assert_eq!(eviction.last_evicted_count(), 3);
}

#[test]
fn retiring_a_mesh_leaves_the_material_of_the_same_uuid() {
    let store = AssetStore::new();
    let uuid = AssetUUID::new();
    cache_mesh(&store, uuid);
    cache_material(&store, uuid);
    let mut eviction = AssetEviction::new();

    eviction.retire_mesh(&store, &uuid);
    eviction.retire_mesh(&store, &AssetUUID::new());

    assert_eq!(mesh_count(&store), 0, "the mesh leaves the store");
    assert_eq!(material_count(&store), 1, "the material stays");
    assert_eq!(eviction.retired_count(), 1);
}

#[test]
fn retiring_a_material_leaves_the_mesh_of_the_same_uuid() {
    let store = AssetStore::new();
    let uuid = AssetUUID::new();
    cache_mesh(&store, uuid);
    cache_material(&store, uuid);
    let mut eviction = AssetEviction::new();

    eviction.retire_material(&store, &uuid);
    eviction.retire_material(&store, &uuid);

    assert_eq!(material_count(&store), 0, "the material leaves the store");
    assert_eq!(mesh_count(&store), 1, "the mesh stays");
    assert_eq!(eviction.retired_count(), 1, "queued once");
}
