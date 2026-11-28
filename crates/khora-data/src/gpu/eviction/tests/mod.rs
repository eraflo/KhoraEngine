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

//! Tests of the asset eviction, by behaviour; the helpers they share live
//! here: GPU copy stubs, the store and reference helpers, and a device
//! recording what it is asked to destroy.

mod liveness;
mod orphans;
mod retirement;

use std::sync::Mutex;

use khora_core::asset::InlineMaterial;
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    PrimitiveTopology, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::CommandEncoder;

use super::*;

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

/// Spawns an entity whose `AssetRef<Box<dyn Material>>` names `uuid`, making `uuid` live.
fn spawn_material_ref(world: &mut World, uuid: AssetUUID) -> khora_core::ecs::entity::EntityId {
    world.spawn(AssetRef::<Box<dyn Material>>::Asset(uuid))
}

/// Spawns an entity whose `AssetRef<Mesh>` names `uuid`, making `uuid` live.
fn spawn_mesh_ref(world: &mut World, uuid: AssetUUID) {
    world.spawn(AssetRef::<Mesh>::Asset(uuid));
}

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
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
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
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!("eviction writes nothing")
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
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
