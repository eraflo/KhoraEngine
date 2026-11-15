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
use crate::render::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::command::{BindGroupDescriptor, BindGroupEntry};
use khora_core::renderer::api::pipeline::{ComputePipelineId, RenderPipelineId};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferId, BufferUsage};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::{ForwardPlusTileConfig, GraphicsDevice};
use std::borrow::Cow;

/// Light slots per tile in the culling shader's light-index list.
///
/// It is `MAX_LIGHTS_PER_TILE` of `light_culling.wgsl`, which indexes the list
/// at `tile * MAX_LIGHTS_PER_TILE`; a test keeps them equal.
pub(crate) const CULLING_SHADER_LIGHTS_PER_TILE: u32 = 128;

/// The buffers, layouts, rings and pipelines Forward+ culls and draws with.
#[derive(Debug, Default)]
pub struct ForwardPlusGpuResources {
    /// Every light of the frame, as `GpuLight`.
    pub light_buffer: Option<BufferId>,
    /// Each tile's list of light indices.
    pub light_index_buffer: Option<BufferId>,
    /// Each tile's (offset, count) pair into the light indices.
    pub light_grid_buffer: Option<BufferId>,
    /// The tile grid the fragment shader reads.
    pub tile_info_buffer: Option<BufferId>,
    /// The culling pass's parameters.
    pub culling_uniforms_buffer: Option<BufferId>,
    /// Each light's 2D shadow view-projection, indexed as `light_buffer` is.
    pub shadow_view_projs_buffer: Option<BufferId>,

    /// The group-0 (camera) layout.
    pub camera_layout: Option<BindGroupLayoutId>,
    /// The group-1 (model) layout.
    pub model_layout: Option<BindGroupLayoutId>,
    /// The group-2 (material) layout.
    pub material_layout: Option<BindGroupLayoutId>,
    /// The group-3 (lighting) layout: lights, shadows, culling results and IBL.
    pub lighting_layout: Option<BindGroupLayoutId>,
    /// The culling pass's layout.
    pub culling_layout: Option<BindGroupLayoutId>,

    /// The camera uniforms, one buffer per frame in flight.
    pub camera_ring: Option<UniformRingBuffer>,
    /// The model uniforms, one element per draw.
    pub model_ring: Option<DynamicUniformRingBuffer>,

    /// The culling pass's bind group.
    pub culling_bind_group: Option<BindGroupId>,
    /// How many tiles the light-index and light-grid buffers hold (0 = none).
    pub tile_capacity: u32,
    /// The light-culling compute pipeline.
    pub culling_pipeline: Option<ComputePipelineId>,
    /// The plain surface's render pipeline, warmed when the lane is prepared.
    pub render_pipeline: Option<RenderPipelineId>,
}

impl ForwardPlusGpuResources {
    /// Returns whether the lane's buffers, layouts and culling pipeline are built.
    ///
    /// The light lists are not part of it: their size follows the target, so the
    /// first frame makes them (`ensure_tile_buffers`).
    pub fn is_initialized(&self) -> bool {
        self.light_buffer.is_some()
            && self.culling_uniforms_buffer.is_some()
            && self.culling_layout.is_some()
            && self.culling_pipeline.is_some()
    }

    /// Grows the light lists and the culling bind group over them to hold `grid`.
    ///
    /// A grid they already hold keeps them.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] before the lane is built, or when the device
    /// refuses a buffer or the bind group; the previous lists stay in place.
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
