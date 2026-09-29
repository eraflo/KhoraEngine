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

//! Implements a lit forward rendering strategy with shader complexity tracking.
//!
//! The `LitForwardLane` is a rendering pipeline that performs lighting calculations
//! in the fragment shader using a forward rendering approach. It supports multiple
//! light types and tracks shader complexity for GORNA resource negotiation.
//!
//! # Shader Complexity Tracking
//!
//! The cost estimation for this lane includes a shader complexity factor that scales
//! with the number of lights in the scene. This allows GORNA to make informed decisions
//! about rendering strategy selection based on performance budgets.

use crate::render_lane::ShaderComplexity;
use khora_core::renderer::api::command::BindGroupLayoutId;

use crate::render_lane::util::DynamicUniformRingBuffer;
use crate::render_lane::util::UniformRingBuffer;
use khora_core::asset::Material;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::RwLock;

mod gpu;
mod render;

/// Constants for cost estimation.
const TRIANGLE_COST: f32 = 0.001;

const DRAW_CALL_COST: f32 = 0.1;

/// Cost multiplier per light in the scene.
const LIGHT_COST_FACTOR: f32 = 0.05;

/// A lane that implements forward rendering with lighting support.
///
/// This lane renders meshes with lighting calculations performed in the fragment
/// shader. It supports multiple light types (directional, point, spot) and
/// includes shader complexity tracking for GORNA resource negotiation.
///
/// # Performance Characteristics
///
/// - **O(meshes × lights)** fragment shader complexity
/// - **Suitable for**: Scenes with moderate light counts (< 20 lights)
/// - **Shader complexity tracking**: Integrates with GORNA for adaptive quality
///
/// # Cost Estimation
///
/// The cost estimation includes:
/// - Base triangle and draw call costs (same as `SimpleUnlitLane`)
/// - Shader complexity multiplier based on the configured complexity level
/// - Per-light cost scaling based on the number of active lights
pub struct LitForwardLane {
    /// The shader complexity level to use for cost estimation.
    pub shader_complexity: ShaderComplexity,
    /// Maximum number of directional lights supported per pass.
    pub max_directional_lights: u32,
    /// Maximum number of point lights supported per pass.
    pub max_point_lights: u32,
    /// Maximum number of spot lights supported per pass.
    pub max_spot_lights: u32,
    /// The stored render pipeline handle (lock-free init-once).
    pipeline: std::sync::OnceLock<RenderPipelineId>,
    /// Backend pipeline system, retained so the render path can resolve the
    /// per-material-variant pipeline (cheap cache hit). Set in `on_gpu_init`.
    pipeline_system:
        std::sync::OnceLock<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>,
    /// Layout for Camera (Group 0).
    camera_layout: std::sync::OnceLock<BindGroupLayoutId>,
    /// Layout for Model (Group 1).
    model_layout: std::sync::OnceLock<BindGroupLayoutId>,
    /// Layout for Material (Group 2).
    material_layout: std::sync::OnceLock<BindGroupLayoutId>,
    /// Layout for Lighting (Group 3) — full layout with shadow atlas + sampler for pipeline.
    light_layout: std::sync::OnceLock<BindGroupLayoutId>,
    /// Layout for the lighting uniform buffer only (1 binding) — used by the ring buffer.
    lighting_buffer_layout: std::sync::OnceLock<BindGroupLayoutId>,
    /// Persistent ring buffer for camera uniforms (eliminates per-frame allocation).
    camera_ring: std::sync::Mutex<Option<UniformRingBuffer>>,
    /// Persistent ring buffer for lighting uniforms (eliminates per-frame allocation).
    lighting_ring: std::sync::Mutex<Option<UniformRingBuffer>>,
    /// Per-frame model (transform) uniforms — dynamic offset per draw.
    model_ring: std::sync::Mutex<Option<DynamicUniformRingBuffer>>,
}

impl Default for LitForwardLane {
    fn default() -> Self {
        Self {
            shader_complexity: ShaderComplexity::SimpleLit,
            max_directional_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 8,
            pipeline: std::sync::OnceLock::new(),
            pipeline_system: std::sync::OnceLock::new(),
            camera_layout: std::sync::OnceLock::new(),
            model_layout: std::sync::OnceLock::new(),
            material_layout: std::sync::OnceLock::new(),
            light_layout: std::sync::OnceLock::new(),
            lighting_buffer_layout: std::sync::OnceLock::new(),
            camera_ring: std::sync::Mutex::new(None),
            lighting_ring: std::sync::Mutex::new(None),
            model_ring: std::sync::Mutex::new(None),
        }
    }
}

impl LitForwardLane {
    /// Creates a new `LitForwardLane` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new `LitForwardLane` with the specified shader complexity.
    pub fn with_complexity(complexity: ShaderComplexity) -> Self {
        Self {
            shader_complexity: complexity,
            ..Default::default()
        }
    }

    /// Returns the effective number of lights that will be used for rendering.
    ///
    /// This clamps the actual light counts to the maximum supported per pass.
    pub fn effective_light_counts(&self, render_world: &RenderWorld) -> (usize, usize, usize) {
        let dir_count = render_world
            .directional_light_count()
            .min(self.max_directional_lights as usize);
        let point_count = render_world
            .point_light_count()
            .min(self.max_point_lights as usize);
        let spot_count = render_world
            .spot_light_count()
            .min(self.max_spot_lights as usize);

        (dir_count, point_count, spot_count)
    }

    /// Calculates the light-based cost factor for the current frame.
    fn light_cost_factor(&self, render_world: &RenderWorld) -> f32 {
        let (dir_count, point_count, spot_count) = self.effective_light_counts(render_world);
        let total_lights = dir_count + point_count + spot_count;

        // Base cost of 1.0 even with no lights (ambient only)
        1.0 + (total_lights as f32 * LIGHT_COST_FACTOR)
    }
}

impl khora_core::lane::Lane for LitForwardLane {
    fn strategy_name(&self) -> &'static str {
        "LitForward"
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
                khora_data::assets::Assets<khora_core::renderer::api::gpu_scene::GpuMesh>,
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
                    khora_data::assets::Assets<khora_core::renderer::api::gpu_scene::GpuMesh>,
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
        let render_ctx = khora_core::renderer::api::frame::RenderContext::new(
            &color_target,
            Some(&depth_target),
            clear_color,
        );

        // Read the per-frame `ShadowFrame` published by whichever shadow
        // strategy ran. `bindings` may be `None` when the strategy
        // hasn't initialised yet — the consumer falls back to skipping
        // the lit render in that case. `entries` is always present
        // (possibly empty).
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

        self.render(
            render_world,
            &shadow_entries,
            shadow_bindings,
            ibl_bindings,
            device.as_ref(),
            encoder,
            ctx.slot_as::<khora_data::render::TransparentEncoder, dyn khora_core::renderer::traits::CommandEncoder>()
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

impl LitForwardLane {
    /// Returns the render pipeline for the given material (or default).
    pub fn get_pipeline_for_material(
        &self,
        _material: Option<&khora_core::asset::AssetHandle<Box<dyn Material>>>,
    ) -> RenderPipelineId {
        // Lock-free read — `OnceLock` initialized in `on_gpu_init`.
        self.pipeline.get().copied().unwrap_or(RenderPipelineId(0))
    }

    fn estimate_render_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        let gpu_mesh_assets = crate::lock_or_log!(
            gpu_meshes.read(),
            "LitForwardLane::estimate_render_cost",
            0.0
        );

        let mut total_triangles = 0u32;
        let mut draw_call_count = 0u32;

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // Calculate triangle count based on primitive topology
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

        // Base cost from triangles and draw calls
        let base_cost =
            (total_triangles as f32 * TRIANGLE_COST) + (draw_call_count as f32 * DRAW_CALL_COST);

        // Apply shader complexity multiplier
        let shader_factor = self.shader_complexity.cost_multiplier();

        // Apply light-based cost scaling
        let light_factor = self.light_cost_factor(render_world);

        // Total cost combines all factors
        base_cost * shader_factor * light_factor
    }
}

#[cfg(test)]
mod tests;
