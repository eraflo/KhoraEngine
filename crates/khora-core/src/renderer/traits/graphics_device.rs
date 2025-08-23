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
use crate::renderer::api::*;
use crate::renderer::error::ResourceError;
use crate::renderer::traits::CommandEncoder;
use std::fmt::Debug;
use std::future::Future;

/// Defines the abstract interface for a graphics device.
///
/// This trait is the central point of interaction with the underlying graphics API
/// (like WGPU, Vulkan, etc.). It abstracts away the specifics of the backend and
/// provides a unified API for creating, managing, and destroying GPU resources such
/// as buffers, textures, and pipelines.
///
/// It is the cornerstone of the `khora-infra` crate's responsibility, where a
/// concrete type will implement this trait to provide actual rendering capabilities.
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
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError>;

    // --- Buffer Management ---

    /// Creates a new GPU buffer.
    fn create_buffer(&self, descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError>;

    /// Creates a new GPU buffer and initializes it with the provided data.
    /// This is often more efficient than `create_buffer` followed by `write_buffer` for static data.
    fn create_buffer_with_data(
        &self,
        descriptor: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError>;

    /// Destroys a GPU buffer, releasing its memory.
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError>;

    /// Writes data to a specific region of a GPU buffer.
    ///
    /// # Arguments
    /// * `id`: The identifier of the buffer to write to.
    /// * `offset`: The offset in bytes from the beginning of the buffer to start writing.
    /// * `data`: The slice of bytes to write into the buffer.
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError>;

    /// Asynchronously writes data to a GPU buffer.
    /// This can be more performant for large data uploads by avoiding stalls.
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static>;

    // --- Texture & Sampler Management ---

    /// Creates a new GPU texture.
    fn create_texture(&self, descriptor: &TextureDescriptor) -> Result<TextureId, ResourceError>;

    /// Destroys a GPU texture, releasing its memory.
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError>;

    /// Writes data to a specific region of a GPU texture.
    ///
    /// # Arguments
    /// * `texture_id`: The identifier of the texture to write to.
    /// * `data`: The raw image data to write.
    /// * `bytes_per_row`: The number of bytes for a single row of texels in `data`.
    /// * `offset`: The 3D offset (x, y, z/layer) in the texture to start writing.
    /// * `size`: The 3D extent (width, height, depth/layers) of the data to write.
    fn write_texture(
        &self,
        texture_id: TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: dimension::Origin3D,
        size: dimension::Extent3D,
    ) -> Result<(), ResourceError>;

    /// Creates a new texture view for a given texture.
    /// A view describes how a shader will interpret a texture's data (e.g., its format, mip levels).
    fn create_texture_view(
        &self,
        texture_id: TextureId,
        descriptor: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError>;

    /// Destroys a texture view.
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError>;

    /// Creates a new sampler.
    /// A sampler defines how a shader will sample from a texture (e.g., filtering, wrapping).
    fn create_sampler(&self, descriptor: &SamplerDescriptor) -> Result<SamplerId, ResourceError>;

    /// Destroys a sampler.
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

    /// Gets information about the active graphics adapter (GPU).
    fn get_adapter_info(&self) -> RendererAdapterInfo;

    /// Checks if a specific, optional rendering feature is supported by the backend.
    fn supports_feature(&self, feature_name: &str) -> bool;
}
