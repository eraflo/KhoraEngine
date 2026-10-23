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
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::GraphicsDevice;

use crate::assets::AssetStore;
use crate::ecs::{HandleComponent, World};

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
    /// GPU meshes taken out of the cache while still referenced (their
    /// asset changed), freed at the next tick.
    retired_meshes: Vec<AssetHandle<GpuMesh>>,
    /// GPU materials taken out of the cache while still referenced, freed
    /// at the next tick.
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

    /// Runs one frame of eviction: frees everything [`retire`](Self::retire)d
    /// since the last tick, then shares `max_per_frame` across orphaned
    /// materials then meshes. Orphans beyond the budget stay cached for the
    /// next frame — harmless, since an orphaned entry is simply not yet
    /// reclaimed. Retired entries are outside the budget: they were named,
    /// not discovered, and are already out of the cache.
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

    /// Takes the GPU projection of `uuid` out of the `GpuMesh` and
    /// `GpuMaterial` caches and queues whatever was there for teardown.
    ///
    /// The projection re-projects any entity left without its GPU handle;
    /// the old resources are freed at the next [`tick`](Self::tick), after
    /// the render that may still read them.
    pub fn retire(&mut self, store: &AssetStore, uuid: &AssetUUID) {
        if let Some(mesh) = store
            .store::<GpuMesh>()
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(uuid)
        {
            self.retired_meshes.push(mesh);
        }
        if let Some(material) = store
            .store::<GpuMaterial>()
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(uuid)
        {
            self.retired_materials.push(material);
        }
    }

    /// Number of retired GPU assets waiting for the next [`tick`](Self::tick).
    pub fn retired_count(&self) -> usize {
        self.retired_meshes.len() + self.retired_materials.len()
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
    orphans
        .iter()
        .filter_map(|uuid| guard.remove(uuid))
        .collect()
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
    use khora_core::renderer::api::command::BindGroupId;
    use khora_core::renderer::api::pipeline::PrimitiveTopology;
    use khora_core::renderer::api::resource::IndexFormat;
    use khora_core::renderer::api::resource::{BufferId, SamplerId};
    use khora_core::renderer::api::shader::ShaderVariantKey;

    /// A cache-only `GpuMaterial` stub (no real GPU resources) — enough to
    /// exercise the device-free selection/removal logic.
    fn material_stub() -> GpuMaterial {
        GpuMaterial {
            uniform_buffer: BufferId(0),
            base_color_view: None,
            metallic_roughness_view: None,
            normal_view: None,
            emissive_view: None,
            occlusion_view: None,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            occlusion_texture: None,
            specular_view: None,
            specular_color_view: None,
            specular_texture: None,
            specular_color_texture: None,
            sampler: SamplerId(0),
            bind_group: BindGroupId(0),
            variant: ShaderVariantKey::empty(),
            double_sided: false,
            blend: false,
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
        use crate::gpu::tests::device::RecordingDevice;
        use khora_core::renderer::api::resource::{TextureId, TextureViewId};

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

    // ── Retirement ──────────────────────────────────────────────────────────

    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
    };
    use khora_core::renderer::api::device::GraphicsAdapterInfo;
    use khora_core::renderer::api::pipeline::{
        ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
        RenderPipelineDescriptor, RenderPipelineId,
    };
    use khora_core::renderer::api::resource::{
        BufferDescriptor, SamplerDescriptor, TextureDescriptor, TextureFormat, TextureId,
        TextureViewDescriptor, TextureViewId,
    };
    use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
    use khora_core::renderer::error::ResourceError;
    use khora_core::renderer::traits::CommandEncoder;
    use std::sync::Mutex;

    /// A device that only records what it was asked to destroy.
    #[derive(Debug, Default)]
    struct DestroyRecorder {
        buffers: Mutex<Vec<BufferId>>,
        textures: Mutex<Vec<TextureId>>,
        views: Mutex<Vec<TextureViewId>>,
        bind_groups: Mutex<Vec<BindGroupId>>,
    }

    impl DestroyRecorder {
        fn buffers(&self) -> HashSet<BufferId> {
            self.buffers.lock().unwrap().iter().copied().collect()
        }
        fn buffer_destroys(&self) -> usize {
            self.buffers.lock().unwrap().len()
        }
        fn textures(&self) -> Vec<TextureId> {
            self.textures.lock().unwrap().clone()
        }
        fn views(&self) -> Vec<TextureViewId> {
            self.views.lock().unwrap().clone()
        }
        fn bind_groups(&self) -> Vec<BindGroupId> {
            self.bind_groups.lock().unwrap().clone()
        }
    }

    impl GraphicsDevice for DestroyRecorder {
        fn create_shader_module(
            &self,
            _d: &ShaderModuleDescriptor,
        ) -> Result<ShaderModuleId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
            unreachable!("eviction frees no shader")
        }
        fn create_render_pipeline(
            &self,
            _d: &RenderPipelineDescriptor,
        ) -> Result<RenderPipelineId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn create_pipeline_layout(
            &self,
            _d: &PipelineLayoutDescriptor,
        ) -> Result<PipelineLayoutId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
            unreachable!("eviction frees no pipeline")
        }
        fn create_compute_pipeline(
            &self,
            _d: &ComputePipelineDescriptor,
        ) -> Result<ComputePipelineId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
            unreachable!("eviction frees no pipeline")
        }
        fn create_bind_group_layout(
            &self,
            _d: &BindGroupLayoutDescriptor,
        ) -> Result<BindGroupLayoutId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn create_bind_group(
            &self,
            _d: &BindGroupDescriptor,
        ) -> Result<BindGroupId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
            unreachable!("eviction frees no layout")
        }
        fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
            self.bind_groups.lock().unwrap().push(id);
            Ok(())
        }
        fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn create_buffer_with_data(
            &self,
            _d: &BufferDescriptor,
            _data: &[u8],
        ) -> Result<BufferId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
            self.buffers.lock().unwrap().push(id);
            Ok(())
        }
        fn write_buffer(
            &self,
            _id: BufferId,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), ResourceError> {
            unreachable!("eviction writes nothing")
        }
        fn write_buffer_async<'a>(
            &'a self,
            _id: BufferId,
            _offset: u64,
            _data: &'a [u8],
        ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static>
        {
            unreachable!("eviction writes nothing")
        }
        fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
            self.textures.lock().unwrap().push(id);
            Ok(())
        }
        fn write_texture(
            &self,
            _id: TextureId,
            _data: &[u8],
            _bytes_per_row: Option<u32>,
            _offset: khora_core::math::dimension::Origin3D,
            _size: khora_core::math::dimension::Extent3D,
        ) -> Result<(), ResourceError> {
            unreachable!("eviction writes nothing")
        }
        fn create_texture_view(
            &self,
            _id: TextureId,
            _d: &TextureViewDescriptor,
        ) -> Result<TextureViewId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
            self.views.lock().unwrap().push(id);
            Ok(())
        }
        fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
            unreachable!("eviction creates nothing")
        }
        fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
            unreachable!("the shared sampler is never freed by eviction")
        }
        fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
            unreachable!("eviction records no commands")
        }
        fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
            unreachable!("eviction submits nothing")
        }
        fn get_surface_format(&self) -> Option<TextureFormat> {
            unreachable!("eviction reads no surface")
        }
        fn get_surface_size(&self) -> (u32, u32) {
            unreachable!("eviction reads no surface")
        }
        fn get_adapter_info(&self) -> GraphicsAdapterInfo {
            unreachable!("eviction reads no adapter")
        }
        fn supports_feature(&self, _feature_name: &str) -> bool {
            unreachable!("eviction reads no feature")
        }
    }

    /// A material owning a uniform buffer, a bind group and one texture with
    /// its view, each id distinct from every other stub's.
    fn textured_material(base: usize) -> GpuMaterial {
        GpuMaterial {
            uniform_buffer: BufferId(base),
            bind_group: BindGroupId(base + 1),
            base_color_texture: Some(TextureId(base + 2)),
            base_color_view: Some(TextureViewId(base + 3)),
            ..material_stub()
        }
    }

    fn mesh_with_buffers(vertex: usize, index: usize) -> GpuMesh {
        GpuMesh {
            vertex_buffer: BufferId(vertex),
            index_buffer: BufferId(index),
            ..mesh_stub()
        }
    }

    #[test]
    fn retiring_takes_the_projection_out_of_the_caches() {
        let store = AssetStore::new();
        let uuid = AssetUUID::new();
        let other = AssetUUID::new();
        cache_mesh(&store, uuid);
        cache_material(&store, uuid);
        cache_material(&store, other);
        let mut eviction = AssetEviction::new();

        eviction.retire(&store, &uuid);

        assert!(!store.store::<GpuMesh>().read().unwrap().contains(&uuid));
        assert!(!store.store::<GpuMaterial>().read().unwrap().contains(&uuid));
        assert!(
            store
                .store::<GpuMaterial>()
                .read()
                .unwrap()
                .contains(&other),
            "another asset's projection stays cached"
        );
        assert_eq!(eviction.retired_count(), 2, "the mesh and the material");
    }

    #[test]
    fn retiring_an_uncached_uuid_queues_nothing() {
        let store = AssetStore::new();
        let cached = AssetUUID::new();
        cache_mesh(&store, cached);
        let mut eviction = AssetEviction::new();

        eviction.retire(&store, &AssetUUID::new());

        assert_eq!(eviction.retired_count(), 0);
        assert_eq!(mesh_count(&store), 1, "nothing else leaves the cache");
    }

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

        eviction.retire(&store, &uuid);
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
            eviction.retire(&store, uuid);
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
}
