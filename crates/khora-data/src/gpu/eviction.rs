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

//! GPU asset-cache eviction — budgeted reclamation of orphaned GPU resources.
//!
//! The CPU→GPU projection ([`ProjectionRegistry`](crate::gpu::ProjectionRegistry))
//! is insert-only: it uploads a `GpuMesh` / `GpuMaterial` the first time an
//! entity needs one, tags the entity, and never revisits it. Despawning an
//! entity, or editing a material inline (which mints a fresh asset UUID),
//! therefore leaves the old cache entry — and the wgpu buffers, textures,
//! views and bind group it owns — alive forever. `AssetHandle` is a plain
//! `Arc`: dropping the last clone frees the *struct*, but its fields are
//! `Copy` id handles into backend slotmaps that only an explicit `destroy_*`
//! releases. So the cache grew without bound.
//!
//! [`AssetEviction`] is the Data-layer self-maintenance that closes the leak,
//! the GPU-asset twin of [`EcsMaintenance`](crate::ecs::EcsMaintenance). Each
//! frame, in [`TickPhase::Maintenance`](crate::ecs::TickPhase::Maintenance), it
//! diffs the cached UUID set against the set still referenced by live entities
//! and, under a per-frame budget, removes the orphans and destroys their GPU
//! resources. It only ever removes entries no live entity references, so it
//! changes representation, never game semantics (adapt the HOW, not the WHAT).
//!
//! Maintenance runs at the end of the tick, after the render descent has read
//! the caches for this frame, so freeing an orphan is safe: nothing projects a
//! despawned entity next frame, and the ECS query already skips orphan rows so
//! a not-yet-compacted despawned row never keeps a UUID artificially live.

use std::collections::HashSet;

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::renderer::api::scene::{GpuMaterial, GpuMesh};
use khora_core::renderer::GraphicsDevice;

use crate::ecs::{HandleComponent, World};
use crate::gpu::AssetStore;

/// Default number of orphaned GPU assets reclaimed per frame, shared across
/// meshes and materials. Bounded like [`EcsMaintenance`] so a burst of
/// despawns spreads its teardown cost over several frames.
const DEFAULT_MAX_PER_FRAME: usize = 16;

/// Direct GPU asset-cache maintenance service.
///
/// Evicts up to `max_per_frame` orphaned GPU asset entries each frame,
/// destroying their backing wgpu resources. Mirrors
/// [`EcsMaintenance`](crate::ecs::EcsMaintenance) for the GPU asset store:
/// budgeted, idempotent, and Data-owned.
pub struct AssetEviction {
    max_per_frame: usize,
    last_evicted_count: usize,
}

impl AssetEviction {
    /// Creates a new eviction service with the default per-frame budget.
    pub fn new() -> Self {
        Self {
            max_per_frame: DEFAULT_MAX_PER_FRAME,
            last_evicted_count: 0,
        }
    }

    /// Creates a new eviction service with a custom per-frame budget.
    pub fn with_budget(max_per_frame: usize) -> Self {
        Self {
            max_per_frame,
            last_evicted_count: 0,
        }
    }

    /// Runs one frame of eviction, sharing `max_per_frame` across materials
    /// then meshes. Orphans beyond the budget stay cached for the next frame —
    /// harmless, since an orphaned entry is simply not yet reclaimed.
    pub fn tick(&mut self, store: &AssetStore, world: &World, device: &dyn GraphicsDevice) {
        self.last_evicted_count = 0;
        let mut remaining = self.max_per_frame;
        if remaining == 0 {
            return;
        }

        let materials = take_orphans::<GpuMaterial>(store, world, remaining);
        for handle in &materials {
            destroy_gpu_material(handle, device);
        }
        remaining -= materials.len();
        self.last_evicted_count += materials.len();

        if remaining > 0 {
            let meshes = take_orphans::<GpuMesh>(store, world, remaining);
            for handle in &meshes {
                destroy_gpu_mesh(handle, device);
            }
            self.last_evicted_count += meshes.len();
        }

        if self.last_evicted_count > 0 {
            log::trace!(
                "AssetEviction: reclaimed {} orphaned GPU asset(s)",
                self.last_evicted_count
            );
        }
    }

    /// Number of GPU assets evicted in the last [`tick`](Self::tick).
    pub fn last_evicted_count(&self) -> usize {
        self.last_evicted_count
    }

    /// The maximum number of GPU assets evicted per frame.
    pub fn max_per_frame(&self) -> usize {
        self.max_per_frame
    }
}

impl Default for AssetEviction {
    fn default() -> Self {
        Self::new()
    }
}

/// The set of asset UUIDs still referenced by a live entity's
/// `HandleComponent<A>`. The ECS query skips orphan rows, so a despawned
/// entity whose row has not yet been compacted does not keep its UUID live.
fn live_handles<A: khora_core::asset::Asset>(world: &World) -> HashSet<AssetUUID> {
    let mut live = HashSet::new();
    for (handle,) in world.query::<(&HandleComponent<A>,)>() {
        live.insert(handle.uuid);
    }
    live
}

/// Removes up to `budget` orphaned entries of type `A` from the store and
/// returns their handles for teardown. Device-free — the removal *is* the leak
/// fix; the caller destroys the returned resources. An entry is orphaned when
/// no live entity references its UUID.
fn take_orphans<A: khora_core::asset::Asset>(
    store: &AssetStore,
    world: &World,
    budget: usize,
) -> Vec<AssetHandle<A>> {
    let live = live_handles::<A>(world);
    let cache = store.store::<A>();

    // Snapshot the orphaned keys under a read lock, then drop it before taking
    // the write lock — no lock is held across the device `destroy_*` calls.
    let orphans: Vec<AssetUUID> = {
        let guard = cache.read().unwrap_or_else(|e| e.into_inner());
        guard
            .keys()
            .filter(|uuid| !live.contains(uuid))
            .take(budget)
            .collect()
    };
    if orphans.is_empty() {
        return Vec::new();
    }

    let mut guard = cache.write().unwrap_or_else(|e| e.into_inner());
    orphans.iter().filter_map(|uuid| guard.remove(uuid)).collect()
}

/// Frees the wgpu resources a [`GpuMaterial`] exclusively owns: its bind group,
/// uniform buffer, and each declared texture with its view. The `sampler` is
/// the engine-shared filtering sampler and is deliberately left intact.
/// Teardown errors are logged, never fatal (a stale id is harmless).
fn destroy_gpu_material(material: &GpuMaterial, device: &dyn GraphicsDevice) {
    if let Err(e) = device.destroy_bind_group(material.bind_group) {
        log::warn!("AssetEviction: destroy_bind_group failed: {e:?}");
    }
    if let Err(e) = device.destroy_buffer(material.uniform_buffer) {
        log::warn!("AssetEviction: destroy_buffer (material uniform) failed: {e:?}");
    }
    for view in [
        material.base_color_view,
        material.metallic_roughness_view,
        material.normal_view,
        material.emissive_view,
    ]
    .into_iter()
    .flatten()
    {
        if let Err(e) = device.destroy_texture_view(view) {
            log::warn!("AssetEviction: destroy_texture_view failed: {e:?}");
        }
    }
    for texture in [
        material.base_color_texture,
        material.metallic_roughness_texture,
        material.normal_texture,
        material.emissive_texture,
    ]
    .into_iter()
    .flatten()
    {
        if let Err(e) = device.destroy_texture(texture) {
            log::warn!("AssetEviction: destroy_texture failed: {e:?}");
        }
    }
}

/// Frees the wgpu resources a [`GpuMesh`] owns: its vertex and index buffers.
/// A non-indexed mesh still holds a real (zero-sized) index buffer, so both are
/// always destroyed. Teardown errors are logged, never fatal.
fn destroy_gpu_mesh(mesh: &GpuMesh, device: &dyn GraphicsDevice) {
    if let Err(e) = device.destroy_buffer(mesh.vertex_buffer) {
        log::warn!("AssetEviction: destroy_buffer (mesh vertex) failed: {e:?}");
    }
    if let Err(e) = device.destroy_buffer(mesh.index_buffer) {
        log::warn!("AssetEviction: destroy_buffer (mesh index) failed: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::renderer::api::pipeline::{PrimitiveTopology, ShaderVariantKey};
    use khora_core::renderer::api::resource::{BufferId, SamplerId};
    use khora_core::renderer::api::command::BindGroupId;
    use khora_core::renderer::api::util::IndexFormat;

    /// A cache-only `GpuMaterial` stub (no real GPU resources) — enough to
    /// exercise the device-free selection/removal logic.
    fn material_stub() -> GpuMaterial {
        GpuMaterial {
            uniform_buffer: BufferId(0),
            base_color_view: None,
            metallic_roughness_view: None,
            normal_view: None,
            emissive_view: None,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            sampler: SamplerId(0),
            bind_group: BindGroupId(0),
            variant: ShaderVariantKey::empty(),
        }
    }

    fn mesh_stub() -> GpuMesh {
        GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(0),
            index_count: 0,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }
    }

    /// Inserts a material into the store under `uuid` and returns nothing;
    /// the entity (if any) is spawned separately so we control liveness.
    fn cache_material(store: &AssetStore, uuid: AssetUUID) {
        store
            .store::<GpuMaterial>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(material_stub()));
    }

    fn cache_mesh(store: &AssetStore, uuid: AssetUUID) {
        store
            .store::<GpuMesh>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(mesh_stub()));
    }

    fn material_count(store: &AssetStore) -> usize {
        store.store::<GpuMaterial>().read().unwrap().len()
    }

    fn mesh_count(store: &AssetStore) -> usize {
        store.store::<GpuMesh>().read().unwrap().len()
    }

    /// Spawns an entity tagged with `HandleComponent<GpuMaterial>` for `uuid`,
    /// making `uuid` a live reference.
    fn spawn_material_ref(world: &mut World, uuid: AssetUUID) {
        world.spawn(HandleComponent {
            handle: AssetHandle::new(material_stub()),
            uuid,
        });
    }

    fn spawn_mesh_ref(world: &mut World, uuid: AssetUUID) {
        world.spawn(HandleComponent {
            handle: AssetHandle::new(mesh_stub()),
            uuid,
        });
    }

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
        // no longer live and becomes evictable.
        let entity = world.spawn(HandleComponent {
            handle: AssetHandle::new(material_stub()),
            uuid,
        });
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
        assert_eq!(material_count(&store), 3, "leftover orphans stay for next frame");
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
}
