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

use super::tile_grid::{grown_tile_capacity, TileGrid};
use crate::render_lane::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::command::{BindGroupDescriptor, BindGroupEntry};
use khora_core::renderer::api::pipeline::{ComputePipelineId, RenderPipelineId};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferId, BufferUsage};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::{ForwardPlusTileConfig, GraphicsDevice};
use std::borrow::Cow;

/// Light slots per tile in the culling shader's light-index list: the
/// `MAX_LIGHTS_PER_TILE` constant of `light_culling.wgsl`, which indexes the
/// list at `tile * MAX_LIGHTS_PER_TILE`. Kept equal to it by a test.
pub(crate) const CULLING_SHADER_LIGHTS_PER_TILE: u32 = 128;

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
    /// How many tiles the light-index and light-grid buffers hold (0 = none).
    pub tile_capacity: u32,
    /// Compute pipeline for light culling.
    pub culling_pipeline: Option<ComputePipelineId>,
    /// Render pipeline for the Forward+ pass.
    pub render_pipeline: Option<RenderPipelineId>,
}

impl ForwardPlusGpuResources {
    /// Returns true once the lane's GPU initialisation has run.
    ///
    /// The light-index and light-grid buffers are not part of it: their size
    /// depends on the colour target, so the first frame allocates them
    /// ([`ensure_tile_buffers`](Self::ensure_tile_buffers)).
    pub fn is_initialized(&self) -> bool {
        self.light_buffer.is_some()
            && self.culling_uniforms_buffer.is_some()
            && self.culling_layout.is_some()
            && self.culling_pipeline.is_some()
    }

    /// Grows the light-index and light-grid buffers (and the culling bind
    /// group over them) so they hold `grid`; a grid they already hold keeps
    /// them. On error the previous buffers are left in place.
    pub(crate) fn ensure_tile_buffers(
        &mut self,
        device: &dyn GraphicsDevice,
        config: &ForwardPlusTileConfig,
        grid: &TileGrid,
    ) -> Result<(), ResourceError> {
        let Some(tiles) = grown_tile_capacity(self.tile_capacity, grid.total()) else {
            return Ok(());
        };
        let (Some(culling_layout), Some(culling_uniforms), Some(lights)) = (
            self.culling_layout,
            self.culling_uniforms_buffer,
            self.light_buffer,
        ) else {
            return Err(ResourceError::BackendError(
                "ForwardPlusGpuResources: light lists requested before GPU initialisation".into(),
            ));
        };

        let tiles_u64 = u64::from(tiles);
        let u32_size = std::mem::size_of::<u32>() as u64;
        let storage = BufferUsage::STORAGE | BufferUsage::COPY_DST;
        // One slot per light a tile can hold, then an (offset, count) pair per
        // tile. The culling shader lays tile `t`'s lights out at
        // `t * CULLING_SHADER_LIGHTS_PER_TILE` whatever the config says, so the
        // stride never drops below it.
        let lights_per_tile = config
            .max_lights_per_tile
            .max(CULLING_SHADER_LIGHTS_PER_TILE);
        let index_size = tiles_u64 * u64::from(lights_per_tile) * u32_size;
        let grid_size = tiles_u64 * 2 * u32_size;

        let index = device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("Forward+ Light Index Buffer")),
            size: index_size,
            usage: storage,
            mapped_at_creation: false,
        })?;
        let light_grid = match device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("Forward+ Light Grid Buffer")),
            size: grid_size,
            usage: storage,
            mapped_at_creation: false,
        }) {
            Ok(buffer) => buffer,
            Err(e) => {
                let _ = device.destroy_buffer(index);
                return Err(e);
            }
        };
        let culling_bind_group = match device.create_bind_group(&BindGroupDescriptor {
            label: Some("Forward+ Culling Bind Group"),
            layout: culling_layout,
            entries: &[
                BindGroupEntry::buffer(0, culling_uniforms, 0, None),
                BindGroupEntry::buffer(1, lights, 0, None),
                BindGroupEntry::buffer(2, index, 0, None),
                BindGroupEntry::buffer(3, light_grid, 0, None),
            ],
        }) {
            Ok(bind_group) => bind_group,
            Err(e) => {
                let _ = device.destroy_buffer(index);
                let _ = device.destroy_buffer(light_grid);
                return Err(e);
            }
        };

        // The replacements exist: only now free what they replace.
        if let Some(old) = self.culling_bind_group.replace(culling_bind_group) {
            let _ = device.destroy_bind_group(old);
        }
        if let Some(old) = self.light_index_buffer.replace(index) {
            let _ = device.destroy_buffer(old);
        }
        if let Some(old) = self.light_grid_buffer.replace(light_grid) {
            let _ = device.destroy_buffer(old);
        }
        self.tile_capacity = tiles;
        Ok(())
    }
}
