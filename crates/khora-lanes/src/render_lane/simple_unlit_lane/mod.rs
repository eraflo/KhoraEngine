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

//! Implements a simple, unlit rendering strategy.
//!
//! The `SimpleUnlitLane` is the most basic rendering pipeline in Khora. It renders
//! meshes without any lighting calculations, making it the fastest and most straightforward
//! rendering strategy. This lane is ideal for:
//! - Debug visualization and prototyping
//! - Rendering UI elements or 2D sprites
//! - Performance-critical scenarios where lighting is not needed
//! - Serving as a fallback when more complex rendering strategies cannot meet their budget
//!
//! As a "Lane" in the CLAD architecture, this implementation is optimized for raw speed
//! and deterministic execution. It contains minimal branching logic and is designed to
//! be driven by a higher-level `RenderAgent`.

use khora_core::{
    asset::Material,
    renderer::api::{
        pipeline::enums::PrimitiveTopology, pipeline::RenderPipelineId, scene::GpuMesh,
    },
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::{OnceLock, RwLock};

mod gpu;
mod render;

/// A lane that implements a simple, unlit forward rendering strategy.
///
/// This lane takes the extracted scene data from a `RenderWorld` and generates
/// GPU commands to render all meshes with a basic, unlit appearance. It does not
/// perform any lighting calculations, shadow mapping, or post-processing effects.
///
/// # Performance Characteristics
/// - **Zero heap allocations** during the render pass encoding
/// - **Linear iteration** over the extracted mesh list
/// - **Minimal state changes** (one pipeline bind per material, ideally)
/// - **Suitable for**: High frame rates, simple scenes, or as a debug/fallback renderer
pub struct SimpleUnlitLane {
    // Init-once handles (set in `on_initialize`, read every frame).
    // `OnceLock` gives lock-free reads after the one-time write — no
    // per-frame Mutex contention on the hot path.
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<khora_core::renderer::api::command::BindGroupLayoutId>,
    model_layout: OnceLock<khora_core::renderer::api::command::BindGroupLayoutId>,
    material_layout: OnceLock<khora_core::renderer::api::command::BindGroupLayoutId>,

    // Per-frame mutated ring buffers — `Mutex` is required for exclusive
    // access during `advance` / `write` / `push`.
    camera_ring: std::sync::Mutex<
        Option<khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer>,
    >,
    model_ring: std::sync::Mutex<
        Option<khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer>,
    >,
    material_ring: std::sync::Mutex<
        Option<khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer>,
    >,
}

impl Default for SimpleUnlitLane {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleUnlitLane {
    /// Creates a new `SimpleUnlitLane`.
    pub fn new() -> Self {
        Self {
            pipeline: OnceLock::new(),
            camera_layout: OnceLock::new(),
            model_layout: OnceLock::new(),
            material_layout: OnceLock::new(),
            camera_ring: std::sync::Mutex::new(None),
            model_ring: std::sync::Mutex::new(None),
            material_ring: std::sync::Mutex::new(None),
        }
    }
}

impl khora_core::lane::Lane for SimpleUnlitLane {
    fn strategy_name(&self) -> &'static str {
        "SimpleUnlit"
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
        let shadow_atlas = ctx.get::<khora_core::lane::ShadowAtlasView>().map(|v| v.0);
        let shadow_sampler = ctx
            .get::<khora_core::lane::ShadowComparisonSampler>()
            .map(|v| v.0);

        let mut render_ctx = khora_core::renderer::api::core::RenderContext::new(
            &color_target,
            Some(&depth_target),
            clear_color,
        );
        render_ctx.shadow_atlas = shadow_atlas.as_ref();
        render_ctx.shadow_sampler = shadow_sampler.as_ref();

        self.render(
            render_world,
            device.as_ref(),
            encoder,
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

impl SimpleUnlitLane {
    /// Returns the render pipeline for the given material (or default).
    pub fn get_pipeline_for_material(
        &self,
        _material: Option<&khora_core::asset::AssetHandle<Box<dyn Material>>>,
    ) -> RenderPipelineId {
        // Lock-free read — `OnceLock::get()` returns a borrow without
        // any synchronization once the cell has been initialized.
        self.pipeline.get().copied().unwrap_or(RenderPipelineId(0))
    }

    fn estimate_render_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        let gpu_mesh_assets = crate::lock_or_log!(
            gpu_meshes.read(),
            "SimpleUnlitLane::estimate_render_cost",
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
                    // Lines and points don't contribute to triangle count
                    PrimitiveTopology::LineList
                    | PrimitiveTopology::LineStrip
                    | PrimitiveTopology::PointList => 0,
                };

                total_triangles += triangle_count;
                draw_call_count += 1;
            }
        }

        // Cost model: triangles have a small per-triangle cost,
        // draw calls have a fixed overhead
        const TRIANGLE_COST: f32 = 0.001;
        const DRAW_CALL_COST: f32 = 0.1;

        (total_triangles as f32 * TRIANGLE_COST) + (draw_call_count as f32 * DRAW_CALL_COST)
    }
}

#[cfg(test)]
mod tests;
