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

use khora_core::renderer::api::{
    command::BindGroupLayoutId,
    util::{
        dynamic_uniform_buffer::DynamicUniformRingBuffer, uniform_ring_buffer::UniformRingBuffer,
    },
};
use khora_core::{
    asset::Material,
    renderer::{
        api::{
            command::{
                BindGroupId, ComputePassDescriptor, ComputePipelineId, LoadOp, Operations,
                RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
                StoreOp,
            },
            core::RenderContext,
            pipeline::enums::PrimitiveTopology,
            pipeline::{
                ComputePipelineSpec, LayoutKey, LayoutSpec, PipelineSpec, RenderPipelineId,
                ShaderVariantKey,
            },
            resource::{BufferId, CameraUniformData},
            scene::GpuMesh,
        },
        traits::CommandEncoder,
        ForwardPlusTileConfig,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

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
/// contract (`khora_data::render::shadow_bindings::binding`) — Forward+
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
        let render_world = match ctx.get::<khora_core::lane::Ref<khora_data::render::RenderWorld>>()
        {
            Some(slot) => slot.get(),
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
        use khora_core::lane::{LaneError, Ref, Slot};
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
        let encoder = ctx
            .get::<Slot<dyn khora_core::renderer::traits::CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
            .get();
        let render_world = ctx
            .get::<Ref<khora_data::render::RenderWorld>>()
            .ok_or(LaneError::missing("Ref<RenderWorld>"))?
            .get();
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
            .get::<Slot<khora_core::lane::OutputDeck>>()
            .map(|s| {
                let frame = s
                    .get()
                    .slot::<khora_core::renderer::api::shadow::ShadowFrame>();
                (frame.entries.clone(), frame.bindings)
            })
            .unwrap_or_default();

        self.render(
            render_world,
            &shadow_entries,
            shadow_bindings,
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

    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        render_world: &RenderWorld,
        shadow_entries: &khora_data::render::ShadowEntries,
        shadow_bindings: Option<khora_data::render::ShadowGpuBindings>,
        device: &dyn khora_core::renderer::GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) {
        let mut resources =
            crate::lock_or_log!(self.gpu_resources.lock(), "ForwardPlusLane::render");

        // 1. Get Active Camera View.
        //
        // When no camera is present (e.g. editor viewport before any in-scene
        // camera is added), a clear render pass must still be submitted
        let Some(view) = render_world.views.first() else {
            let color_attachment = khora_core::renderer::api::command::RenderPassColorAttachment {
                view: render_ctx.color_target,
                resolve_target: None,
                ops: khora_core::renderer::api::command::Operations {
                    load: khora_core::renderer::api::command::LoadOp::Clear(render_ctx.clear_color),
                    store: khora_core::renderer::api::command::StoreOp::Store,
                },
                base_array_layer: 0,
            };
            let clear_desc = khora_core::renderer::api::command::RenderPassDescriptor {
                label: Some("ForwardPlus Clear-Only Pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
                    khora_core::renderer::api::command::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(khora_core::renderer::api::command::Operations {
                            load: khora_core::renderer::api::command::LoadOp::Clear(1.0),
                            store: khora_core::renderer::api::command::StoreOp::Store,
                        }),
                        stencil_ops: None,
                        base_array_layer: 0,
                    }
                }),
            };
            let _ = encoder.begin_render_pass(&clear_desc);
            return;
        };

        // 2. Prepare Camera Uniforms (Group 0)
        let camera_uniforms = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = if let Some(ref mut ring) = resources.camera_ring {
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group()
        } else {
            return;
        };

        // 3. Prepare Tile Info (sent to Group 3)
        if let Some(tile_buffer) = resources.tile_info_buffer {
            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());
            let tile_info = [
                num_tiles_x,
                num_tiles_y,
                config.tile_size.pixels(),
                config.max_lights_per_tile,
            ];
            if let Err(e) = device.write_buffer(tile_buffer, 0, bytemuck::cast_slice(&tile_info)) {
                log::error!("ForwardPlusLane::render: tile info buffer write failed: {e:?}");
            }
        }

        // 4. Update Light Data — fold per-light shadow metadata from the
        // ShadowFrame into each GpuLight, and accumulate a parallel
        // `shadow_view_projs` buffer indexed identically to `lights`.
        //
        // Directional / spot: `shadow_map_index` = 2D atlas layer,
        // `shadow_view_projs[i]` carries the per-light VP matrix.
        // Point: `shadow_map_index` = cube atlas index,
        // `shadow_far_plane` = perspective far plane used by the shadow
        // pass (matches `ShadowEntry::Cube.far_plane`).
        use khora_data::render::ShadowEntry;
        let lights: Vec<_> = render_world
            .lights
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut gl = khora_core::renderer::GpuLight::from_parts(
                    [l.position.x, l.position.y, l.position.z],
                    [l.direction.x, l.direction.y, l.direction.z],
                    &l.light_type,
                );
                match shadow_entries.get(i) {
                    Some(ShadowEntry::Atlas2D { atlas_index, .. }) => {
                        gl.shadow_map_index = *atlas_index;
                    }
                    Some(ShadowEntry::Cube {
                        cube_array_index,
                        far_plane,
                        ..
                    }) => {
                        gl.shadow_map_index = *cube_array_index;
                        gl.shadow_far_plane = *far_plane;
                    }
                    None => {}
                }
                gl
            })
            .collect();

        if let Some(light_buffer) = resources.light_buffer {
            if let Err(e) = device.write_buffer(light_buffer, 0, bytemuck::cast_slice(&lights)) {
                log::error!("ForwardPlusLane::render: light data buffer write failed: {e:?}");
            }
        }

        // Parallel shadow view-projection matrices — directional / spot
        // pull from the entry's `view_proj`, everything else gets the
        // identity (bypassed via `shadow_map_index < 0` early-out in
        // `sample_shadow_pcf`).
        let shadow_view_projs: Vec<[[f32; 4]; 4]> = render_world
            .lights
            .iter()
            .enumerate()
            .map(|(i, _)| match shadow_entries.get(i) {
                Some(ShadowEntry::Atlas2D { view_proj, .. }) => view_proj.to_cols_array_2d(),
                _ => khora_core::math::Mat4::IDENTITY.to_cols_array_2d(),
            })
            .collect();

        if let Some(svp_buffer) = resources.shadow_view_projs_buffer {
            if !shadow_view_projs.is_empty() {
                if let Err(e) =
                    device.write_buffer(svp_buffer, 0, bytemuck::cast_slice(&shadow_view_projs))
                {
                    log::error!(
                        "ForwardPlusLane::render: shadow view-proj buffer write failed: {e:?}"
                    );
                }
            }
        }

        // Prepare and write Culling Uniforms
        if let Some(culling_buffer) = resources.culling_uniforms_buffer {
            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());

            let inv_vp = view.view_proj.inverse().unwrap_or_default();

            let culling_data = khora_core::renderer::api::scene::CullingUniformsData {
                view_projection: view.view_proj.to_cols_array_2d(),
                inverse_projection: inv_vp.to_cols_array_2d(),
                screen_dimensions: [width as f32, height as f32],
                tile_count: [num_tiles_x, num_tiles_y],
                num_lights: lights.len() as u32,
                tile_size: config.tile_size.pixels(),
                _padding: [0.0; 2],
            };

            if let Err(e) =
                device.write_buffer(culling_buffer, 0, bytemuck::bytes_of(&culling_data))
            {
                log::error!("ForwardPlusLane::render: culling uniforms buffer write failed: {e:?}");
            }
        }

        // Run Culling Compute Pass
        if let (Some(culling_pipeline), Some(culling_bg)) =
            (resources.culling_pipeline, resources.culling_bind_group)
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Forward+ Light Culling Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&culling_pipeline);
            compute_pass.set_bind_group(0, &culling_bg, &[]);

            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());
            compute_pass.dispatch_workgroups(num_tiles_x, num_tiles_y, 1);
        }

        // 5. Prepare Per-Mesh Data (Dynamic Uniforms)
        let mut draw_commands = Vec::new();

        if let Some(ref mut ring) = resources.model_ring {
            ring.advance();
        }

        // Fallback pipeline (empty variant) used only if the per-variant
        // resolve fails.
        let fallback_pipeline = resources.render_pipeline.unwrap_or(RenderPipelineId(0));

        let gpu_mesh_assets = crate::lock_or_log!(gpu_meshes.read(), "ForwardPlusLane::render");
        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // The material projection tags every rendered entity with a
                // `GpuMaterial` before RenderFlow runs.
                let Some(gpu_material) = &extracted_mesh.gpu_material else {
                    continue;
                };

                // Resolve the pipeline for this material's texture variant
                // (cheap cache hit after first compile; never retained).
                let pipeline_id = self
                    .pipeline_system
                    .get()
                    .map(|ps| {
                        ps.pipeline(
                            device,
                            &render_pipeline_spec(
                                device,
                                gpu_material.variant.clone(),
                                gpu_material.double_sided,
                            ),
                        )
                    })
                    .transpose()
                    .unwrap_or_else(|e| {
                        log::error!("ForwardPlusLane: pipeline resolve failed: {:?}", e);
                        None
                    })
                    .unwrap_or(fallback_pipeline);

                // Compute Matrices
                let model_mat = extracted_mesh.transform.to_matrix();
                let normal_mat = model_mat.inverse().unwrap_or_default().transpose();

                let model_uniforms = khora_core::renderer::api::scene::ModelUniforms {
                    model_matrix: model_mat.to_cols_array_2d(),
                    normal_matrix: normal_mat.to_cols_array_2d(),
                };

                // Model transform → dynamic ring; material → cached GpuMaterial.
                let (model_bg, model_offset) = if let Some(ref mut ring) = resources.model_ring {
                    let offset = match ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                        Ok(off) => off,
                        Err(_) => continue,
                    };
                    (*ring.current_bind_group(), offset)
                } else {
                    continue;
                };

                draw_commands.push(khora_core::renderer::api::command::DrawCommand {
                    pipeline: pipeline_id,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_count: gpu_mesh_handle.index_count,
                    index_format: gpu_mesh_handle.index_format,
                    model_bind_group: Some(model_bg),
                    model_offset,
                    material_bind_group: Some(gpu_material.bind_group),
                    material_offset: 0,
                });
            }
        }

        // Batch by pipeline (variant): one `set_pipeline` per variant.
        draw_commands.sort_by_key(|cmd| cmd.pipeline.0);

        // 6. Render Pass
        let color_attachment = RenderPassColorAttachment {
            view: render_ctx.color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(render_ctx.clear_color),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
        };

        let render_pass_desc = RenderPassDescriptor {
            label: Some("ForwardPlus Render Pass"),
            color_attachments: &[color_attachment],
            depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
                RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                    base_array_layer: 0,
                }
            }),
        };

        // Build the per-frame group-3 (lighting) bind group. It packs
        // the lighting domain into one group per the canonical 4-group
        // render convention: the light list (0), the shadow atlases
        // (1/2/3, from the shared `khora::shadow::bindings` contract),
        // the per-tile culling results (4/5/6) and the shadow
        // view-projections (7). It mixes persistent light buffers with
        // the per-frame shadow atlas views, so it cannot be cached.
        //
        // If no shadow strategy ran this frame, skip — wgpu requires
        // every pipeline-declared group to be bound.
        let Some(shadow_bindings) = shadow_bindings else {
            log::warn!(
                "ForwardPlusLane: ShadowGpuBindings not available (shadow agent inactive?), skipping render"
            );
            return;
        };
        let (
            Some(lighting_layout),
            Some(light_buffer),
            Some(light_index_buffer),
            Some(light_grid_buffer),
            Some(tile_info_buffer),
            Some(shadow_view_projs_buffer),
        ) = (
            resources.lighting_layout,
            resources.light_buffer,
            resources.light_index_buffer,
            resources.light_grid_buffer,
            resources.tile_info_buffer,
            resources.shadow_view_projs_buffer,
        )
        else {
            log::warn!("ForwardPlusLane: lighting GPU resources not initialized, skipping render");
            return;
        };

        use khora_core::renderer::api::command::{BindGroupDescriptor, BindGroupEntry};
        let mut lighting_entries: Vec<BindGroupEntry> = Vec::with_capacity(8);
        lighting_entries.push(BindGroupEntry::buffer(g3::LIGHTS, light_buffer, 0, None));
        // Bindings 1/2/3 — shadow atlas 2D + sampler + cube atlas.
        khora_data::render::shadow_bindings::fill_shadow_bind_group_entries(
            &shadow_bindings,
            &mut lighting_entries,
        );
        lighting_entries.push(BindGroupEntry::buffer(
            g3::LIGHT_INDICES,
            light_index_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::LIGHT_GRID,
            light_grid_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::TILE_INFO,
            tile_info_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::SHADOW_VIEW_PROJS,
            shadow_view_projs_buffer,
            0,
            None,
        ));
        let lighting_bg = match device.create_bind_group(&BindGroupDescriptor {
            label: Some("forward_plus_lighting_bind_group"),
            layout: lighting_layout,
            entries: &lighting_entries,
        }) {
            Ok(bg) => bg,
            Err(e) => {
                log::error!(
                    "ForwardPlusLane: failed to create lighting bind group: {:?}",
                    e
                );
                return;
            }
        };

        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Bind Group 0: Camera
        render_pass.set_bind_group(0, &camera_bind_group, &[]);

        // Bind Group 3: Lighting (lights + shadows + tile culling).
        render_pass.set_bind_group(3, &lighting_bg, &[]);

        // Draw Cached Commands — set the pipeline once per variant (commands
        // are pre-sorted by pipeline above).
        let mut current_pipeline: Option<RenderPipelineId> = None;
        for cmd in &draw_commands {
            if current_pipeline != Some(cmd.pipeline) {
                render_pass.set_pipeline(&cmd.pipeline);
                current_pipeline = Some(cmd.pipeline);
            }
            if let Some(ref bg) = cmd.model_bind_group {
                render_pass.set_bind_group(1, bg, &[cmd.model_offset]);
            }
            if let Some(ref bg) = cmd.material_bind_group {
                render_pass.set_bind_group(2, bg, &[]);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
        }

        drop(render_pass);
        let _ = device.destroy_bind_group(lighting_bg);
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

    fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use khora_core::renderer::api::{
            command::{BindGroupDescriptor, BindGroupEntry},
            resource::CameraUniformData,
            scene::ModelUniforms,
        };
        use std::borrow::Cow;

        log::info!("ForwardPlusLane: Initializing GPU resources...");

        // Layouts — canonical Camera (group 0) + Material (group 2) come from
        // the backend's cache (shared with the lit lanes); the per-draw Model
        // (group 1), Forward+ lighting (group 3) and the compute culling layout
        // are bespoke inline layouts shared with the lane's pipelines + buffers.
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
        let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
        let model_layout = pipeline_system.inline_layout(
            device,
            FP_MODEL_LAYOUT_LABEL,
            &fp_model_layout_entries(),
        )?;
        let lighting_layout = pipeline_system.inline_layout(
            device,
            FP_LIGHTING_LAYOUT_LABEL,
            &fp_lighting_layout_entries(),
        )?;
        let culling_layout = pipeline_system.inline_layout(
            device,
            FP_CULLING_LAYOUT_LABEL,
            &fp_culling_layout_entries(),
        )?;

        // Pipelines (compiled + cached by the backend). Warm the empty-variant
        // render pipeline (untextured materials); textured variants compile
        // lazily in the render path keyed by `GpuMaterial::variant`.
        let pipeline_id = pipeline_system.pipeline(
            device,
            &render_pipeline_spec(device, ShaderVariantKey::empty(), false),
        )?;
        let culling_pipeline =
            pipeline_system.compute_pipeline(device, &culling_pipeline_spec())?;

        // 3. Create Buffers and Rings

        // Light Data Buffer
        let light_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Buffer")),
                size: 64 * 1024,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Light Index List
        let light_index_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Index Buffer")),
                size: 120 * 68 * 256 * 4,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Light Grid
        let light_grid_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Grid Buffer")),
                size: 120 * 68 * 2 * 4,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Tile Info Buffer
        let tile_info_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Tile Info")),
                size: 256,
                usage: khora_core::renderer::api::resource::BufferUsage::UNIFORM
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Shadow view-projection matrices (one mat4 per light, indexed
        // identically to `light_buffer`). 64 KB matches the light buffer
        // sizing — ~1024 matrices, well above any realistic scene.
        let shadow_view_projs_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Shadow ViewProj Buffer")),
                size: 64 * 1024,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Culling Uniforms
        let culling_uniforms_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Culling Uniforms")),
                size: 256,
                usage: khora_core::renderer::api::resource::BufferUsage::UNIFORM
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Ring Buffers
        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "Forward+ Camera Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_ring = DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
            khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
            "Forward+ Model Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Materials are no longer per-frame ring uniforms: each material is a
        // cached `GpuMaterial` (uniforms + textures + group-2 bind group)
        // produced by the data-layer projection. The lane just binds it.

        // 4. Bind Groups

        let culling_bg = device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Forward+ Culling Bind Group"),
                layout: culling_layout,
                entries: &[
                    BindGroupEntry::buffer(0, culling_uniforms_buffer, 0, None),
                    BindGroupEntry::buffer(1, light_buffer, 0, None),
                    BindGroupEntry::buffer(2, light_index_buffer, 0, None),
                    BindGroupEntry::buffer(3, light_grid_buffer, 0, None),
                ],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // The group-3 (lighting) bind group is rebuilt every frame in
        // `render` because it combines the persistent light buffers with
        // the per-frame shadow atlas views — it cannot be cached here.

        // 5. Store all resources
        let mut res = self.gpu_resources.lock().map_err(|_| {
            khora_core::renderer::error::RenderError::ResourceError(
                khora_core::renderer::error::ResourceError::BackendError(
                    "ForwardPlusLane gpu_resources mutex poisoned".into(),
                ),
            )
        })?;
        res.light_buffer = Some(light_buffer);
        res.light_index_buffer = Some(light_index_buffer);
        res.light_grid_buffer = Some(light_grid_buffer);
        res.tile_info_buffer = Some(tile_info_buffer);
        res.culling_uniforms_buffer = Some(culling_uniforms_buffer);
        res.shadow_view_projs_buffer = Some(shadow_view_projs_buffer);
        res.camera_layout = Some(camera_layout);
        res.model_layout = Some(model_layout);
        res.material_layout = Some(material_layout);
        res.lighting_layout = Some(lighting_layout);
        res.culling_layout = Some(culling_layout);
        res.camera_ring = Some(camera_ring);
        res.model_ring = Some(model_ring);
        res.culling_bind_group = Some(culling_bg);
        res.culling_pipeline = Some(culling_pipeline);
        res.render_pipeline = Some(pipeline_id);

        Ok(())
    }

    fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        let mut resources = crate::lock_or_log!(
            self.gpu_resources.lock(),
            "ForwardPlusLane::on_gpu_shutdown"
        );

        if let Some(ring) = resources.camera_ring.take() {
            ring.destroy(device);
        }
        if let Some(ring) = resources.model_ring.take() {
            ring.destroy(device);
        }

        if let Some(id) = resources.light_buffer.take() {
            device.destroy_buffer(id).ok();
        }
        if let Some(id) = resources.light_index_buffer.take() {
            device.destroy_buffer(id).ok();
        }
        if let Some(id) = resources.light_grid_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
        if let Some(id) = resources.culling_uniforms_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
        if let Some(id) = resources.shadow_view_projs_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
    }
}

// ─── Free functions (CLAD: declarative specs + bespoke layouts) ───

/// Stable cache label for the Forward+ per-draw model layout.
const FP_MODEL_LAYOUT_LABEL: &str = "forward_plus_model_layout";
/// Stable cache label for the Forward+ group-3 lighting layout.
const FP_LIGHTING_LAYOUT_LABEL: &str = "forward_plus_lighting_layout";
/// Stable cache label for the Forward+ compute culling layout.
const FP_CULLING_LAYOUT_LABEL: &str = "forward_plus_culling_layout";

/// Bespoke group-1 (model) layout: a single dynamic-offset uniform buffer.
fn fp_model_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::scene::ModelUniforms;
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<ModelUniforms>() as u64),
        },
    }]
}

/// Bespoke group-3 (lighting) layout — the canonical 4-group render
/// convention's lighting domain. One bind group holds every lighting input:
/// the light list (0), the shadow atlases (1/2/3, shared
/// `khora::shadow::bindings` contract), the per-tile culling results (4/5/6)
/// and the per-light shadow view-projections (7).
fn fp_lighting_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{BindGroupLayoutEntry, BufferBindingType};
    use khora_core::renderer::api::util::ShaderStageFlags;
    let mut entries: Vec<BindGroupLayoutEntry> = vec![BindGroupLayoutEntry::buffer(
        g3::LIGHTS,
        ShaderStageFlags::FRAGMENT,
        BufferBindingType::Storage { read_only: true },
        false,
        None,
    )];
    entries.extend(khora_data::render::shadow_bindings::shadow_bind_group_layout_entries());
    entries.extend([
        BindGroupLayoutEntry::buffer(
            g3::LIGHT_INDICES,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::LIGHT_GRID,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::TILE_INFO,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Uniform,
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::SHADOW_VIEW_PROJS,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
    ]);
    entries
}

/// Bespoke compute culling layout (compute pass side).
fn fp_culling_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{BindGroupLayoutEntry, BufferBindingType};
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![
        BindGroupLayoutEntry::buffer(
            0,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Uniform,
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            1,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            2,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: false },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            3,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: false },
            false,
            None,
        ),
    ]
}

/// The declarative render pipeline spec for Forward+ under a given material
/// variant — built each call, deduped by the `PipelineSystem`. Canonical
/// Camera/Material; bespoke Model + Lighting. The group-2 (Material) layout
/// resolves to the variant's texture-binding set.
fn render_pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
    variant: ShaderVariantKey,
    double_sided: bool,
) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        CompareFunction, CullMode, VertexFormat, VertexStepMode,
    };
    use khora_core::renderer::api::pipeline::state::{
        ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
        PrimitiveStateDescriptor, VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    };
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    PipelineSpec {
        label: "ForwardPlus Pipeline",
        shader: "khora::pipelines::forward_plus",
        variant,
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Inline {
                label: FP_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(fp_model_layout_entries()),
            },
            LayoutSpec::Named(LayoutKey::Material),
            LayoutSpec::Inline {
                label: FP_LIGHTING_LAYOUT_LABEL,
                entries: Cow::Owned(fp_lighting_layout_entries()),
            },
        ],
        vertex_buffers: vec![VertexBufferLayoutDescriptor {
            array_stride: 32,
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vec![
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
            ]),
        }],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            // Single-sided materials cull back faces; double-sided disable
            // culling. The cull mode is part of the pipeline cache key.
            cull_mode: if double_sided {
                None
            } else {
                Some(CullMode::Back)
            },
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: CompareFunction::Less,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState::default(),
        }),
        color_targets: vec![ColorTargetStateDescriptor {
            format: device
                .get_surface_format()
                .unwrap_or(TextureFormat::Rgba8UnormSrgb),
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// The declarative compute pipeline spec for Forward+ light culling.
fn culling_pipeline_spec() -> ComputePipelineSpec {
    use std::borrow::Cow;
    ComputePipelineSpec {
        label: "Forward+ Culling Pipeline",
        shader: "khora::pipelines::light_culling",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![LayoutSpec::Inline {
            label: FP_CULLING_LAYOUT_LABEL,
            entries: Cow::Owned(fp_culling_layout_entries()),
        }],
        entry_point: "cs_main",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::Lane;
    use khora_core::renderer::TileSize;

    #[test]
    fn test_forward_plus_lane_creation() {
        let lane = ForwardPlusLane::new();
        assert_eq!(lane.tile_config.tile_size, TileSize::X16);
        assert_eq!(lane.tile_config.max_lights_per_tile, 128);
        assert_eq!(lane.shader_complexity, ShaderComplexity::SimpleLit);
    }

    #[test]
    fn test_forward_plus_lane_with_config() {
        let config = ForwardPlusTileConfig {
            tile_size: TileSize::X32,
            max_lights_per_tile: 256,
            use_depth_prepass: true,
        };
        let lane = ForwardPlusLane::with_config(config);

        assert_eq!(lane.tile_config.tile_size, TileSize::X32);
        assert_eq!(lane.tile_config.max_lights_per_tile, 256);
        assert!(lane.tile_config.use_depth_prepass);
    }

    #[test]
    fn test_tile_count_calculation() {
        let mut lane = ForwardPlusLane::new();
        lane.set_screen_size(1920, 1080);

        let (tiles_x, tiles_y) = lane.tile_count();
        assert_eq!(tiles_x, 120); // 1920 / 16
        assert_eq!(tiles_y, 68); // ceil(1080 / 16)
    }

    #[test]
    fn test_strategy_name() {
        let lane = ForwardPlusLane::new();
        assert_eq!(lane.strategy_name(), "ForwardPlus");
    }

    #[test]
    fn test_pipeline_id() {
        let lane = ForwardPlusLane::new();
        // No GPU init → pipeline not yet created → fallback to RenderPipelineId(0)
        let pipeline = lane.get_pipeline_for_material(None);
        assert_eq!(pipeline, RenderPipelineId(0));
    }
}
