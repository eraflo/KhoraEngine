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
//! `MeshRef` or `MaterialRef` names its uuid, and its resources are freed
//! only by an explicit `destroy_*`.

use std::collections::HashSet;

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::GraphicsDevice;
use khora_core::sync::RwLockExt;

use crate::assets::AssetStore;
use crate::ecs::{MaterialRef, MeshRef, World};

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
trait NamedByReference: Asset {
    /// Returns the uuids the live entities' references name.
    ///
    /// The query skips orphan rows, so a despawned entity not yet compacted
    /// names nothing.
    fn named(world: &World) -> HashSet<AssetUUID>;
}

impl NamedByReference for GpuMesh {
    fn named(world: &World) -> HashSet<AssetUUID> {
        world.query::<&MeshRef>().map(MeshRef::uuid).collect()
    }
}

impl NamedByReference for GpuMaterial {
    fn named(world: &World) -> HashSet<AssetUUID> {
        world
            .query::<&MaterialRef>()
            .map(MaterialRef::uuid)
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

    /// Spawns an entity whose `MaterialRef` names `uuid`, making `uuid` live.
    fn spawn_material_ref(world: &mut World, uuid: AssetUUID) -> khora_core::ecs::entity::EntityId {
        world.spawn(MaterialRef::Asset(uuid))
    }

    /// Spawns an entity whose `MeshRef` names `uuid`, making `uuid` live.
    fn spawn_mesh_ref(world: &mut World, uuid: AssetUUID) {
        world.spawn(MeshRef::Asset(uuid));
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

    // ── Liveness by reference ───────────────────────────────────────────────

    use crate::ecs::{MaterialRef, MeshRef};

    /// Returns whether the GPU store of `A` holds a copy under `uuid`.
    fn resident<A: khora_core::asset::Asset>(store: &AssetStore, uuid: &AssetUUID) -> bool {
        store.store::<A>().read().unwrap().contains(uuid)
    }

    /// **A GPU mesh no reference names is evicted.**
    ///
    /// Kept while an entity's `MeshRef` names its uuid; once that last
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
        let entity = world.spawn(MeshRef::Asset(uuid));
        let device = DestroyRecorder::default();
        let mut eviction = AssetEviction::new();

        eviction.tick(&store, &world, &device);
        assert!(
            resident::<GpuMesh>(&store, &uuid),
            "a GPU mesh a MeshRef names is kept"
        );

        world
            .remove_component::<MeshRef>(entity)
            .expect("the entity holds a MeshRef");
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
        let entity = world.spawn(MaterialRef::Asset(uuid));
        let device = DestroyRecorder::default();
        let mut eviction = AssetEviction::new();

        eviction.tick(&store, &world, &device);
        assert!(
            resident::<GpuMaterial>(&store, &uuid),
            "a GPU material a MaterialRef names is kept"
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
        let gone = world.spawn(MeshRef::Asset(uuid));
        world.spawn(MeshRef::Asset(uuid));
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
    /// Named by an inline `MaterialRef`, whose uuid is its content's.
    #[test]
    fn a_gpu_material_still_named_is_kept() {
        let store = AssetStore::new();
        let mut world = World::new();
        let reference =
            MaterialRef::inline(Box::new(khora_core::asset::StandardMaterial::default()));
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
}
