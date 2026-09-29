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

//! The GPU buffers and bind groups Forward+ keeps across frames.

use crate::render_lane::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::pipeline::{ComputePipelineId, RenderPipelineId};
use khora_core::renderer::api::resource::BufferId;

// --- ForwardPlusLane ---

/// GPU resource handles for the Forward+ compute pass.
///
/// These are created during lane initialization and used each frame
/// for light culling and rendering.
#[derive(Debug, Default)]
pub struct ForwardPlusGpuResources {
    /// Buffer containing all GpuLight instances.
    pub light_buffer: Option<BufferId>,
    /// Buffer containing per-tile light index lists.
    pub light_index_buffer: Option<BufferId>,
    /// Buffer containing (offset, count) pairs per tile.
    pub light_grid_buffer: Option<BufferId>,
    /// Buffer containing tile info for the fragment shader.
    pub tile_info_buffer: Option<BufferId>,
    /// Uniform buffer for culling parameters.
    pub culling_uniforms_buffer: Option<BufferId>,
    /// Storage buffer with per-light 2D shadow view-projection matrices,
    /// indexed identically to the `light_buffer`.
    pub shadow_view_projs_buffer: Option<BufferId>,

    /// Bind group layout for Group 0 (Camera).
    pub camera_layout: Option<BindGroupLayoutId>,
    /// Bind group layout for Group 1 (Model).
    pub model_layout: Option<BindGroupLayoutId>,
    /// Bind group layout for Group 2 (Material).
    pub material_layout: Option<BindGroupLayoutId>,
    /// Bind group layout for Group 3 — the lighting domain: light list,
    /// shadow atlases, per-tile culling results + shadow view-projs.
    pub lighting_layout: Option<BindGroupLayoutId>,
    /// Bind group layout for Culling compute pass.
    pub culling_layout: Option<BindGroupLayoutId>,

    /// Ring buffer for camera uniforms.
    pub camera_ring: Option<UniformRingBuffer>,
    /// Ring buffer for model uniforms.
    pub model_ring: Option<DynamicUniformRingBuffer>,

    /// Bind group for the culling compute shader.
    pub culling_bind_group: Option<BindGroupId>,
    /// Compute pipeline for light culling.
    pub culling_pipeline: Option<ComputePipelineId>,
    /// Render pipeline for the Forward+ pass.
    pub render_pipeline: Option<RenderPipelineId>,
}

impl ForwardPlusGpuResources {
    /// Returns true if all required resources are initialized.
    pub fn is_initialized(&self) -> bool {
        self.light_buffer.is_some()
            && self.light_index_buffer.is_some()
            && self.light_grid_buffer.is_some()
            && self.culling_uniforms_buffer.is_some()
            && self.culling_bind_group.is_some()
            && self.culling_pipeline.is_some()
    }
}
