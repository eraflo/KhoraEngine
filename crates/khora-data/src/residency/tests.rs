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

//! Where the residencies sit in the tick.

use crate::ecs::{DataSystemRegistration, TickPhase};

/// The registered data system called `name`, if any.
fn data_system(name: &str) -> Option<&'static DataSystemRegistration> {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
}

/// **Residencies run before the agents, after the reload.**
///
/// The UI images, the meshes and the materials are made resident in
/// `PreExtract`, before the Flows project and the agents draw, and after
/// `asset_reload` replaced the copies of whatever changed, so a changed asset
/// reaches the device the frame it changed. The mesh and material residencies
/// keep their registered names, which saves and telemetry know them by.
#[test]
fn residency_runs_before_agents() {
    for name in ["image_on_gpu", "gpu_mesh_sync", "gpu_material_sync"] {
        let Some(residency) = data_system(name) else {
            panic!("the `{name}` residency is not registered");
        };
        assert_eq!(
            residency.phase,
            TickPhase::PreExtract,
            "`{name}` runs before the agents"
        );
        assert!(
            residency.runs_after.contains(&"asset_reload"),
            "`{name}` orders after asset_reload; it runs after {:?}",
            residency.runs_after
        );
    }
}

/// **Materials are made resident after meshes.**
///
/// `gpu_material_sync` orders after `gpu_mesh_sync`, one order every frame;
/// both order after the resolver, which stores this frame's copies of the
/// assets the references name.
#[test]
fn the_material_residency_runs_after_the_mesh_residency() {
    let Some(materials) = data_system("gpu_material_sync") else {
        panic!("the `gpu_material_sync` residency is not registered");
    };
    assert!(
        materials.runs_after.contains(&"gpu_mesh_sync"),
        "`gpu_material_sync` orders after gpu_mesh_sync; it runs after {:?}",
        materials.runs_after
    );
    for name in ["gpu_mesh_sync", "gpu_material_sync"] {
        let Some(residency) = data_system(name) else {
            panic!("the `{name}` residency is not registered");
        };
        assert!(
            residency.runs_after.contains(&"asset_resolver"),
            "`{name}` orders after asset_resolver; it runs after {:?}",
            residency.runs_after
        );
    }
}

mod mesh_on_gpu {
    //! What the mesh residency does with a device that refuses its buffers.

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::math::dimension::{Extent3D, Origin3D};
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId,
        CommandBufferId,
    };
    use khora_core::renderer::api::device::GraphicsAdapterInfo;
    use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
    use khora_core::renderer::api::pipeline::{
        ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
        RenderPipelineDescriptor, RenderPipelineId,
    };
    use khora_core::renderer::api::resource::{
        BufferDescriptor, BufferId, BufferUsage, SamplerDescriptor, SamplerId, TextureDescriptor,
        TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
    };
    use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
    use khora_core::renderer::error::ResourceError;
    use khora_core::renderer::traits::CommandEncoder;
    use khora_core::renderer::GraphicsDevice;
    use khora_core::residency::Residency;

    use crate::assets::procedural_mesh::create_plane;
    use crate::ecs::{MeshRef, Tick, World};
    use crate::residency::MeshOnGpu;
    use crate::{AssetStore, ProjectionRegistry};

    /// A device that refuses every index buffer while `refuse_indices` is
    /// set, and records the buffers it made and the ones given back.
    #[derive(Debug, Default)]
    struct IndexRefusing {
        refuse_indices: AtomicBool,
        next_id: AtomicUsize,
        made: Mutex<Vec<BufferId>>,
        destroyed: Mutex<Vec<BufferId>>,
    }

    impl IndexRefusing {
        fn refusing() -> Self {
            let device = Self::default();
            device.refuse_indices.store(true, Ordering::Relaxed);
            device
        }
        fn buffer(&self, descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            if descriptor.usage.contains(BufferUsage::INDEX)
                && self.refuse_indices.load(Ordering::Relaxed)
            {
                return Err(ResourceError::BackendError(
                    "no room for an index buffer".into(),
                ));
            }
            let id = BufferId(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
            self.made.lock().expect("the made log").push(id);
            Ok(id)
        }
        fn made(&self) -> Vec<BufferId> {
            self.made.lock().expect("the made log").clone()
        }
        fn destroyed(&self) -> Vec<BufferId> {
            self.destroyed.lock().expect("the destroyed log").clone()
        }
    }

    impl GraphicsDevice for IndexRefusing {
        fn create_shader_module(
            &self,
            _d: &ShaderModuleDescriptor,
        ) -> Result<ShaderModuleId, ResourceError> {
            unreachable!("a mesh upload compiles no shader")
        }
        fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload compiles no shader")
        }
        fn create_render_pipeline(
            &self,
            _d: &RenderPipelineDescriptor,
        ) -> Result<RenderPipelineId, ResourceError> {
            unreachable!("a mesh upload builds no pipeline")
        }
        fn create_pipeline_layout(
            &self,
            _d: &PipelineLayoutDescriptor,
        ) -> Result<PipelineLayoutId, ResourceError> {
            unreachable!("a mesh upload builds no pipeline")
        }
        fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload builds no pipeline")
        }
        fn create_compute_pipeline(
            &self,
            _d: &ComputePipelineDescriptor,
        ) -> Result<ComputePipelineId, ResourceError> {
            unreachable!("a mesh upload builds no pipeline")
        }
        fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload builds no pipeline")
        }
        fn create_bind_group_layout(
            &self,
            _d: &BindGroupLayoutDescriptor,
        ) -> Result<BindGroupLayoutId, ResourceError> {
            unreachable!("a mesh upload binds nothing")
        }
        fn create_bind_group(
            &self,
            _d: &BindGroupDescriptor,
        ) -> Result<BindGroupId, ResourceError> {
            unreachable!("a mesh upload binds nothing")
        }
        fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload binds nothing")
        }
        fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload binds nothing")
        }
        fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            self.buffer(d)
        }
        fn create_buffer_with_data(
            &self,
            d: &BufferDescriptor,
            _data: &[u8],
        ) -> Result<BufferId, ResourceError> {
            self.buffer(d)
        }
        fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
            self.destroyed.lock().expect("the destroyed log").push(id);
            Ok(())
        }
        fn write_buffer(
            &self,
            _id: BufferId,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_buffer_async<'a>(
            &'a self,
            _id: BufferId,
            _offset: u64,
            _data: &'a [u8],
        ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static>
        {
            Box::new(std::future::ready(Ok(())))
        }
        fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
            unreachable!("a mesh upload makes no texture")
        }
        fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload makes no texture")
        }
        fn write_texture(
            &self,
            _id: TextureId,
            _data: &[u8],
            _bytes_per_row: Option<u32>,
            _offset: Origin3D,
            _size: Extent3D,
        ) -> Result<(), ResourceError> {
            unreachable!("a mesh upload makes no texture")
        }
        fn create_texture_view(
            &self,
            _texture: TextureId,
            _d: &TextureViewDescriptor,
        ) -> Result<TextureViewId, ResourceError> {
            unreachable!("a mesh upload makes no texture")
        }
        fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload makes no texture")
        }
        fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
            unreachable!("a mesh upload makes no sampler")
        }
        fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
            unreachable!("a mesh upload makes no sampler")
        }
        fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
            unreachable!("a mesh upload records no command")
        }
        fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
            unreachable!("a mesh upload records no command")
        }
        fn get_surface_format(&self) -> Option<TextureFormat> {
            None
        }
        fn get_surface_size(&self) -> (u32, u32) {
            (0, 0)
        }
        fn get_adapter_info(&self) -> GraphicsAdapterInfo {
            unreachable!("a mesh upload reads no adapter")
        }
        fn supports_feature(&self, _feature_name: &str) -> bool {
            false
        }
    }

    /// **A mesh the device refuses leaves nothing behind.**
    ///
    /// When the index buffer is refused, the make fails and names it, and the
    /// vertex buffer already made for that mesh goes back to the device.
    #[test]
    fn a_refused_index_buffer_gives_the_vertex_buffer_back() {
        let device = IndexRefusing::refusing();
        let mut target: &dyn GraphicsDevice = &device;
        let Err(refusal) = MeshOnGpu::make(&create_plane(1.0, 0.0), &mut target) else {
            panic!("a refused index buffer makes no mesh");
        };
        assert!(
            refusal.reason.contains("index buffer"),
            "the refusal names the index buffer: {refusal}"
        );
        assert_eq!(device.made().len(), 1, "only the vertex buffer was made");
        assert_eq!(
            device.destroyed(),
            device.made(),
            "the vertex buffer goes back to the device"
        );
    }

    /// **A refused mesh is tried again.**
    ///
    /// The store holds a mesh an entity names; `sync_meshes` leaves it
    /// without a GPU copy while the device refuses it, and makes it under its
    /// uuid on the next sync once the device accepts.
    #[test]
    fn a_refused_mesh_is_uploaded_once_the_device_accepts() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = IndexRefusing::refusing();
        let uuid = AssetUUID::new_v5("meshes/plane");
        store
            .store::<Mesh>()
            .write()
            .expect("the mesh store")
            .insert(uuid, AssetHandle::new(create_plane(1.0, 0.0)));
        let mut world = World::new();
        world.spawn(MeshRef::Asset(uuid));
        let resident = |store: &AssetStore| {
            store
                .store::<GpuMesh>()
                .read()
                .expect("the gpu mesh store")
                .contains(&uuid)
        };

        projection.sync_meshes(&world, Tick(0), &device, None);
        assert!(!resident(&store), "a refused mesh has no GPU copy");

        device.refuse_indices.store(false, Ordering::Relaxed);
        projection.sync_meshes(&world, Tick(0), &device, None);
        assert!(
            resident(&store),
            "the mesh is uploaded once the device accepts it"
        );
    }
}

mod by_arrival {
    //! The mesh and material residencies, driven by the copies that arrive in
    //! the asset store, and the references that keep their GPU copies.

    use std::sync::{Arc, Mutex};

    use khora_core::asset::{AssetHandle, AssetUUID, Material, StandardMaterial};
    use khora_core::lane::OutputDeck;
    use khora_core::math::Extent3D;
    use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
    use khora_core::renderer::api::material::GpuMaterial;
    use khora_core::renderer::api::resource::{
        CpuTexture, SampleCount, TextureDimension, TextureFormat, TextureUsage,
    };
    use khora_core::renderer::traits::PipelineSystem;
    use khora_core::renderer::GraphicsDevice;
    use khora_core::Runtime;

    use crate::assets::procedural_mesh::create_plane;
    use crate::ecs::{
        DataSystemRegistration, HandleComponent, MaterialRef, MeshRef, Observer, SystemContext,
        World,
    };
    use crate::gpu::tests::device::{LayoutOnlyPipelines, RecordingDevice};
    use crate::{AssetEviction, AssetStore, ProjectionRegistry};

    /// The registered data system called `name`.
    fn system(name: &'static str) -> &'static DataSystemRegistration {
        inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == name)
            .unwrap_or_else(|| panic!("`{name}` is registered"))
    }

    /// A decoded 1×1 RGBA texture.
    fn one_pixel_texture() -> AssetHandle<CpuTexture> {
        AssetHandle::new(CpuTexture {
            pixels: vec![255; 4],
            size: Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    /// A runtime holding what the residencies and the eviction need: the asset
    /// store, the projection over it, a recording device, the material layouts
    /// and the eviction service.
    struct Gpu {
        store: AssetStore,
        device: Arc<RecordingDevice>,
        runtime: Runtime,
    }

    impl Gpu {
        fn new() -> Self {
            let store = AssetStore::new();
            let device = Arc::new(RecordingDevice::default());
            let mut runtime = Runtime::default();
            runtime.resources.insert(store.clone());
            runtime
                .resources
                .insert(ProjectionRegistry::new(store.clone()));
            runtime
                .resources
                .insert(Arc::new(LayoutOnlyPipelines) as Arc<dyn PipelineSystem>);
            runtime
                .resources
                .insert(Arc::new(Mutex::new(AssetEviction::new())));
            runtime
                .backends
                .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
            Self {
                store,
                device,
                runtime,
            }
        }

        /// Runs the data system `name` once, as the scheduler's runner does:
        /// handed the tick of its last run, its writes stamped with this one.
        fn run(&self, world: &mut World, name: &'static str) {
            let observer = Observer::DataSystem(name);
            let ctx = SystemContext {
                since: world.observer_last_seen(observer),
                resumed: world.observer_resumes(observer),
                scope: None,
            };
            let tick = world.begin_run();
            let mut deck = OutputDeck::new();
            (system(name).run)(world, &self.runtime, &mut deck, ctx);
            world.note_observer_run(observer, tick);
            world.end_run();
        }

        /// One frame of the residencies: meshes, then materials.
        fn frame(&self, world: &mut World) {
            self.run(world, "gpu_mesh_sync");
            self.run(world, "gpu_material_sync");
        }

        /// The end-of-frame eviction.
        fn evict(&self, world: &mut World) {
            self.run(world, "asset_eviction");
        }

        /// Puts `mesh` in the store under `uuid`, as the resolver or the reload does.
        fn land_mesh(&self, uuid: AssetUUID, mesh: Mesh) {
            self.store
                .store::<Mesh>()
                .write()
                .expect("the mesh store")
                .insert(uuid, AssetHandle::new(mesh));
        }

        fn land_material(&self, uuid: AssetUUID, material: StandardMaterial) {
            self.store
                .store::<Box<dyn Material>>()
                .write()
                .expect("the material store")
                .insert(
                    uuid,
                    AssetHandle::new(Box::new(material) as Box<dyn Material>),
                );
        }

        fn land_texture(&self, uuid: AssetUUID) {
            self.store
                .store::<CpuTexture>()
                .write()
                .expect("the texture store")
                .insert(uuid, one_pixel_texture());
        }

        /// Takes the copy held under `uuid` out of the mesh store.
        fn remove_mesh(&self, uuid: AssetUUID) {
            self.store
                .store::<Mesh>()
                .write()
                .expect("the mesh store")
                .remove(&uuid);
        }

        /// The GPU copy of the mesh `uuid`, if resident.
        fn gpu_mesh(&self, uuid: AssetUUID) -> Option<AssetHandle<GpuMesh>> {
            self.store
                .store::<GpuMesh>()
                .read()
                .expect("the gpu mesh store")
                .get(&uuid)
                .cloned()
        }

        /// The GPU copy of the material `uuid`, if resident.
        fn gpu_material(&self, uuid: AssetUUID) -> Option<AssetHandle<GpuMaterial>> {
            self.store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .get(&uuid)
                .cloned()
        }
    }

    /// Stores a copy of `make` under `uuid` in `Assets<A>` more times than its
    /// journal keeps, so the residency reading it next reads a resync.
    fn overflow_arrivals<A: khora_core::asset::Asset>(
        gpu: &Gpu,
        uuid: AssetUUID,
        make: impl Fn() -> AssetHandle<A>,
    ) {
        let store = gpu.store.store::<A>();
        let mut store = store.write().expect("the store");
        for _ in 0..=crate::assets::ARRIVALS_KEPT {
            store.insert(uuid, make());
        }
    }

    /// A material whose copies alive are counted by the marker each holds.
    #[derive(Clone)]
    struct CountedMaterial {
        #[expect(
            dead_code,
            reason = "held, never read: the test reads its strong count to count the copies alive"
        )]
        alive: Arc<()>,
    }

    impl khora_core::asset::Asset for CountedMaterial {}
    impl Material for CountedMaterial {}

    /// **A mesh that lands is made resident once, however many entities name it.**
    ///
    /// Two entities name one uuid by their `MeshRef` alone. Once its copy
    /// lands in the store, the GPU copy under that uuid holds its vertices,
    /// made once, and not again on a later frame.
    #[test]
    fn a_landed_mesh_is_made_resident_once_for_every_entity_naming_it() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        world.spawn(MeshRef::Asset(uuid));
        world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);

        let plane = create_plane(1.0, 0.0);
        let vertices = plane.create_vertex_buffer();
        gpu.land_mesh(uuid, plane);
        gpu.frame(&mut world);
        gpu.frame(&mut world);

        let Some(resident) = gpu.gpu_mesh(uuid) else {
            panic!("the landed mesh is resident under its uuid");
        };
        assert_eq!(
            gpu.device.data_of(resident.vertex_buffer),
            Some(vertices.clone()),
            "the GPU copy holds the landed mesh's vertices"
        );
        assert_eq!(
            gpu.device.uploads_of(&vertices),
            1,
            "one GPU copy for both entities, not made again on a later frame"
        );
    }

    /// **A replaced mesh is made again on the GPU, under the same uuid.**
    #[test]
    fn a_replaced_mesh_is_remade_on_the_gpu() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);
        gpu.land_mesh(uuid, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        let Some(first) = gpu.gpu_mesh(uuid) else {
            panic!("the landed mesh is resident");
        };

        let larger = create_plane(2.0, 0.0);
        let vertices = larger.create_vertex_buffer();
        gpu.land_mesh(uuid, larger);
        gpu.frame(&mut world);

        let Some(second) = gpu.gpu_mesh(uuid) else {
            panic!("the replaced mesh is resident under the same uuid");
        };
        assert_ne!(
            second.vertex_buffer, first.vertex_buffer,
            "a new GPU copy, not the old one"
        );
        assert_eq!(
            gpu.device.data_of(second.vertex_buffer),
            Some(vertices),
            "the new GPU copy holds the replacing mesh's vertices"
        );
    }

    /// **A remade mesh gives its old buffers back to the device.**
    ///
    /// The entity still names the uuid, so only the old copy is freed, by the
    /// end of the frame's eviction; the new copy stays.
    #[test]
    fn a_replaced_mesh_gives_its_old_buffers_back() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);
        gpu.land_mesh(uuid, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        let Some(first) = gpu.gpu_mesh(uuid) else {
            panic!("the landed mesh is resident");
        };

        gpu.land_mesh(uuid, create_plane(2.0, 0.0));
        gpu.frame(&mut world);
        gpu.evict(&mut world);

        let destroyed = gpu.device.destroyed_buffers();
        for buffer in [first.vertex_buffer, first.index_buffer] {
            assert!(
                destroyed.contains(&buffer),
                "the old copy's {buffer:?} is given back: {destroyed:?}"
            );
        }
        let Some(second) = gpu.gpu_mesh(uuid) else {
            panic!("the new copy stays resident");
        };
        for buffer in [second.vertex_buffer, second.index_buffer] {
            assert!(
                !destroyed.contains(&buffer),
                "the new copy's {buffer:?} is kept: {destroyed:?}"
            );
        }
    }

    /// **A mesh gone from the store leaves the GPU.**
    ///
    /// Its GPU copy is taken out the frame the copy goes, though an entity
    /// still names it, and its buffers are given back by the eviction.
    #[test]
    fn a_mesh_gone_from_the_store_leaves_the_gpu() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);
        gpu.land_mesh(uuid, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        let Some(resident) = gpu.gpu_mesh(uuid) else {
            panic!("the landed mesh is resident");
        };

        gpu.remove_mesh(uuid);
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_mesh(uuid).is_none(),
            "a mesh gone from the store has no GPU copy"
        );

        gpu.evict(&mut world);
        let destroyed = gpu.device.destroyed_buffers();
        for buffer in [resident.vertex_buffer, resident.index_buffer] {
            assert!(
                destroyed.contains(&buffer),
                "the gone copy's {buffer:?} is given back: {destroyed:?}"
            );
        }
    }

    /// **A material waits for its textures, then becomes resident.**
    ///
    /// Its copy lands before the texture it names: it is not made resident,
    /// however many frames pass. The frame the texture lands, it is, though
    /// the material itself did not arrive again.
    #[test]
    fn a_material_waits_for_its_textures_then_becomes_resident() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let mesh = AssetUUID::new_v5("meshes/plane.obj");
        let material = AssetUUID::new_v5("materials/painted.kmat");
        let texture = AssetUUID::new_v5("textures/paint.png");
        world.spawn((MeshRef::Asset(mesh), MaterialRef::Asset(material)));
        gpu.frame(&mut world);

        gpu.land_mesh(mesh, create_plane(1.0, 0.0));
        gpu.land_material(
            material,
            StandardMaterial {
                base_color_texture: Some(texture),
                ..Default::default()
            },
        );
        gpu.frame(&mut world);
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_material(material).is_none(),
            "a material whose texture is missing is not resident"
        );
        assert!(
            gpu.device.textures().is_empty(),
            "nothing of the material is uploaded while it waits"
        );

        gpu.land_texture(texture);
        gpu.frame(&mut world);

        let Some(resident) = gpu.gpu_material(material) else {
            panic!("the material is resident the frame its texture lands");
        };
        let Some(view) = resident.base_color_view else {
            panic!("the resident material binds its base colour texture");
        };
        assert_eq!(
            gpu.device.texture_of_view(view),
            resident.base_color_texture,
            "the view is of the texture the material owns"
        );
    }

    /// **A residency that resyncs makes resident only the stored copies a reference names.**
    ///
    /// The store holds two meshes and a material before either residency
    /// first reads it; entities name both meshes' uuids and the material's,
    /// and a third stored mesh nothing names. One frame makes the named copies
    /// resident under their uuids, and not the unnamed one.
    #[test]
    fn a_residency_that_resyncs_makes_the_named_stored_copies_resident() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let plane = AssetUUID::new_v5("meshes/plane.obj");
        let floor = AssetUUID::new_v5("meshes/floor.obj");
        let unnamed = AssetUUID::new_v5("meshes/unnamed.obj");
        let paint = AssetUUID::new_v5("materials/paint.kmat");
        gpu.land_mesh(plane, create_plane(1.0, 0.0));
        gpu.land_mesh(floor, create_plane(8.0, -1.0));
        gpu.land_mesh(unnamed, create_plane(3.0, 2.0));
        gpu.land_material(paint, StandardMaterial::default());
        world.spawn((MeshRef::Asset(plane), MaterialRef::Asset(paint)));
        world.spawn(MeshRef::Asset(floor));

        gpu.frame(&mut world);

        for uuid in [plane, floor] {
            assert!(
                gpu.gpu_mesh(uuid).is_some(),
                "the stored mesh {uuid:?} a reference names is resident"
            );
        }
        assert!(
            gpu.gpu_material(paint).is_some(),
            "the stored material a reference names is resident"
        );
        assert!(
            gpu.gpu_mesh(unnamed).is_none(),
            "a stored mesh nothing names is not made resident"
        );
    }

    /// **A mesh named again after its GPU copy was evicted is made resident again.**
    ///
    /// Its last entity goes and the eviction frees the GPU copy; the copy in
    /// the store stays and does not arrive again. An entity naming it later
    /// (an undone delete, a respawn) is drawn all the same.
    #[test]
    fn a_mesh_named_again_after_its_gpu_copy_was_evicted_is_made_resident_again() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        let first = world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);
        gpu.land_mesh(uuid, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        assert!(gpu.gpu_mesh(uuid).is_some(), "the landed mesh is resident");

        world.despawn(first);
        gpu.evict(&mut world);
        assert!(
            gpu.gpu_mesh(uuid).is_none(),
            "no reference names it: its GPU copy is evicted"
        );

        world.spawn(MeshRef::Asset(uuid));
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_mesh(uuid).is_some(),
            "named again, the stored mesh is made resident again"
        );
    }

    /// **No entity carries a GPU tag.**
    ///
    /// The GPU copies live in the store under their uuids only: the tag types
    /// are not registered, and a frame of both residencies, on an entity
    /// holding only its references, attaches none.
    #[test]
    fn no_entity_carries_a_gpu_tag() {
        let gpu = Gpu::new();
        let mut world = World::new();
        for name in ["HandleComponent<GpuMesh>", "HandleComponent<GpuMaterial>"] {
            assert_eq!(
                world.components().key_named(name),
                None,
                "`{name}` is not a registered component"
            );
        }

        let mesh_uuid = AssetUUID::new_v5("meshes/plane.obj");
        let material_uuid = AssetUUID::new_v5("materials/plain.kmat");
        let mesh = AssetHandle::new(create_plane(1.0, 0.0));
        let material: AssetHandle<Box<dyn Material>> =
            AssetHandle::new(Box::new(StandardMaterial::default()));
        gpu.frame(&mut world);
        let entity = world.spawn((MeshRef::Asset(mesh_uuid), MaterialRef::Asset(material_uuid)));
        gpu.store
            .store::<Mesh>()
            .write()
            .expect("the mesh store")
            .insert(mesh_uuid, mesh);
        gpu.store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .insert(material_uuid, material);
        gpu.frame(&mut world);

        assert!(
            gpu.gpu_mesh(mesh_uuid).is_some() && gpu.gpu_material(material_uuid).is_some(),
            "the mesh and the material are resident"
        );
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_none(),
            "the entity carries no GPU mesh tag"
        );
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the entity carries no GPU material tag"
        );
    }

    /// **A resync makes no GPU copy the eviction freed.**
    ///
    /// A mesh no reference names is evicted. More meshes then arrive in one
    /// frame than the journal keeps: the residency reads the whole store, and
    /// the mesh that did not arrive stays off the GPU, as it would have had
    /// the residency read every arrival.
    #[test]
    fn a_resync_makes_no_gpu_copy_the_eviction_freed() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let unnamed = AssetUUID::new_v5("meshes/unnamed.obj");
        let busy = AssetUUID::new_v5("meshes/busy.obj");
        gpu.land_mesh(unnamed, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        gpu.evict(&mut world);
        assert!(
            gpu.gpu_mesh(unnamed).is_none(),
            "the premise: no reference names it, its GPU copy is evicted"
        );

        overflow_arrivals(&gpu, busy, || AssetHandle::new(create_plane(2.0, 0.0)));
        gpu.frame(&mut world);

        assert!(
            gpu.gpu_mesh(unnamed).is_none(),
            "a mesh that did not arrive and no reference names is not made again"
        );
    }

    /// **A resync forgets a material gone from the store while off the GPU.**
    ///
    /// Its last reference went and its GPU copy was evicted, then its copy left
    /// the store in a frame of more arrivals than the journal keeps. Once the
    /// residency has resynced, nothing it remembers holds the material any more.
    #[test]
    fn a_resync_forgets_a_material_gone_from_the_store_while_off_the_gpu() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let counted = AssetUUID::new_v5("materials/counted.kmat");
        let busy = AssetUUID::new_v5("materials/busy.kmat");
        let alive = Arc::new(());
        let naming = world.spawn(MaterialRef::Asset(counted));
        gpu.store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .insert(
                counted,
                AssetHandle::new(Box::new(CountedMaterial {
                    alive: Arc::clone(&alive),
                }) as Box<dyn Material>),
            );
        gpu.frame(&mut world);
        assert!(gpu.gpu_material(counted).is_some(), "the premise: resident");
        world.despawn(naming);
        gpu.evict(&mut world);
        assert!(
            gpu.gpu_material(counted).is_none(),
            "the premise: no reference names it, its GPU copy is evicted"
        );

        gpu.store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .remove(&counted);
        overflow_arrivals(&gpu, busy, || {
            AssetHandle::new(Box::new(StandardMaterial::default()) as Box<dyn Material>)
        });
        gpu.frame(&mut world);

        assert_eq!(
            Arc::strong_count(&alive),
            1,
            "a material gone from both stores is held by nothing the residency remembers"
        );
    }

    /// **A resync before a named copy lands makes it resident when it lands.**
    ///
    /// The mesh an entity names is still being decoded through a frame of
    /// more arrivals than the journal keeps: nothing is made then, and the
    /// frame its copy lands, it is.
    #[test]
    fn a_resync_before_a_named_copy_lands_makes_it_resident_when_it_lands() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let decoding = AssetUUID::new_v5("meshes/decoding.obj");
        let busy = AssetUUID::new_v5("meshes/busy.obj");
        world.spawn(MeshRef::Asset(decoding));
        gpu.frame(&mut world);

        overflow_arrivals(&gpu, busy, || AssetHandle::new(create_plane(2.0, 0.0)));
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_mesh(decoding).is_none(),
            "a named mesh the store does not hold yet has no GPU copy"
        );

        gpu.land_mesh(decoding, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_mesh(decoding).is_some(),
            "the named mesh is resident the frame its copy lands"
        );
    }

    /// **A texture stored again as the same copy remakes no material; another copy does.**
    ///
    /// The material is built from the texture's copy: the same handle stored
    /// again changes nothing, while a new handle of equal content is a new
    /// copy, which the material is made again from.
    #[test]
    fn a_texture_stored_again_remakes_its_material_only_as_another_copy() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let material = AssetUUID::new_v5("materials/painted.kmat");
        let paint = AssetUUID::new_v5("textures/paint.png");
        world.spawn(MaterialRef::Asset(material));
        gpu.land_texture(paint);
        gpu.land_material(
            material,
            StandardMaterial {
                base_color_texture: Some(paint),
                ..Default::default()
            },
        );
        gpu.frame(&mut world);
        let Some(first) = gpu.gpu_material(material) else {
            panic!("the premise: the material is resident");
        };

        let same = gpu
            .store
            .store::<CpuTexture>()
            .read()
            .expect("the texture store")
            .get(&paint)
            .cloned()
            .expect("the texture is stored");
        gpu.store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .insert(paint, same);
        gpu.frame(&mut world);
        let Some(kept) = gpu.gpu_material(material) else {
            panic!("the material stays resident");
        };
        assert_eq!(
            kept.bind_group, first.bind_group,
            "the same texture copy stored again remakes nothing"
        );

        gpu.land_texture(paint);
        gpu.frame(&mut world);
        let Some(remade) = gpu.gpu_material(material) else {
            panic!("the material stays resident");
        };
        assert_ne!(
            remade.bind_group, first.bind_group,
            "a new copy of equal content makes the material again"
        );
    }

    /// **A texture resync makes no material again whose textures are unchanged.**
    ///
    /// A resident material names a texture. More copies of another texture
    /// arrive in one frame than the journal keeps; the material's texture is
    /// the same copy, so its GPU copy is kept, not made a second time.
    #[test]
    fn a_texture_resync_remakes_no_material_whose_textures_are_unchanged() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let material = AssetUUID::new_v5("materials/painted.kmat");
        let paint = AssetUUID::new_v5("textures/paint.png");
        let busy = AssetUUID::new_v5("textures/busy.png");
        world.spawn(MaterialRef::Asset(material));
        gpu.land_texture(paint);
        gpu.land_material(
            material,
            StandardMaterial {
                base_color_texture: Some(paint),
                ..Default::default()
            },
        );
        gpu.frame(&mut world);
        let Some(before) = gpu.gpu_material(material) else {
            panic!("the premise: the material is resident");
        };

        overflow_arrivals(&gpu, busy, one_pixel_texture);
        gpu.frame(&mut world);

        let Some(after) = gpu.gpu_material(material) else {
            panic!("the material stays resident");
        };
        assert_eq!(
            after.bind_group, before.bind_group,
            "the GPU copy made from the same material and texture is kept"
        );
        assert_eq!(
            gpu.device.bind_groups().len(),
            1,
            "the material is made once"
        );
    }

    /// **A texture gone from the store takes its materials off the GPU until it lands again.**
    ///
    /// The material itself does not change: it waits for its texture, with
    /// no fallback, and is made again the frame the texture lands again.
    #[test]
    fn a_texture_gone_takes_its_materials_off_the_gpu_until_it_lands_again() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let material = AssetUUID::new_v5("materials/painted.kmat");
        let paint = AssetUUID::new_v5("textures/paint.png");
        world.spawn(MaterialRef::Asset(material));
        gpu.land_texture(paint);
        gpu.land_material(
            material,
            StandardMaterial {
                base_color_texture: Some(paint),
                ..Default::default()
            },
        );
        gpu.frame(&mut world);
        let Some(first) = gpu.gpu_material(material) else {
            panic!("the premise: the material is resident");
        };

        gpu.store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .remove(&paint);
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_material(material).is_none(),
            "a material whose texture is gone is not resident"
        );
        gpu.evict(&mut world);
        let Some(first_texture) = first.base_color_texture else {
            panic!("the premise: the material owned its texture");
        };
        assert!(
            gpu.device.destroyed_textures().contains(&first_texture),
            "the GPU copy taken off gives its texture back"
        );

        gpu.land_texture(paint);
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_material(material).is_some(),
            "the material is resident again the frame its texture lands again"
        );
    }

    /// **A material named again after its GPU copy was evicted is made resident again.**
    ///
    /// The copy in the store stays and does not arrive again: the changed
    /// `MaterialRef` alone brings it back.
    #[test]
    fn a_material_named_again_after_its_gpu_copy_was_evicted_is_made_resident_again() {
        let gpu = Gpu::new();
        let mut world = World::new();
        let material = AssetUUID::new_v5("materials/plain.kmat");
        let first = world.spawn(MaterialRef::Asset(material));
        gpu.land_material(material, StandardMaterial::default());
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_material(material).is_some(),
            "the premise: resident"
        );

        world.despawn(first);
        gpu.evict(&mut world);
        assert!(
            gpu.gpu_material(material).is_none(),
            "the premise: no reference names it, its GPU copy is evicted"
        );

        world.spawn(MaterialRef::Asset(material));
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_material(material).is_some(),
            "named again, the stored material is made resident again"
        );
    }

    /// **A restored snapshot naming an evicted mesh makes it resident again.**
    ///
    /// What the editor's Stop does: the entities saved before play come back
    /// by a load, not a spawn, and the mesh they name, evicted meanwhile, is
    /// drawn again.
    #[test]
    fn a_restored_snapshot_naming_an_evicted_mesh_makes_it_resident_again() {
        use crate::scene::snapshot::{prepare_snapshot, write_snapshot};
        use crate::scene::Identity;

        let gpu = Gpu::new();
        let mut world = World::new();
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        let entity = world.spawn(MeshRef::Asset(uuid));
        gpu.land_mesh(uuid, create_plane(1.0, 0.0));
        gpu.frame(&mut world);
        let snapshot = write_snapshot(&world).expect("the world is snapshotted");

        world.despawn(entity);
        gpu.frame(&mut world);
        gpu.evict(&mut world);
        assert!(
            gpu.gpu_mesh(uuid).is_none(),
            "the premise: no reference names it, its GPU copy is evicted"
        );

        prepare_snapshot(&mut world, snapshot)
            .expect("the snapshot is read")
            .commit(&mut world, Identity::Keep);
        gpu.frame(&mut world);
        assert!(
            gpu.gpu_mesh(uuid).is_some(),
            "the restored entity's mesh is made resident again"
        );
    }
}
