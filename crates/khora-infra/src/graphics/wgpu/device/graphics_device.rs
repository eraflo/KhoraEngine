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

//! `GraphicsDevice` for the wgpu device. Resource creation, writes and
//! removal delegate to the device's inherent methods, one sibling file per
//! kind of resource (`pipelines`, `buffers`, `textures`, `bind_groups`);
//! the device queries and command submission are answered here.

use khora_core::gpu::DeviceCaps;
use khora_core::math::dimension;
use khora_core::renderer::api::command as api_cmd;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::device::{
    GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
};
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::api::resource::texture::{self as api_tex};
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::{GraphicsDevice, ResourceError};

use crate::graphics::wgpu::command::WgpuCommandEncoder;

use std::future::Future;
use wgpu;

use super::WgpuDevice;

impl GraphicsDevice for WgpuDevice {
    // --- Shader Module Operations ---

    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        self.compile_shader_module(descriptor)
    }

    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.remove_shader_module(id)
    }

    // -- Render Pipeline Operations ---

    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        self.build_render_pipeline(descriptor)
    }

    fn create_pipeline_layout(
        &self,
        descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        self.build_pipeline_layout(descriptor)
    }

    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.remove_render_pipeline(id)
    }

    fn create_compute_pipeline(
        &self,
        descriptor: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        self.build_compute_pipeline(descriptor)
    }

    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.remove_compute_pipeline(id)
    }

    // --- Buffer Operations ---

    fn create_buffer(
        &self,
        descriptor: &api_buf::BufferDescriptor,
    ) -> Result<api_buf::BufferId, ResourceError> {
        self.allocate_buffer(descriptor)
    }

    fn create_buffer_with_data(
        &self,
        descriptor: &api_buf::BufferDescriptor,
        data: &[u8],
    ) -> Result<api_buf::BufferId, ResourceError> {
        self.allocate_buffer_with_data(descriptor, data)
    }

    fn destroy_buffer(&self, id: api_buf::BufferId) -> Result<(), ResourceError> {
        self.release_buffer(id)
    }

    fn write_buffer(
        &self,
        id: api_buf::BufferId,
        offset: u64,
        data: &[u8],
    ) -> Result<(), ResourceError> {
        self.queue_buffer_write(id, offset, data)
    }

    fn write_buffer_async<'a>(
        &'a self,
        id: api_buf::BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        self.map_buffer_write(id, offset, data)
    }

    // --- Texture and Sampler Operations ---

    fn create_texture(
        &self,
        descriptor: &api_tex::TextureDescriptor,
    ) -> Result<api_tex::TextureId, ResourceError> {
        self.allocate_texture(descriptor)
    }

    fn destroy_texture(&self, id: api_tex::TextureId) -> Result<(), ResourceError> {
        self.release_texture(id)
    }

    fn write_texture(
        &self,
        texture_id: api_tex::TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: dimension::Origin3D,
        size: dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        self.queue_texture_write(texture_id, data, bytes_per_row, offset, size)
    }

    fn create_texture_view(
        &self,
        texture_id: api_tex::TextureId,
        descriptor: &api_tex::TextureViewDescriptor,
    ) -> Result<api_tex::TextureViewId, ResourceError> {
        self.build_texture_view(texture_id, descriptor)
    }

    fn destroy_texture_view(&self, id: api_tex::TextureViewId) -> Result<(), ResourceError> {
        self.remove_texture_view(id)
    }

    fn create_sampler(
        &self,
        descriptor: &api_tex::SamplerDescriptor,
    ) -> Result<api_tex::SamplerId, ResourceError> {
        self.build_sampler(descriptor)
    }

    fn destroy_sampler(&self, id: api_tex::SamplerId) -> Result<(), ResourceError> {
        self.remove_sampler(id)
    }

    // --- Device Queries and Submission ---

    fn get_surface_format(&self) -> Option<TextureFormat> {
        self.surface().format
    }

    fn get_surface_size(&self) -> (u32, u32) {
        self.surface().size
    }

    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        self.internal
            .adapter
            .get()
            .cloned()
            .unwrap_or_else(|| GraphicsAdapterInfo {
                name: "<undescribed adapter>".to_owned(),
                backend_type: GraphicsBackendType::Unknown,
                device_type: RendererDeviceType::Unknown,
            })
    }

    /// Who made the adapter the device runs on, which device and driver, as
    /// wgpu reports them; `None` until the render system gave the adapter.
    fn adapter_identity(&self) -> Option<khora_core::renderer::api::device::AdapterIdentity> {
        (self.internal.adapter_handle.get()).map(|adapter| {
            crate::graphics::wgpu::conversions::adapter_identity_of(&adapter.get_info())
        })
    }

    /// Read from the device's limits: storage buffers are there when a
    /// shader stage may bind at least one, compute shaders when a dispatch may
    /// run at least one workgroup of one invocation (a downlevel device, such
    /// as WebGL2, allows neither).
    fn caps(&self) -> DeviceCaps {
        let limits = self.wgpu_device().limits();
        DeviceCaps {
            storage_buffers: limits.max_storage_buffers_per_shader_stage > 0,
            compute: limits.max_compute_workgroups_per_dimension > 0
                && limits.max_compute_invocations_per_workgroup > 0,
        }
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let features = self.wgpu_device().features();
        match feature_name {
            "gpu_timestamps" => features.contains(wgpu::Features::TIMESTAMP_QUERY),
            "texture_compression_bc" => features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            "polygon_mode_line" => features.contains(wgpu::Features::POLYGON_MODE_LINE),
            "polygon_mode_point" => features.contains(wgpu::Features::POLYGON_MODE_POINT),
            _ => {
                log::warn!(
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {feature_name}"
                );
                false
            }
        }
    }

    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        let descriptor = wgpu::CommandEncoderDescriptor { label };
        let encoder = self.wgpu_device().create_command_encoder(&descriptor);

        Box::new(WgpuCommandEncoder {
            encoder: Some(encoder), // Wrap in Option to be `take`n in finish()
            device: self.clone(),   // Clone the Arc handle
        })
    }

    fn submit_command_buffer(&self, command_buffer_id: api_cmd::CommandBufferId) {
        let buffer =
            match self.with_registry(|r| r.pending_command_buffers.remove(&command_buffer_id)) {
                Ok(Some(buffer)) => buffer,
                Ok(None) => {
                    log::error!(
                        "Attempted to submit a CommandBufferId ({:?}) that does not exist.",
                        command_buffer_id
                    );
                    return;
                }
                Err(e) => {
                    log::error!(
                        "WgpuDevice::submit_command_buffer: {e}, dropping {:?}",
                        command_buffer_id
                    );
                    return;
                }
            };
        self.wgpu_queue().submit(std::iter::once(buffer));
    }

    // --- Bind Group Operations ---

    fn create_bind_group_layout(
        &self,
        descriptor: &api_cmd::BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        self.build_bind_group_layout(descriptor)
    }

    fn create_bind_group(
        &self,
        descriptor: &api_cmd::BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError> {
        self.build_bind_group(descriptor)
    }

    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.remove_bind_group_layout(id)
    }

    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.remove_bind_group(id)
    }
}
