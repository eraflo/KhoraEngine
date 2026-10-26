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

/// **Residencies run before the agents, after the reload.** The UI images,
/// the meshes and the materials are made resident in `PreExtract`, before
/// the Flows project and the agents draw, and after `asset_reload` replaced
/// the copies of whatever changed — so a changed asset reaches the device the
/// frame it changed. The mesh and material residencies keep the names they
/// were registered under before, which saves and telemetry know them by.
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
    use khora_core::renderer::api::gpu_scene::GpuMesh;
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
    use crate::ecs::{HandleComponent, World};
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

    /// **A mesh the device refuses leaves nothing behind.** When the index
    /// buffer is refused, the make fails and names it, and the vertex buffer
    /// already made for that mesh goes back to the device.
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

    /// **A refused mesh is tried again.** `gpu_mesh_sync` leaves an entity
    /// whose mesh the device refused without a GPU mesh, and caches nothing
    /// for it; once the device accepts, the next sync uploads and tags it.
    #[test]
    fn a_refused_mesh_is_uploaded_once_the_device_accepts() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = IndexRefusing::refusing();
        let uuid = AssetUUID::new_v5("meshes/plane");
        let mut world = World::new();
        let entity = world.spawn(HandleComponent {
            handle: AssetHandle::new(create_plane(1.0, 0.0)),
            uuid,
        });

        projection.sync_all(&mut world, &device);
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_none(),
            "a refused mesh is not tagged"
        );
        assert!(
            !store
                .store::<GpuMesh>()
                .read()
                .expect("the gpu mesh store")
                .contains(&uuid),
            "a refused mesh is not cached"
        );

        device.refuse_indices.store(false, Ordering::Relaxed);
        projection.sync_all(&mut world, &device);
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_some(),
            "the mesh is uploaded once the device accepts it"
        );
    }
}
