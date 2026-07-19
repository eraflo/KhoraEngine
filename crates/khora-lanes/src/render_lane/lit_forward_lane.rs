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

use khora_core::renderer::api::command::BindGroupLayoutId;

use khora_core::renderer::api::pipeline::{LayoutKey, LayoutSpec, PipelineSpec, ShaderVariantKey};
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer;
use khora_core::{
    asset::Material,
    renderer::{
        api::{
            command::{
                LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
                RenderPassDescriptor, StoreOp,
            },
            core::RenderContext,
            pipeline::enums::PrimitiveTopology,
            pipeline::RenderPipelineId,
            scene::{
                DirectionalLightUniform, GpuMesh, LightingUniforms, ModelUniforms,
                PointLightUniform, SpotLightUniform, MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS,
                MAX_SPOT_LIGHTS,
            },
        },
        traits::CommandEncoder,
    },
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::RwLock;

/// Constants for cost estimation.
const TRIANGLE_COST: f32 = 0.001;
const DRAW_CALL_COST: f32 = 0.1;
/// Cost multiplier per light in the scene.
const LIGHT_COST_FACTOR: f32 = 0.05;

/// Shader complexity levels for resource budgeting and GORNA negotiation.
///
/// This enum represents the relative computational cost of different shader
/// configurations, allowing the rendering system to communicate workload
/// estimates to the resource allocation system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ShaderComplexity {
    /// No lighting calculations, vertex colors only.
    /// Fastest rendering path.
    Unlit,
    /// Basic Lambertian diffuse + simple specular.
    /// Moderate performance cost.
    #[default]
    SimpleLit,
    /// Full PBR with Cook-Torrance BRDF.
    /// Highest quality, highest cost.
    FullPBR,
}

impl ShaderComplexity {
    /// Returns a cost multiplier for the given complexity level.
    ///
    /// This multiplier is applied to the base rendering cost to estimate
    /// the total GPU workload for different shader configurations.
    pub fn cost_multiplier(&self) -> f32 {
        match self {
            ShaderComplexity::Unlit => 1.0,
            ShaderComplexity::SimpleLit => 1.5,
            ShaderComplexity::FullPBR => 2.5,
        }
    }

    /// Returns a human-readable name for this complexity level.
    pub fn name(&self) -> &'static str {
        match self {
            ShaderComplexity::Unlit => "Unlit",
            ShaderComplexity::SimpleLit => "SimpleLit",
            ShaderComplexity::FullPBR => "FullPBR",
        }
    }
}

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
        // strategy ran. `bindings` may be `None` when the strategy
        // hasn't initialised yet — the consumer falls back to skipping
        // the lit render in that case. `entries` is always present
        // (possibly empty).
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

impl LitForwardLane {
    /// Returns the render pipeline for the given material (or default).
    pub fn get_pipeline_for_material(
        &self,
        _material: Option<&khora_core::asset::AssetHandle<Box<dyn Material>>>,
    ) -> RenderPipelineId {
        // Lock-free read — `OnceLock` initialized in `on_gpu_init`.
        self.pipeline.get().copied().unwrap_or(RenderPipelineId(0))
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
        use khora_core::renderer::api::command::{
            BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding,
        };

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
                label: Some("LitForward Clear-Only Pass"),
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

        // 2. Prepare Global Uniforms via Persistent Ring Buffers
        //    Instead of creating new GPU buffers every frame, we advance the ring
        //    buffer to the next slot and write the updated data in-place.

        // Camera Uniforms — write to persistent ring buffer
        let camera_uniforms = khora_core::renderer::api::resource::CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = {
            let mut lock = crate::lock_or_log!(
                self.camera_ring.lock(),
                "LitForwardLane::render camera_ring"
            );
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("LitForwardLane: camera ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group() // Copy the BindGroupId out
        };

        // Lighting Uniforms — build struct CPU-side, then write to persistent ring buffer
        let mut lighting_uniforms = LightingUniforms {
            directional_lights: [DirectionalLightUniform {
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                shadow_view_proj: [[0.0; 4]; 4],
                shadow_params: [0.0; 4],
            }; MAX_DIRECTIONAL_LIGHTS],
            point_lights: [PointLightUniform {
                position: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                shadow_params: [0.0; 4],
            }; MAX_POINT_LIGHTS],
            spot_lights: [SpotLightUniform {
                position: [0.0; 4],
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                params: [0.0; 4],
                shadow_view_proj: [[0.0; 4]; 4],
                shadow_params: [0.0; 4],
            }; MAX_SPOT_LIGHTS],
            num_directional_lights: 0,
            num_point_lights: 0,
            num_spot_lights: 0,
            _padding: 0,
        };

        for (light_index, light) in render_world.lights.iter().enumerate() {
            let shadow = shadow_entries.get(light_index);

            // Atlas2D / single-matrix path used by directional + spot.
            let (atlas2d_view_proj, atlas2d_index) = match shadow {
                Some(khora_data::render::ShadowEntry::Atlas2D {
                    view_proj,
                    atlas_index,
                }) => (*view_proj, *atlas_index as f32),
                _ => (khora_core::math::Mat4::IDENTITY, -1.0),
            };

            // Cube path used by point lights.
            let (cube_layer, cube_far_plane) = match shadow {
                Some(khora_data::render::ShadowEntry::Cube {
                    cube_array_index,
                    far_plane,
                    ..
                }) => (*cube_array_index as f32, *far_plane),
                _ => (-1.0, 0.0),
            };

            match light.light_type {
                khora_core::renderer::light::LightType::Directional(ref d) => {
                    if (lighting_uniforms.num_directional_lights as usize) < MAX_DIRECTIONAL_LIGHTS
                    {
                        let idx = lighting_uniforms.num_directional_lights as usize;
                        lighting_uniforms.directional_lights[idx] = DirectionalLightUniform {
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                0.0,
                            ],
                            color: d.color.with_alpha(d.intensity),
                            shadow_view_proj: atlas2d_view_proj.to_cols_array_2d(),
                            shadow_params: [
                                atlas2d_index,
                                d.shadow_bias,
                                d.shadow_normal_bias,
                                0.0,
                            ],
                        };
                        lighting_uniforms.num_directional_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Point(ref p) => {
                    if (lighting_uniforms.num_point_lights as usize) < MAX_POINT_LIGHTS {
                        let idx = lighting_uniforms.num_point_lights as usize;
                        // `shadow_params` for point lights:
                        //   x = cube layer (or -1 for none)
                        //   y = depth bias
                        //   z = normal bias
                        //   w = far plane (matches the perspective the
                        //       shadow pass used; the WGSL `sample_point_shadow`
                        //       helper recomputes the depth value using it).
                        lighting_uniforms.point_lights[idx] = PointLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                p.range,
                            ],
                            color: p.color.with_alpha(p.intensity),
                            shadow_params: [
                                cube_layer,
                                p.shadow_bias,
                                p.shadow_normal_bias,
                                cube_far_plane,
                            ],
                        };
                        lighting_uniforms.num_point_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Spot(ref s) => {
                    if (lighting_uniforms.num_spot_lights as usize) < MAX_SPOT_LIGHTS {
                        let idx = lighting_uniforms.num_spot_lights as usize;
                        lighting_uniforms.spot_lights[idx] = SpotLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                s.range,
                            ],
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                s.inner_cone_angle.cos(),
                            ],
                            color: s.color.with_alpha(s.intensity),
                            params: [s.outer_cone_angle.cos(), 0.0, 0.0, 0.0],
                            shadow_view_proj: atlas2d_view_proj.to_cols_array_2d(),
                            shadow_params: [
                                atlas2d_index,
                                s.shadow_bias,
                                s.shadow_normal_bias,
                                0.0,
                            ],
                        };
                        lighting_uniforms.num_spot_lights += 1;
                    }
                }
            }
        }

        let (_lighting_bind_group, lighting_ring_buffer_id) = {
            let mut lock = crate::lock_or_log!(
                self.lighting_ring.lock(),
                "LitForwardLane::render lighting_ring"
            );
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("LitForwardLane: lighting ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&lighting_uniforms)) {
                log::error!("Failed to write lighting ring buffer: {:?}", e);
                return;
            }
            (*ring.current_bind_group(), ring.current_buffer())
        };

        // Shadow data (view-proj matrices, atlas indices) is already embedded
        // in the lighting_uniforms via the ExtractedLight fields patched by
        // ShadowPassLane::execute() Phase 2.  The shadow atlas texture and
        // comparison sampler are passed through the RenderContext.

        // Acquire locks
        let gpu_mesh_assets =
            crate::lock_or_log!(gpu_meshes.read(), "LitForwardLane::render gpu_meshes");

        // Fallback pipeline (empty variant) used only if the per-variant
        // resolve fails; lock-free read via OnceLock.
        let fallback_pipeline = self.pipeline.get().copied().unwrap_or(RenderPipelineId(0));

        // Prepare Draw Commands
        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());

        let mut temp_bind_groups = Vec::new();

        // Model transforms go through the per-frame dynamic ring; materials
        // are consumed from the cached `GpuMaterial` (no per-draw churn).
        let mut model_ring_lock =
            crate::lock_or_log!(self.model_ring.lock(), "LitForwardLane::render model_ring");
        let model_ring = match model_ring_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("LitForwardLane: model ring buffer not initialized");
                return;
            }
        };
        model_ring.advance();

        for extracted_mesh in &render_world.meshes {
            let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) else {
                continue;
            };
            // The material projection tags every rendered entity with a
            // `GpuMaterial` before `RenderFlow` runs.
            let Some(gpu_material) = &extracted_mesh.gpu_material else {
                continue;
            };
            // Resolve the pipeline for this material's texture variant. The
            // backend caches per `(shader, variant, format)` so this is a
            // cheap lookup after first compile; the lane never retains a
            // long-term `RenderPipelineId` per variant.
            let pipeline_id = self
                .pipeline_system
                .get()
                .map(|ps| {
                    ps.pipeline(
                        device,
                        &pipeline_spec(device, gpu_material.variant.clone(), gpu_material.double_sided),
                    )
                })
                .transpose()
                .unwrap_or_else(|e| {
                    log::error!("LitForwardLane: pipeline resolve failed: {:?}", e);
                    None
                })
                .unwrap_or(fallback_pipeline);
            let model_mat = extracted_mesh.transform.to_matrix();
            let normal_mat = if let Some(inverse) = model_mat.inverse() {
                inverse.transpose()
            } else {
                continue;
            };
            let model_uniforms = ModelUniforms {
                model_matrix: model_mat.to_cols_array_2d(),
                normal_matrix: normal_mat.to_cols_array_2d(),
            };
            let model_offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                Ok(offset) => offset,
                Err(e) => {
                    log::error!("LitForwardLane: failed to push model uniforms: {:?}", e);
                    continue;
                }
            };
            let model_bg = *model_ring.current_bind_group();

            draw_commands.push(khora_core::renderer::api::command::DrawCommand {
                pipeline: pipeline_id,
                vertex_buffer: gpu_mesh_handle.vertex_buffer,
                index_buffer: gpu_mesh_handle.index_buffer,
                index_format: gpu_mesh_handle.index_format,
                index_count: gpu_mesh_handle.index_count,
                model_bind_group: Some(model_bg),
                model_offset,
                material_bind_group: Some(gpu_material.bind_group),
                material_offset: 0,
            });
        }

        // Batch by pipeline (variant) so each pipeline is set once across the
        // whole pass — avoids per-draw pipeline thrash when materials mix
        // variants. Stable sort keeps submission order within a variant.
        draw_commands.sort_by_key(|cmd| cmd.pipeline.0);

        // Render Pass
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
            label: Some("Lit Forward Pass"),
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

        // Build Lighting Bind Group. Shadow-related entries (atlas2D,
        // sampler, atlas_cube) are filled by
        // [`khora_data::render::shadow_bindings::fill_shadow_bind_group_entries`]
        // — we never reference the individual texture types here.
        let final_lighting_bind_group = if let Some(layout) = self.light_layout.get().copied() {
            let Some(shadow_bindings) = shadow_bindings else {
                log::warn!(
                    "LitForwardLane: ShadowGpuBindings not available (shadow agent inactive?), skipping lit render"
                );
                return;
            };

            // binding 0 — lighting uniform buffer (lit lane's responsibility)
            let mut entries = vec![BindGroupEntry {
                binding: khora_data::render::shadow_bindings::binding::LIGHTING_UNIFORMS,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: lighting_ring_buffer_id,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }];
            // bindings 1, 2, 3 — shadow lane's responsibility (opaque)
            khora_data::render::shadow_bindings::fill_shadow_bind_group_entries(
                &shadow_bindings,
                &mut entries,
            );

            match device.create_bind_group(&BindGroupDescriptor {
                label: Some("lit_forward_lighting_bind_group_dynamic"),
                layout,
                entries: &entries,
            }) {
                Ok(bg) => {
                    temp_bind_groups.push(bg);
                    bg
                }
                Err(e) => {
                    log::error!(
                        "LitForwardLane: Failed to create lighting bind group: {:?}",
                        e
                    );
                    return;
                }
            }
        } else {
            log::warn!("LitForwardLane: light layout not initialized");
            return;
        };

        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Bind global camera and lighting
        render_pass.set_bind_group(0, &camera_bind_group, &[]);
        render_pass.set_bind_group(3, &final_lighting_bind_group, &[]);

        let mut current_pipeline: Option<RenderPipelineId> = None;

        for cmd in &draw_commands {
            if current_pipeline != Some(cmd.pipeline) {
                render_pass.set_pipeline(&cmd.pipeline);
                current_pipeline = Some(cmd.pipeline);
            }

            if let Some(bg) = &cmd.model_bind_group {
                render_pass.set_bind_group(1, bg, &[cmd.model_offset]);
            }

            if let Some(bg) = &cmd.material_bind_group {
                render_pass.set_bind_group(2, bg, &[]);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);

            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
        }

        // Only the per-frame group-3 lighting bind group is transient now;
        // model uniforms live in the ring, materials in the GpuMaterial cache.
        for bg in temp_bind_groups {
            let _ = device.destroy_bind_group(bg);
        }
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

    fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::util::lock::mutex_lock_render;

        log::info!("LitForwardLane: Initializing GPU resources...");

        // Canonical layouts come from the backend's LayoutCache (deduped across
        // lit lanes + shared with the material projection's group-2 bind group).
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
        let model_layout = pipeline_system.layout(device, LayoutKey::Model, &variant)?;
        let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
        let light_layout = pipeline_system.layout(device, LayoutKey::Lighting, &variant)?;
        let lighting_buffer_layout =
            pipeline_system.layout(device, LayoutKey::LightingBuffer, &variant)?;

        // Warm the empty-variant pipeline (untextured materials). Textured
        // variants are compiled lazily in the render path on first use, keyed
        // by `GpuMaterial::variant`.
        let pipeline_id =
            pipeline_system.pipeline(device, &pipeline_spec(device, ShaderVariantKey::empty(), false))?;

        // Init-once writes — `set` is lock-free; second call returns Err
        // which we ignore (re-init is a logic bug, not a runtime fault).
        let _ = self.camera_layout.set(camera_layout);
        let _ = self.model_layout.set(model_layout);
        let _ = self.material_layout.set(material_layout);
        let _ = self.light_layout.set(light_layout);
        let _ = self.lighting_buffer_layout.set(lighting_buffer_layout);
        let _ = self.pipeline.set(pipeline_id);

        // Persistent ring buffers (per-lane GPU buffers) built against the
        // shared layouts. This eliminates per-frame buffer allocation in the
        // render hot path.
        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>() as u64,
            "Camera Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let lighting_ring = UniformRingBuffer::new(
            device,
            lighting_buffer_layout,
            0,
            std::mem::size_of::<LightingUniforms>() as u64,
            "Lighting Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_ring = DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
            khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
            "LitForward Model Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *mutex_lock_render(&self.camera_ring, "LitForward init.camera_ring")? = Some(camera_ring);
        *mutex_lock_render(&self.lighting_ring, "LitForward init.lighting_ring")? =
            Some(lighting_ring);
        *mutex_lock_render(&self.model_ring, "LitForward init.model_ring")? = Some(model_ring);

        log::info!(
            "LitForwardLane: Persistent ring buffers created (camera: {} bytes, lighting: {} bytes, {} slots each)",
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>(),
            std::mem::size_of::<LightingUniforms>(),
            khora_core::renderer::api::core::MAX_FRAMES_IN_FLIGHT,
        );

        Ok(())
    }

    fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        // Destroy ring buffers first (they own buffers + bind groups).
        // `.lock().ok().and_then(|mut g| g.take())` gracefully degrades
        // to a no-op on poisoning rather than panicking.
        //
        // Bind-group layouts and the pipeline are owned + cached by the
        // `PipelineSystem` backend (shared across lit lanes), so the lane
        // must not destroy them here.
        if let Some(ring) = self.camera_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.lighting_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
    }
}

// ─── Free functions (CLAD: declarative pipeline spec) ───

/// The declarative pipeline spec for LitForward under a given material
/// variant — built each call, deduped by the `PipelineSystem`. Layouts are
/// the canonical 4-group budget; the backend owns + caches them per variant
/// (shared across lit lanes). The group-2 (Material) layout resolves to the
/// variant's texture-binding set.
fn pipeline_spec(
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
        label: "LitForward Pipeline",
        shader: "khora::pipelines::lit_forward",
        variant,
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Named(LayoutKey::Model),
            LayoutSpec::Named(LayoutKey::Material),
            LayoutSpec::Named(LayoutKey::Lighting),
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

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::Lane;
    use khora_core::{
        asset::{AssetHandle, AssetUUID},
        math::{affine_transform::AffineTransform, Mat4},
        renderer::{
            api::{pipeline::enums::PrimitiveTopology, resource::BufferId, util::IndexFormat},
            light::DirectionalLight,
        },
    };
    use khora_data::render::{ExtractedLight, ExtractedMesh};
    use std::sync::Arc;

    fn create_test_gpu_mesh(index_count: u32) -> GpuMesh {
        GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }
    }

    #[test]
    fn test_lit_forward_lane_creation() {
        let lane = LitForwardLane::new();
        assert_eq!(lane.strategy_name(), "LitForward");
        assert_eq!(lane.shader_complexity, ShaderComplexity::SimpleLit);
    }

    #[test]
    fn test_lit_forward_lane_with_complexity() {
        let lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);
        assert_eq!(lane.shader_complexity, ShaderComplexity::FullPBR);
    }

    #[test]
    fn test_shader_complexity_ordering() {
        assert!(ShaderComplexity::Unlit < ShaderComplexity::SimpleLit);
        assert!(ShaderComplexity::SimpleLit < ShaderComplexity::FullPBR);
    }

    #[test]
    fn test_shader_complexity_cost_multipliers() {
        assert_eq!(ShaderComplexity::Unlit.cost_multiplier(), 1.0);
        assert_eq!(ShaderComplexity::SimpleLit.cost_multiplier(), 1.5);
        assert_eq!(ShaderComplexity::FullPBR.cost_multiplier(), 2.5);
    }

    #[test]
    fn test_cost_estimation_empty_world() {
        let lane = LitForwardLane::new();
        let render_world = RenderWorld::default();
        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);
        assert_eq!(cost, 0.0, "Empty world should have zero cost");
    }

    #[test]
    fn test_cost_estimation_with_meshes() {
        let lane = LitForwardLane::new();

        // Create a GPU mesh with 300 indices (100 triangles)
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = create_test_gpu_mesh(300);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
            material: None,
            gpu_material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Base cost without lights: 100 * 0.001 + 1 * 0.1 = 0.2
        // With SimpleLit multiplier (1.5) and no lights (factor 1.0):
        // 0.2 * 1.5 * 1.0 = 0.3
        assert!(
            (cost - 0.3).abs() < 0.0001,
            "Cost should be 0.3 for 100 triangles with SimpleLit complexity, got {}",
            cost
        );
    }

    #[test]
    fn test_cost_estimation_with_lights() {
        use khora_core::{
            math::{Mat4, Vec3},
            renderer::light::LightType,
        };

        let lane = LitForwardLane::new();

        // Create a GPU mesh
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = create_test_gpu_mesh(300);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
            material: None,
            gpu_material: None,
        });

        // Add 4 directional lights
        for _ in 0..4 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Directional(DirectionalLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::new(0.0, -1.0, 0.0),
                shadow_view_proj: Mat4::IDENTITY,
                shadow_atlas_index: None,
            });
        }

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Base cost: 0.2
        // Shader multiplier (SimpleLit): 1.5
        // Light factor: 1.0 + (4 * 0.05) = 1.2
        // Total: 0.2 * 1.5 * 1.2 = 0.36
        assert!(
            (cost - 0.36).abs() < 0.0001,
            "Cost should be 0.36 with 4 lights, got {}",
            cost
        );
    }

    #[test]
    fn test_cost_increases_with_complexity() {
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = create_test_gpu_mesh(300);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
            material: None,
            gpu_material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));

        let unlit_lane = LitForwardLane::with_complexity(ShaderComplexity::Unlit);
        let simple_lane = LitForwardLane::with_complexity(ShaderComplexity::SimpleLit);
        let pbr_lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);

        let unlit_cost = unlit_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);
        let simple_cost = simple_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);
        let pbr_cost = pbr_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        assert!(
            unlit_cost < simple_cost,
            "Unlit should be cheaper than SimpleLit"
        );
        assert!(
            simple_cost < pbr_cost,
            "SimpleLit should be cheaper than PBR"
        );
    }

    #[test]
    fn test_effective_light_counts() {
        use khora_core::{
            math::Vec3,
            renderer::light::{LightType, PointLight},
        };

        let lane = LitForwardLane {
            max_directional_lights: 2,
            max_point_lights: 4,
            max_spot_lights: 2,
            ..Default::default()
        };

        let mut render_world = RenderWorld::default();

        // Add 5 directional lights (max is 2)
        for _ in 0..5 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Directional(DirectionalLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::new(0.0, -1.0, 0.0),
                shadow_view_proj: Mat4::IDENTITY,
                shadow_atlas_index: None,
            });
        }

        // Add 3 point lights (max is 4)
        for _ in 0..3 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Point(PointLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::ZERO,
                shadow_view_proj: Mat4::IDENTITY,
                shadow_atlas_index: None,
            });
        }

        let (dir, point, spot) = lane.effective_light_counts(&render_world);
        assert_eq!(dir, 2, "Should be clamped to max 2 directional lights");
        assert_eq!(point, 3, "Should use all 3 point lights (under max)");
        assert_eq!(spot, 0, "Should have 0 spot lights");
    }

    #[test]
    fn test_get_pipeline_for_material() {
        let lane = LitForwardLane::new();

        // No GPU init → pipeline not yet created → fallback to RenderPipelineId(0)
        let pipeline = lane.get_pipeline_for_material(None);
        assert_eq!(pipeline, RenderPipelineId(0));

        // Same for repeated calls
        let pipeline = lane.get_pipeline_for_material(None);
        assert_eq!(pipeline, RenderPipelineId(0));
    }
}
