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

//! Forward+ (Tiled Forward) rendering lane implementation.
//!
//! This module implements a Forward+ rendering strategy that uses a compute shader
//! to perform per-tile light culling before the main render pass. This approach
//! significantly reduces the number of lights processed per fragment, making it
//! ideal for scenes with many lights (>20).
//!
//! # Architecture
//!
//! The Forward+ pipeline works in two stages:
//!
//! 1. **Light Culling (Compute Pass)**: The screen is divided into tiles (16x16 pixels).
//!    For each tile, the compute shader determines which lights intersect the tile's
//!    frustum and builds a list of affecting light indices.
//!
//! 2. **Rendering (Render Pass)**: Each fragment looks up its tile's light list and
//!    only evaluates lighting for those specific lights, rather than all lights in the scene.
//!
//! # Performance Characteristics
//!
//! - **O(tiles × lights)** for light culling (compute pass)
//! - **O(fragments × lights_per_tile)** for shading (render pass)
//! - **Suitable for**: Scenes with many lights (>20)
//! - **Break-even point**: ~20 lights (vs standard forward rendering)
//!
//! # SAA Compliance (Symbiotic Adaptive Architecture)
//!
//! This lane integrates with GORNA through:
//! - `estimate_cost()`: Provides accurate cost estimation including compute overhead
//! - Configurable tile size and max lights per tile
//! - Runtime-adjustable configuration via `ForwardPlusTileConfig`

use crate::render_lane::ShaderComplexity;
use khora_data::render::RenderWorld;

use khora_core::{
    asset::Material,
    renderer::{
        api::{pipeline::enums::PrimitiveTopology, pipeline::RenderPipelineId, scene::GpuMesh},
        traits::CommandEncoder,
        ForwardPlusTileConfig,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

mod gpu;
mod gpu_resources;
mod render;

pub use gpu_resources::ForwardPlusGpuResources;

// --- Cost Estimation Constants ---

/// Base cost per triangle rendered.
const TRIANGLE_COST: f32 = 0.001;

/// Cost per draw call issued.
const DRAW_CALL_COST: f32 = 0.1;

/// Fixed overhead for the compute pass (tile frustum + dispatch).
const COMPUTE_PASS_OVERHEAD: f32 = 0.5;

/// Cost factor per tile in the light culling pass.
const PER_TILE_COST: f32 = 0.0001;

/// Cost factor per light-tile intersection test.
const LIGHT_TILE_TEST_COST: f32 = 0.00001;

/// Binding indices inside the group-3 *lighting* bind group, mirroring
/// `forward_plus.wgsl`. Bindings 1/2/3 belong to the shared shadow
/// contract (`khora_core::renderer::api::shadow::bindings::binding`) — Forward+
/// owns 0, 4, 5, 6, 7 around them. See the canonical render bind-group
/// convention in `.agent/conventions.md`.
mod g3 {
    /// `lights` storage buffer.
    pub const LIGHTS: u32 = 0;
    /// `light_indices` storage buffer (per-tile light lists).
    pub const LIGHT_INDICES: u32 = 4;
    /// `light_grid` storage buffer (per-tile offset/count pairs).
    pub const LIGHT_GRID: u32 = 5;
    /// `tile_info` uniform buffer.
    pub const TILE_INFO: u32 = 6;
    /// `light_shadow_view_projs` storage buffer.
    pub const SHADOW_VIEW_PROJS: u32 = 7;
}

/// A rendering lane that implements Forward+ (Tiled Forward) rendering.
///
/// Forward+ divides the screen into tiles and uses a compute shader to determine
/// which lights affect each tile before the main render pass. This significantly
/// reduces per-fragment lighting cost for scenes with many lights.
///
/// # Configuration
///
/// The lane is configured via `ForwardPlusTileConfig`, which controls:
/// - **Tile size**: 16x16 or 32x32 pixels (trade-off between culling granularity and overhead)
/// - **Max lights per tile**: Memory budget for per-tile light lists
/// - **Depth pre-pass**: Optional optimization for depth-bounded light culling
pub struct ForwardPlusLane {
    /// Tile configuration for light culling.
    pub tile_config: ForwardPlusTileConfig,

    /// Shader complexity for cost estimation.
    pub shader_complexity: ShaderComplexity,

    /// Current screen dimensions (for tile count calculation).
    screen_size: (u32, u32),

    /// GPU resources for compute and render passes.
    pub gpu_resources: std::sync::Mutex<ForwardPlusGpuResources>,

    /// Backend pipeline system, retained so the render path can resolve the
    /// per-material-variant pipeline (cheap cache hit). Set in `on_gpu_init`.
    pipeline_system:
        std::sync::OnceLock<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>,
}

impl Default for ForwardPlusLane {
    fn default() -> Self {
        Self {
            tile_config: ForwardPlusTileConfig::default(),
            shader_complexity: ShaderComplexity::SimpleLit,
            screen_size: (1920, 1080),
            gpu_resources: std::sync::Mutex::new(ForwardPlusGpuResources::default()),
            pipeline_system: std::sync::OnceLock::new(),
        }
    }
}

impl ForwardPlusLane {
    /// Creates a new `ForwardPlusLane` with default settings.
    ///
    /// Default configuration:
    /// - Tile size: 16x16 pixels
    /// - Max lights per tile: 128
    /// - No depth pre-pass
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new `ForwardPlusLane` with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The tile configuration for light culling
    pub fn with_config(config: ForwardPlusTileConfig) -> Self {
        Self {
            tile_config: config,
            ..Default::default()
        }
    }

    /// Creates a new `ForwardPlusLane` with the specified shader complexity.
    pub fn with_complexity(complexity: ShaderComplexity) -> Self {
        Self {
            shader_complexity: complexity,
            ..Default::default()
        }
    }

    /// Updates the screen size used for tile calculations.
    ///
    /// This should be called when the window is resized to recalculate
    /// tile counts and buffer sizes.
    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_size = (width, height);
    }

    /// Calculates the number of tiles in each dimension.
    pub fn tile_count(&self) -> (u32, u32) {
        self.tile_config
            .tile_dimensions(self.screen_size.0, self.screen_size.1)
    }

    /// Calculates the total number of tiles on screen.
    pub fn total_tiles(&self) -> u32 {
        let (tiles_x, tiles_y) = self.tile_count();
        tiles_x * tiles_y
    }

    /// Returns the effective number of lights in the scene.
    ///
    /// This counts all light types (directional, point, spot) that will be
    /// processed by the light culling pass.
    pub fn effective_light_count(&self, render_world: &RenderWorld) -> usize {
        render_world.directional_light_count()
            + render_world.point_light_count()
            + render_world.spot_light_count()
    }

    /// Estimates the cost of the compute pass (light culling).
    fn compute_pass_cost(&self, render_world: &RenderWorld) -> f32 {
        let total_tiles = self.total_tiles() as f32;
        let light_count = self.effective_light_count(render_world) as f32;

        // Compute pass cost = overhead + per-tile cost + light-tile tests
        COMPUTE_PASS_OVERHEAD
            + (total_tiles * PER_TILE_COST)
            + (total_tiles * light_count * LIGHT_TILE_TEST_COST)
    }

    /// Calculates the per-fragment light cost factor.
    ///
    /// For Forward+, this uses sqrt(total_lights) instead of linear scaling
    /// because lights are culled per-tile, so each fragment only processes
    /// a subset of lights.
    fn fragment_light_factor(&self, render_world: &RenderWorld) -> f32 {
        let total_lights = self.effective_light_count(render_world) as f32;

        if total_lights == 0.0 {
            return 1.0;
        }

        // Sublinear scaling: sqrt(lights) because of tile culling
        // Clamped to max_lights_per_tile
        let effective_lights = total_lights
            .sqrt()
            .min(self.tile_config.max_lights_per_tile as f32);

        1.0 + (effective_lights * 0.02)
    }
}

impl khora_core::lane::Lane for ForwardPlusLane {
    fn strategy_name(&self) -> &'static str {
        "ForwardPlus"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Render
    }

    fn estimate_cost(&self, ctx: &khora_core::lane::LaneContext) -> f32 {
        let render_world = match ctx.get_ref::<khora_data::render::RenderWorld>() {
            Some(render_world) => render_world,
            None => return 1.0,
        };
        let gpu_meshes = match ctx.get::<std::sync::Arc<
            std::sync::RwLock<
                khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
            >,
        >>() {
            Some(arc) => arc,
            None => return 1.0,
        };
        self.estimate_render_cost(render_world, gpu_meshes)
    }

    fn on_initialize(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(khora_core::lane::LaneError::missing(
                "Arc<dyn GraphicsDevice>",
            ))?
            .clone();
        let pipeline_system = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .ok_or(khora_core::lane::LaneError::missing(
                "Arc<dyn PipelineSystem>",
            ))?
            .clone();
        // Retain the system so the render hot path can resolve a pipeline per
        // material variant (a cheap cache hit after first compile).
        let _ = self.pipeline_system.set(pipeline_system.clone());
        self.on_gpu_init(device.as_ref(), pipeline_system.as_ref())
            .map_err(|e| khora_core::lane::LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::LaneError;
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let gpu_meshes = ctx
            .get::<std::sync::Arc<
                std::sync::RwLock<
                    khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
                >,
            >>()
            .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?
            .clone();
        let mut encoder_guard = ctx
            .slot::<dyn khora_core::renderer::traits::CommandEncoder>()
            .ok_or(LaneError::missing("&mut dyn CommandEncoder"))?;
        let encoder = &mut *encoder_guard;
        let render_world = ctx
            .get_ref::<khora_data::render::RenderWorld>()
            .ok_or(LaneError::missing("&RenderWorld"))?;
        let color_target = ctx
            .get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?
            .0;
        let depth_target = ctx
            .get::<khora_core::lane::DepthTarget>()
            .ok_or(LaneError::missing("DepthTarget"))?
            .0;
        let clear_color = ctx
            .get::<khora_core::lane::ClearColor>()
            .ok_or(LaneError::missing("ClearColor"))?
            .0;

        let render_ctx = khora_core::renderer::api::core::RenderContext::new(
            &color_target,
            Some(&depth_target),
            clear_color,
        );

        // Read the per-frame `ShadowFrame` published by whichever shadow
        // strategy ran. Mirror of LitForwardLane's pattern — single
        // cross-lane channel.
        let (shadow_entries, shadow_bindings) = ctx
            .slot::<khora_core::lane::OutputDeck>()
            .map(|mut s| {
                let frame = s.slot::<khora_core::renderer::api::shadow::ShadowFrame>();
                (frame.entries.clone(), frame.bindings)
            })
            .unwrap_or_default();

        let ibl_bindings = ctx
            .get::<khora_core::renderer::api::ibl::IblGpuBindings>()
            .copied();

        // The second encoder, when the caller splits its passes. Absent in
        // harnesses that drive the lane directly.
        let mut transparent_encoder = ctx.slot_as::<khora_data::render::TransparentEncoder, dyn khora_core::renderer::traits::CommandEncoder>();

        self.render(
            render_world,
            &shadow_entries,
            shadow_bindings,
            ibl_bindings,
            device.as_ref(),
            encoder,
            transparent_encoder
                .as_deref_mut()
                .map(|encoder| encoder as &mut dyn CommandEncoder),
            &render_ctx,
            &gpu_meshes,
        );
        Ok(())
    }

    fn on_shutdown(&self, ctx: &mut khora_core::lane::LaneContext) {
        if let Some(device) = ctx.get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
        {
            self.on_gpu_shutdown(device.as_ref());
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl ForwardPlusLane {
    /// Returns the render pipeline for the given material (or default).
    pub fn get_pipeline_for_material(
        &self,
        _material: Option<&khora_core::asset::AssetHandle<Box<dyn Material>>>,
    ) -> RenderPipelineId {
        // Return the stored pipeline. Fallback to pipeline 0 if on_gpu_init
        // hasn't run yet OR if the mutex is poisoned (degraded rendering
        // rather than crashing the frame loop — R7).
        let resources = crate::lock_or_log!(
            self.gpu_resources.lock(),
            "ForwardPlusLane::get_pipeline_for_material",
            RenderPipelineId(0)
        );
        resources.render_pipeline.unwrap_or(RenderPipelineId(0))
    }

    fn estimate_render_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        let gpu_mesh_assets = crate::lock_or_log!(
            gpu_meshes.read(),
            "ForwardPlusLane::estimate_render_cost",
            0.0
        );

        let mut total_triangles = 0u32;
        let mut draw_call_count = 0u32;

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                let triangle_count = match gpu_mesh.primitive_topology {
                    PrimitiveTopology::TriangleList => gpu_mesh.index_count / 3,
                    PrimitiveTopology::TriangleStrip => {
                        if gpu_mesh.index_count >= 3 {
                            gpu_mesh.index_count - 2
                        } else {
                            0
                        }
                    }
                    PrimitiveTopology::LineList
                    | PrimitiveTopology::LineStrip
                    | PrimitiveTopology::PointList => 0,
                };

                total_triangles += triangle_count;
                draw_call_count += 1;
            }
        }

        // Base geometry cost
        let geometry_cost =
            (total_triangles as f32 * TRIANGLE_COST) + (draw_call_count as f32 * DRAW_CALL_COST);

        // Shader complexity multiplier
        let shader_multiplier = self.shader_complexity.cost_multiplier();

        // Compute pass overhead
        let compute_cost = self.compute_pass_cost(render_world);

        // Per-fragment light factor (sublinear for Forward+)
        let light_factor = self.fragment_light_factor(render_world);

        // Total cost
        compute_cost + (geometry_cost * shader_multiplier * light_factor)
    }
}

#[cfg(test)]
mod tests;
