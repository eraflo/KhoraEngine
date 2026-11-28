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

//! Orphans: a GPU copy no entity names is evicted, within the budget of a
//! call.

use super::*;

#[test]
fn keeps_referenced_material_evicts_orphan() {
    let store = AssetStore::new();
    let mut world = World::new();

    let live = AssetUUID::new();
    let orphan = AssetUUID::new();
    cache_material(&store, live);
    cache_material(&store, orphan);
    spawn_material_ref(&mut world, live);

    let removed = take_orphans::<GpuMaterial>(&store, &world, 16);
    assert_eq!(removed.len(), 1, "exactly the orphan is removed");
    assert_eq!(material_count(&store), 1, "cache no longer grows unbounded");
    assert!(
        store.store::<GpuMaterial>().read().unwrap().contains(&live),
        "the referenced material stays cached"
    );
}

#[test]
fn nothing_evicted_when_all_referenced() {
    let store = AssetStore::new();
    let mut world = World::new();
    let a = AssetUUID::new();
    let b = AssetUUID::new();
    cache_material(&store, a);
    cache_material(&store, b);
    spawn_material_ref(&mut world, a);
    spawn_material_ref(&mut world, b);

    let removed = take_orphans::<GpuMaterial>(&store, &world, 16);
    assert!(removed.is_empty());
    assert_eq!(material_count(&store), 2);
}

#[test]
fn despawn_makes_material_evictable() {
    let store = AssetStore::new();
    let mut world = World::new();
    let uuid = AssetUUID::new();
    cache_material(&store, uuid);

    // Spawn then despawn: the query skips the orphaned row, so the UUID is
    // out of use and becomes evictable.
    let entity = spawn_material_ref(&mut world, uuid);
    assert_eq!(take_orphans::<GpuMaterial>(&store, &world, 16).len(), 0);

    world.despawn(entity);
    let removed = take_orphans::<GpuMaterial>(&store, &world, 16);
    assert_eq!(removed.len(), 1);
    assert_eq!(material_count(&store), 0);
}

#[test]
fn budget_bounds_evictions_per_call() {
    let store = AssetStore::new();
    let world = World::new();
    for _ in 0..5 {
        cache_material(&store, AssetUUID::new());
    }
    // No live refs → all 5 are orphans, but the budget caps the batch.
    let removed = take_orphans::<GpuMaterial>(&store, &world, 2);
    assert_eq!(removed.len(), 2);
    assert_eq!(
        material_count(&store),
        3,
        "leftover orphans stay for next frame"
    );
}

#[test]
fn evicts_orphaned_mesh_and_respects_liveness() {
    let store = AssetStore::new();
    let mut world = World::new();
    let live = AssetUUID::new();
    let orphan = AssetUUID::new();
    cache_mesh(&store, live);
    cache_mesh(&store, orphan);
    spawn_mesh_ref(&mut world, live);

    let removed = take_orphans::<GpuMesh>(&store, &world, 16);
    assert_eq!(removed.len(), 1);
    assert_eq!(mesh_count(&store), 1);
    assert!(store.store::<GpuMesh>().read().unwrap().contains(&live));
}

#[test]
fn evicting_a_material_frees_its_specular_maps() {
    use khora_core::renderer::api::resource::{TextureId, TextureViewId};

    use crate::gpu::tests::device::RecordingDevice;

    let store = AssetStore::new();
    let world = World::new();
    store.store::<GpuMaterial>().write().unwrap().insert(
        AssetUUID::new(),
        AssetHandle::new(GpuMaterial {
            specular_view: Some(TextureViewId(71)),
            specular_color_view: Some(TextureViewId(72)),
            specular_texture: Some(TextureId(81)),
            specular_color_texture: Some(TextureId(82)),
            ..material_stub()
        }),
    );
    let device = RecordingDevice::default();

    AssetEviction::new().tick(&store, &world, &device);

    assert_eq!(material_count(&store), 0, "the orphan is evicted");
    let views = device.destroyed_views();
    let textures = device.destroyed_textures();
    for view in [TextureViewId(71), TextureViewId(72)] {
        assert!(views.contains(&view), "{view:?} freed: {views:?}");
    }
    for texture in [TextureId(81), TextureId(82)] {
        assert!(
            textures.contains(&texture),
            "{texture:?} freed: {textures:?}"
        );
    }
}
