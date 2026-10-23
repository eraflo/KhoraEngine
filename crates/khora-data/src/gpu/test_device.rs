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

//! A graphics device that records what the projection and the eviction ask
//! of it, so their effect on GPU resources is readable without a GPU.

use std::sync::Mutex;

use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, BindingResource, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{CommandEncoder, GraphicsDevice, PipelineSystem};

/// One texture the device created.
#[derive(Debug, Clone)]
pub(crate) struct CreatedTexture {
    pub format: TextureFormat,
    pub size: Extent3D,
}

/// One bind group entry the device was handed: its binding and, for a texture
/// view, which view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundEntry {
    pub binding: u32,
    pub view: Option<TextureViewId>,
}

#[derive(Debug, Default)]
struct Log {
    next_id: usize,
    buffers_with_data: Vec<(BufferId, Vec<u8>)>,
    textures: Vec<CreatedTexture>,
    views: Vec<(TextureViewId, TextureId)>,
    bind_groups: Vec<Vec<BoundEntry>>,
    destroyed_textures: Vec<TextureId>,
    destroyed_views: Vec<TextureViewId>,
}

impl Log {
    fn next(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }
}

/// Records every resource created and destroyed; creation always succeeds.
#[derive(Debug, Default)]
pub(crate) struct RecordingDevice {
    log: Mutex<Log>,
}

impl RecordingDevice {
    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The bytes of every buffer created with data, in creation order.
    pub fn buffers_with_data(&self) -> Vec<Vec<u8>> {
        self.log()
            .buffers_with_data
            .iter()
            .map(|(_, bytes)| bytes.clone())
            .collect()
    }

    /// Every texture created, in creation order.
    pub fn textures(&self) -> Vec<CreatedTexture> {
        self.log().textures.clone()
    }

    /// The texture a view was created on.
    pub fn texture_of_view(&self, view: TextureViewId) -> Option<TextureId> {
        self.log()
            .views
            .iter()
            .find(|(v, _)| *v == view)
            .map(|(_, texture)| *texture)
    }

    /// The entries of every bind group created, in creation order.
    pub fn bind_groups(&self) -> Vec<Vec<BoundEntry>> {
        self.log().bind_groups.clone()
    }

    pub fn destroyed_textures(&self) -> Vec<TextureId> {
        self.log().destroyed_textures.clone()
    }

    pub fn destroyed_views(&self) -> Vec<TextureViewId> {
        self.log().destroyed_views.clone()
    }
}

impl GraphicsDevice for RecordingDevice {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!("the material projection compiles no shader")
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!("the material projection compiles no shader")
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        let id = self.log().next();
        Ok(BindGroupLayoutId(id))
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        let entries = d
            .entries
            .iter()
            .map(|entry| BoundEntry {
                binding: entry.binding,
                view: match entry.resource {
                    BindingResource::TextureView(view) => Some(view),
                    _ => None,
                },
            })
            .collect();
        let mut log = self.log();
        log.bind_groups.push(entries);
        let id = log.next();
        Ok(BindGroupId(id))
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        let id = self.log().next();
        Ok(BufferId(id))
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        let mut log = self.log();
        let id = BufferId(log.next());
        log.buffers_with_data.push((id, data.to_vec()));
        Ok(id)
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        Box::new(std::future::ready(Ok(())))
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        let mut log = self.log();
        let id = TextureId(log.next());
        log.textures.push(CreatedTexture {
            format: d.format,
            size: d.size,
        });
        Ok(id)
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.log().destroyed_textures.push(id);
        Ok(())
    }
    fn write_texture(
        &self,
        _id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        _size: Extent3D,
    ) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_texture_view(
        &self,
        texture: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        let mut log = self.log();
        let id = TextureViewId(log.next());
        log.views.push((id, texture));
        Ok(id)
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.log().destroyed_views.push(id);
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        let id = self.log().next();
        Ok(SamplerId(id))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unreachable!("the material projection records no command")
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
        unreachable!("the material projection records no command")
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        None
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (0, 0)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        unreachable!("the material projection reads no adapter")
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}

/// A pipeline system whose every material layout is one fixed id.
#[derive(Debug, Default)]
pub(crate) struct LayoutOnlyPipelines;

impl PipelineSystem for LayoutOnlyPipelines {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        Ok(BindGroupLayoutId(0))
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("the material projection builds no inline layout")
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        unreachable!("the material projection builds no pipeline")
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        unreachable!("the material projection builds no pipeline")
    }
}
