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

use crate::subsystems::renderer::api::common_types::RendererAdapterInfo;
use crate::subsystems::renderer::api::shader_types::{ShaderModuleDescriptor, ShaderModuleId};
use crate::subsystems::renderer::api::pipeline_types::{RenderPipelineDescriptor, RenderPipelineId};
use crate::subsystems::renderer::error::ResourceError;
use std::fmt::Debug;

pub trait GraphicsDevice: Send + Sync + Debug {
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
        descriptor: &RenderPipelineDescriptor
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

    /// Get the adapter information of the rendering system.
    fn get_adapter_info(&self) -> RendererAdapterInfo;

    /// Indicate if a specific feature is supported.
    fn supports_feature(&self, feature_name: &str) -> bool;
}
