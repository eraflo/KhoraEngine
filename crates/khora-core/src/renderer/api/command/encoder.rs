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

//! Opaque handles and basic command structures.

/// An opaque handle to a recorded command buffer that is ready for submission.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CommandBufferId(pub u64);

/// A generalized draw command containing all necessary state and bindings for a single draw call.
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// The render pipeline to use for this draw call.
    pub pipeline: crate::renderer::api::pipeline::RenderPipelineId,
    /// The vertex buffer containing the geometry.
    pub vertex_buffer: crate::renderer::api::resource::BufferId,
    /// The index buffer defining the draw order.
    pub index_buffer: crate::renderer::api::resource::BufferId,
    /// The format of the indices (16-bit or 32-bit).
    pub index_format: crate::renderer::api::util::IndexFormat,
    /// The number of indices to draw.
    pub index_count: u32,
    /// An optional bind group for model-specific uniforms (typically group 1).
    pub model_bind_group: Option<crate::renderer::api::command::BindGroupId>,
    /// Optional dynamic offset for the model bind group.
    pub model_offset: u32,
    /// An optional bind group for material-specific uniforms (typically group 2).
    pub material_bind_group: Option<crate::renderer::api::command::BindGroupId>,
    /// Optional dynamic offset for the material bind group.
    pub material_offset: u32,
}
