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

use crate::math::dimension;
use crate::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
};
use crate::renderer::api::device::GraphicsAdapterInfo;
use crate::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use crate::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use crate::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use crate::renderer::error::ResourceError;
use crate::renderer::traits::CommandEncoder;
use std::fmt::Debug;
use std::future::Future;

/// Backend-neutral graphics device that creates, writes and destroys GPU resources.
///
/// Resources are named by opaque ids, never by backend handles. The
/// implementation lives in `khora-infra`.
///
/// # Called from many threads at once
///
/// Lanes prepare and record in concurrent waves, so any method may run on
/// several threads together — creating, writing and destroying at the same
/// moment. An implementation must never wait while holding something another
/// call could wait for: its locks are **leaves**, held only to look an entry
/// up, insert or remove it, and never across a call to the graphics API or to
/// anything else. Code above the device (a pipeline system, a glyph atlas)
/// may hold its own lock while calling the device, because the device never
/// calls back up.
///
/// Any method returning a [`ResourceError`] may also fail when the backend's
/// own state is unusable (the wgpu backend: its registry lock poisoned).
pub trait GraphicsDevice: Send + Sync + Debug + 'static {
    // --- Shader Management ---

    /// Creates a shader module from a descriptor.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the shader source is invalid or fails to compile.
    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError>;

    /// Destroys a shader module, releasing its GPU resources.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live shader module.
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError>;

    // --- Pipeline Management ---

    /// Creates a render pipeline from a descriptor.
    ///
    /// A render pipeline represents the entire configurable state of the GPU for a
    /// draw call, including shaders, vertex layouts, and blend states.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the pipeline configuration is invalid or compilation fails.
    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError>;

    /// Creates a pipeline layout from a descriptor.
    ///
    /// The pipeline layout defines the set of resource bindings (e.g., uniform buffers,
    /// textures) that a pipeline can access.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the layout is invalid.
    fn create_pipeline_layout(
        &self,
        descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError>;

    /// Destroys a render pipeline, releasing its GPU resources.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live render pipeline.
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError>;

    // --- Compute Pipeline Management ---

    /// Creates a compute pipeline from a descriptor.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the pipeline configuration is invalid or compilation fails.
    fn create_compute_pipeline(
        &self,
        descriptor: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError>;

    /// Destroys a compute pipeline, releasing its GPU resources.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live compute pipeline.
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError>;

    // --- Bind Group Management ---

    /// Creates a bind group layout from a descriptor.
    ///
    /// A bind group layout describes the structure and types of resources that can be
    /// bound to shaders at a specific bind group index.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the layout is invalid.
    fn create_bind_group_layout(
        &self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError>;

    /// Creates a bind group from a descriptor.
    ///
    /// A bind group represents the actual bound resources (buffers, textures, etc.)
    /// that conform to a specific bind group layout.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if the bind group configuration is invalid or
    /// doesn't match its layout.
    fn create_bind_group(
        &self,
        descriptor: &BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError>;

    /// Destroys a bind group layout, releasing its resources.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live layout.
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError>;

    /// Destroys a bind group, releasing its resources.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live bind group.
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError>;

    // --- Buffer Management ---

    /// Creates a new GPU buffer.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] when the backend cannot create it. The wgpu
    /// backend reports an invalid descriptor through wgpu's own validation.
    fn create_buffer(&self, descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError>;

    /// Creates a new GPU buffer initialized with `data`.
    ///
    /// Cheaper than `create_buffer` then `write_buffer` for static data.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] when the backend cannot create it. The wgpu
    /// backend reports an invalid descriptor through wgpu's own validation.
    fn create_buffer_with_data(
        &self,
        descriptor: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError>;

    /// Destroys a GPU buffer, releasing its memory.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live buffer.
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError>;

    /// Writes data to a specific region of a GPU buffer.
    ///
    /// # Arguments
    /// * `id`: The identifier of the buffer to write to.
    /// * `offset`: The offset in bytes from the beginning of the buffer to start writing.
    /// * `data`: The slice of bytes to write into the buffer.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live buffer or the write
    /// runs past its end.
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError>;

    /// Writes data to a GPU buffer without stalling, for large uploads.
    ///
    /// # Errors
    /// The future resolves to a [`ResourceError`] when `id` names no live buffer
    /// or the mapping fails.
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static>;

    // --- Texture & Sampler Management ---

    /// Creates a new GPU texture.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] when the backend cannot create it. The wgpu
    /// backend reports an invalid descriptor through wgpu's own validation.
    fn create_texture(&self, descriptor: &TextureDescriptor) -> Result<TextureId, ResourceError>;

    /// Destroys a GPU texture, releasing its memory.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live texture.
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError>;

    /// Writes data to a specific region of a GPU texture.
    ///
    /// # Arguments
    /// * `texture_id`: The identifier of the texture to write to.
    /// * `data`: The raw image data to write.
    /// * `bytes_per_row`: The number of bytes for a single row of texels in `data`.
    /// * `offset`: The 3D offset (x, y, z/layer) in the texture to start writing.
    /// * `size`: The 3D extent (width, height, depth/layers) of the data to write.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `texture_id` names no live texture.
    fn write_texture(
        &self,
        texture_id: TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: dimension::Origin3D,
        size: dimension::Extent3D,
    ) -> Result<(), ResourceError>;

    /// Creates a view of `texture_id`: how a shader reads its format and mip levels.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `texture_id` names no live texture.
    fn create_texture_view(
        &self,
        texture_id: TextureId,
        descriptor: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError>;

    /// Destroys a texture view.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live view.
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError>;

    /// Creates a sampler: how a shader filters and wraps texture reads.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] when the backend cannot create it. The wgpu
    /// backend reports an invalid descriptor through wgpu's own validation.
    fn create_sampler(&self, descriptor: &SamplerDescriptor) -> Result<SamplerId, ResourceError>;

    /// Destroys a sampler.
    ///
    /// # Errors
    /// Returns a [`ResourceError`] if `id` names no live sampler.
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError>;

    // --- Command Management ---

    /// Creates a new command encoder to record GPU commands.
    ///
    /// # Arguments
    /// * `label`: An optional debug label for the command encoder.
    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder>;

    /// Submits a previously recorded command buffer to the GPU's command queue for execution.
    fn submit_command_buffer(&self, command_buffer: CommandBufferId);

    // --- Device Introspection ---

    /// Gets the texture format of the primary render surface.
    fn get_surface_format(&self) -> Option<TextureFormat>;

    /// Returns the `(width, height)` of the primary render surface, in pixels.
    fn get_surface_size(&self) -> (u32, u32);

    /// Gets information about the active graphics adapter (GPU).
    fn get_adapter_info(&self) -> GraphicsAdapterInfo;

    /// Returns the adapter's vendor, device and driver, or `None` if unknown.
    ///
    /// It names the machine's GPU in its
    /// [`MachineIdentity`](crate::platform::MachineIdentity).
    fn adapter_identity(&self) -> Option<crate::renderer::api::device::AdapterIdentity> {
        None
    }

    /// Checks if a specific, optional rendering feature is supported by the backend.
    fn supports_feature(&self, feature_name: &str) -> bool;

    /// Returns the capabilities a lane may need; the default reports none.
    ///
    /// Read once at boot to decide which lanes stay on their families' menus.
    fn caps(&self) -> crate::gpu::DeviceCaps {
        crate::gpu::DeviceCaps::default()
    }
}
