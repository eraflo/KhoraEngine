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

use crate::math::dimension::{self as math_dim};
use crate::subsystems::renderer::api::buffer_types::{BufferDescriptor, BufferId};
use crate::subsystems::renderer::api::common_types::RendererAdapterInfo;
use crate::subsystems::renderer::api::pipeline_types::{
    RenderPipelineDescriptor, RenderPipelineId,
};
use crate::subsystems::renderer::api::shader_types::{ShaderModuleDescriptor, ShaderModuleId};
use crate::subsystems::renderer::api::texture_types::{
    SamplerDescriptor, SamplerId, TextureDescriptor, TextureId, TextureViewDescriptor,
    TextureViewId,
};
use crate::subsystems::renderer::error::ResourceError;
use std::fmt::Debug;
use std::future::Future;

pub trait GraphicsDevice: Send + Sync + Debug + 'static {
    /// Creates a shader module from the provided descriptor.
    /// ## Arguments
    /// * `descriptor` - A reference to a `ShaderModuleDescriptor` containing the shader source and other properties.
    /// ## Returns
    /// A `Result` containing the ID of the created shader module or an error if the creation fails.
    /// ## Errors
    /// * `ResourceError` - If the shader module creation fails.
    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError>;

    /// Destroys the shader module associated with the given ID.
    /// This function is used to release the resources associated with the shader module.
    /// ## Arguments
    /// * `id` - The ID of the shader module to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    /// ## Errors
    /// * `ResourceError` - If the shader module destruction fails.
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError>;

    /// Creates a render pipeline from the provided descriptor.
    /// ## Arguments
    /// * `descriptor` - A reference to a `RenderPipelineDescriptor` containing the pipeline configuration.
    /// ## Returns
    /// A `Result` containing the ID of the created render pipeline or an error if the creation fails.
    /// ## Errors
    /// * `ResourceError` - If the render pipeline creation fails.
    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError>;

    /// Destroys the render pipeline associated with the given ID.
    /// This function is used to release the resources associated with the render pipeline.
    /// ## Arguments
    /// * `id` - The ID of the render pipeline to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    /// ## Errors
    /// * `ResourceError` - If the render pipeline destruction fails.
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError>;

    /// Creates a new GPU buffer.
    /// ## Arguments
    /// * `descriptor` - A reference to a `BufferDescriptor` containing the buffer configuration.
    /// ## Returns
    /// A `Result` containing the ID of the created buffer or an error if the creation fails.
    fn create_buffer(&self, descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError>;

    /// Destroys a GPU buffer.
    /// ## Arguments
    /// * `id` - The ID of the buffer to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError>;

    /// Writes data to a GPU buffer.
    /// ## Arguments
    /// * `id` - The ID of the buffer to write to.
    /// * `offset` - The offset in the buffer where the data will be written.
    /// * `data` - A slice of bytes containing the data to be written.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError>;

    /// Writes data to a GPU buffer asynchronously (returns a Future).
    /// This is an optimization for larger data uploads.
    /// ## Arguments
    /// * `id` - The ID of the buffer to write to.
    /// * `offset` - The offset in the buffer where the data will be written.
    /// * `data` - A slice of bytes containing the data to be written.
    /// ## Returns
    /// A `Box` containing a `Future` that resolves to a `Result` indicating success or failure of the operation.
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'a>;

    /// Creates a new GPU texture.
    /// ## Arguments
    /// * `descriptor` - A reference to a `TextureDescriptor` containing the texture configuration.
    /// ## Returns
    /// A `Result` containing the ID of the created texture or an error if the creation fails.
    fn create_texture(&self, descriptor: &TextureDescriptor) -> Result<TextureId, ResourceError>;

    /// Destroys a GPU texture.
    /// ## Arguments
    /// * `id` - The ID of the texture to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError>;

    /// Writes data to a GPU texture.
    /// ## Arguments
    /// * `texture_id` - The ID of the texture to write to.
    /// * `data` - A slice of bytes containing the data to be written.
    /// * `bytes_per_row` - The number of bytes per row in the texture data.
    /// * `offset` - The offset in the texture where the data will be written.
    /// * `size` - The size of the texture.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn write_texture(
        &self,
        texture_id: TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: math_dim::Origin3D,
        size: math_dim::Extent3D,
    ) -> Result<(), ResourceError>;

    /// Creates a new texture view for a given texture.
    /// ## Arguments
    /// * `texture_id` - The ID of the texture for which the view will be created.
    /// * `descriptor` - A reference to a `TextureViewDescriptor` containing the view configuration.
    /// ## Returns
    /// A `Result` containing the ID of the created texture view or an error if the creation fails.
    fn create_texture_view(
        &self,
        texture_id: TextureId,
        descriptor: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError>;

    /// Destroys a texture view.
    /// ## Arguments
    /// * `id` - The ID of the texture view to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError>;

    /// Creates a new sampler.
    /// ## Arguments
    /// * `descriptor` - A reference to a `SamplerDescriptor` containing the sampler configuration.
    /// ## Returns
    /// A `Result` containing the ID of the created sampler or an error if the creation fails.
    fn create_sampler(&self, descriptor: &SamplerDescriptor) -> Result<SamplerId, ResourceError>;

    /// Destroys a sampler.
    /// ## Arguments
    /// * `id` - The ID of the sampler to be destroyed.
    /// ## Returns
    /// A `Result` indicating success or failure of the operation.
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError>;

    /// Get the adapter information of the rendering system.
    fn get_adapter_info(&self) -> RendererAdapterInfo;

    /// Indicate if a specific feature is supported.
    fn supports_feature(&self, feature_name: &str) -> bool;
}
