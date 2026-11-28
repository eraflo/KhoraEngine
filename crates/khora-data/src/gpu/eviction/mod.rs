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

//! [`AssetEviction`]: releases the GPU meshes and materials no reference names.
//!
//! It releases under a per-frame budget. A GPU copy is kept while some
//! `AssetRef<Mesh>` or `AssetRef<Box<dyn Material>>` names its uuid, and its resources are freed
//! only by an explicit `destroy_*`.

use std::collections::HashSet;

use khora_core::asset::{AssetHandle, AssetUUID, Material, Stored};
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::GraphicsDevice;
use khora_core::sync::RwLockExt;

use crate::assets::AssetStore;
use crate::ecs::{AssetRef, World};

/// Default number of orphaned GPU assets released per frame, meshes and
/// materials together.
///
/// Bounded so a burst of despawns spreads its teardown over several frames.
const DEFAULT_MAX_PER_FRAME: usize = 16;

/// The GPU asset caches' maintenance service, the twin of
/// [`EcsMaintenance`](crate::ecs::EcsMaintenance).
///
/// Ticked in [`TickPhase::Maintenance`](crate::ecs::TickPhase::Maintenance),
/// after the frame's render has read the caches; it removes only what no
/// reference names, so it never changes game semantics.
pub struct AssetEviction {
    max_per_frame: usize,
    last_evicted_count: usize,
    /// GPU meshes taken out of the cache while still named (their asset
    /// changed or went), freed at the next tick.
    retired_meshes: Vec<AssetHandle<GpuMesh>>,
    /// GPU materials taken out of the cache while still named, freed at the
    /// next tick.
    retired_materials: Vec<AssetHandle<GpuMaterial>>,
}

impl AssetEviction {
    /// Creates a new eviction service with the default per-frame budget.
    pub fn new() -> Self {
        Self {
            max_per_frame: DEFAULT_MAX_PER_FRAME,
            last_evicted_count: 0,
            retired_meshes: Vec::new(),
            retired_materials: Vec::new(),
        }
    }

    /// Creates a new eviction service with a custom per-frame budget.
    pub fn with_budget(max_per_frame: usize) -> Self {
        Self {
            max_per_frame,
            last_evicted_count: 0,
            retired_meshes: Vec::new(),
            retired_materials: Vec::new(),
        }
    }

    /// Frees every retired copy, then up to `max_per_frame` orphans.
    ///
    /// The retired copies are those the mesh and material residencies took out
    /// of the store because their asset was replaced or went.
    ///
    /// Materials come before meshes; orphans beyond the budget wait for the
    /// next frame. Retired entries are outside the budget: they are already
    /// out of the cache.
    pub fn tick(&mut self, store: &AssetStore, world: &World, device: &dyn GraphicsDevice) {
        self.last_evicted_count = 0;
        for handle in self.retired_materials.drain(..) {
            destroy_gpu_material(&handle, device);
            self.last_evicted_count += 1;
        }
        for handle in self.retired_meshes.drain(..) {
            destroy_gpu_mesh(&handle, device);
            self.last_evicted_count += 1;
        }

        let mut remaining = self.max_per_frame;
        if remaining == 0 {
            return;
        }
        let retired = self.last_evicted_count;

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

        if self.last_evicted_count > retired {
            log::trace!(
                "AssetEviction: reclaimed {} orphaned GPU asset(s)",
                self.last_evicted_count - retired
            );
        }
    }

    /// Takes the GPU mesh of `uuid` out of the store and queues it for teardown.
    ///
    /// Its resources are freed at the next [`tick`](Self::tick), after the
    /// render that may still read them. A uuid with no GPU mesh queues nothing.
    pub(crate) fn retire_mesh(&mut self, store: &AssetStore, uuid: &AssetUUID) {
        if let Some(mesh) = store.store::<GpuMesh>().write_recovered().remove(uuid) {
            self.retired_meshes.push(mesh);
        }
    }

    /// Takes the GPU material of `uuid` out of the store and queues it for teardown.
    ///
    /// Its resources are freed at the next [`tick`](Self::tick), after the
    /// render that may still read them. A uuid with no GPU material queues nothing.
    pub(crate) fn retire_material(&mut self, store: &AssetStore, uuid: &AssetUUID) {
        if let Some(material) = store.store::<GpuMaterial>().write_recovered().remove(uuid) {
            self.retired_materials.push(material);
        }
    }

    /// Returns the number of retired GPU assets waiting for the next
    /// [`tick`](Self::tick).
    pub fn retired_count(&self) -> usize {
        self.retired_meshes.len() + self.retired_materials.len()
    }

    /// Returns the number of GPU assets freed by the last [`tick`](Self::tick).
    pub fn last_evicted_count(&self) -> usize {
        self.last_evicted_count
    }

    /// Returns the maximum number of orphans freed per frame.
    pub fn max_per_frame(&self) -> usize {
        self.max_per_frame
    }
}

impl Default for AssetEviction {
    fn default() -> Self {
        Self::new()
    }
}

/// A GPU copy kept while a reference component names its uuid.
trait NamedByReference: Stored {
    /// Returns the uuids the live entities' references name.
    ///
    /// The query skips orphan rows, so a despawned entity not yet compacted
    /// names nothing.
    fn named(world: &World) -> HashSet<AssetUUID>;
}

impl NamedByReference for GpuMesh {
    fn named(world: &World) -> HashSet<AssetUUID> {
        world
            .query::<&AssetRef<Mesh>>()
            .map(AssetRef::<Mesh>::uuid)
            .collect()
    }
}

impl NamedByReference for GpuMaterial {
    fn named(world: &World) -> HashSet<AssetUUID> {
        world
            .query::<&AssetRef<Box<dyn Material>>>()
            .map(AssetRef::<Box<dyn Material>>::uuid)
            .collect()
    }
}

/// Removes up to `budget` entries of `A` no reference names, and returns them
/// for the caller to destroy.
fn take_orphans<A: NamedByReference>(
    store: &AssetStore,
    world: &World,
    budget: usize,
) -> Vec<AssetHandle<A>> {
    let live = A::named(world);
    let cache = store.store::<A>();

    // Snapshot the orphaned keys under a read lock, then drop it before taking
    // the write lock — no lock is held across the device `destroy_*` calls.
    let orphans: Vec<AssetUUID> = {
        let guard = cache.read_recovered();
        guard
            .keys()
            .filter(|uuid| !live.contains(uuid))
            .take(budget)
            .collect()
    };
    if orphans.is_empty() {
        return Vec::new();
    }

    let mut guard = cache.write_recovered();
    orphans
        .iter()
        .filter_map(|uuid| guard.remove(uuid))
        .collect()
}

/// Frees the GPU resources a [`GpuMaterial`] owns alone; errors are logged.
///
/// Its sampler is the engine's shared one, and is left intact.
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
        material.occlusion_view,
        material.specular_view,
        material.specular_color_view,
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
        material.occlusion_texture,
        material.specular_texture,
        material.specular_color_texture,
    ]
    .into_iter()
    .flatten()
    {
        if let Err(e) = device.destroy_texture(texture) {
            log::warn!("AssetEviction: destroy_texture failed: {e:?}");
        }
    }
}

/// Frees a [`GpuMesh`]'s vertex and index buffers; errors are logged.
///
/// An unindexed mesh still holds a real, empty index buffer.
fn destroy_gpu_mesh(mesh: &GpuMesh, device: &dyn GraphicsDevice) {
    if let Err(e) = device.destroy_buffer(mesh.vertex_buffer) {
        log::warn!("AssetEviction: destroy_buffer (mesh vertex) failed: {e:?}");
    }
    if let Err(e) = device.destroy_buffer(mesh.index_buffer) {
        log::warn!("AssetEviction: destroy_buffer (mesh index) failed: {e:?}");
    }
}

#[cfg(test)]
mod tests;
