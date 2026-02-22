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

#[allow(unused_imports)]
use khora_core::math::{Extent2D, Extent3D, LinearRgba, Mat4, Origin3D};
#[allow(unused_imports)]
use khora_core::renderer::api::command::BindGroupLayoutId;

use super::RenderWorld;
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
                DirectionalLightUniform, GpuMesh, LightingUniforms, MaterialUniforms,
                ModelUniforms, PointLightUniform, SpotLightUniform, MAX_DIRECTIONAL_LIGHTS,
                MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
            },
        },
        traits::CommandEncoder,
    },
};
use khora_data::assets::Assets;
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
#[derive(Debug)]
pub struct LitForwardLane {
    /// The shader complexity level to use for cost estimation.
    pub shader_complexity: ShaderComplexity,
    /// Maximum number of directional lights supported per pass.
    pub max_directional_lights: u32,
    /// Maximum number of point lights supported per pass.
    pub max_point_lights: u32,
    /// Maximum number of spot lights supported per pass.
    pub max_spot_lights: u32,
    /// The stored render pipeline handle.
    pipeline: std::sync::Mutex<Option<RenderPipelineId>>,
    /// Layout for Camera (Group 0)
    camera_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
    /// Layout for Model (Group 1)
    model_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
    /// Layout for Material (Group 2)
    material_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
    /// Layout for Lighting (Group 3) — full layout with shadow atlas + sampler for pipeline.
    light_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
    /// Layout for the lighting uniform buffer only (1 binding) — used by the ring buffer.
    lighting_buffer_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
    /// Persistent ring buffer for camera uniforms (eliminates per-frame allocation).
    camera_ring: std::sync::Mutex<Option<UniformRingBuffer>>,
    /// Persistent ring buffer for lighting uniforms (eliminates per-frame allocation).
    lighting_ring: std::sync::Mutex<Option<UniformRingBuffer>>,
}

impl Default for LitForwardLane {
    fn default() -> Self {
        Self {
            shader_complexity: ShaderComplexity::SimpleLit,
            max_directional_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 8,
            pipeline: std::sync::Mutex::new(None),
            camera_layout: std::sync::Mutex::new(None),
            model_layout: std::sync::Mutex::new(None),
            material_layout: std::sync::Mutex::new(None),
            light_layout: std::sync::Mutex::new(None),
            lighting_buffer_layout: std::sync::Mutex::new(None),
            camera_ring: std::sync::Mutex::new(None),
            lighting_ring: std::sync::Mutex::new(None),
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
        let render_world =
            match ctx.get::<khora_core::lane::Slot<crate::render_lane::RenderWorld>>() {
                Some(slot) => slot.get_ref(),
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
            ))?;
        self.on_gpu_init(device.as_ref())
            .map_err(|e| khora_core::lane::LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Slot};
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
            .get::<Slot<crate::render_lane::RenderWorld>>()
            .ok_or(LaneError::missing("Slot<RenderWorld>"))?
            .get_ref();
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

impl LitForwardLane {
    /// Returns the render pipeline for the given material (or default).
    pub fn get_pipeline_for_material(
        &self,
        _material: Option<&khora_core::asset::AssetHandle<Box<dyn Material>>>,
    ) -> RenderPipelineId {
        // Return the stored pipeline. Fallback to pipeline 0 if on_gpu_init hasn't run yet.
        self.pipeline.lock().unwrap().unwrap_or(RenderPipelineId(0))
    }

    fn render(
        &self,
        render_world: &RenderWorld,
        device: &dyn khora_core::renderer::GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) {
        use khora_core::renderer::api::{
            command::{BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding},
            resource::{BufferDescriptor, BufferUsage},
        };

        // 1. Get Active Camera View
        let view = if let Some(first_view) = render_world.views.first() {
            first_view
        } else {
            return; // No camera, nothing to render
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
            let mut lock = self.camera_ring.lock().unwrap();
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

        for light in &render_world.lights {
            match light.light_type {
                khora_core::renderer::light::LightType::Directional(ref d) => {
                    if (lighting_uniforms.num_directional_lights as usize) < MAX_DIRECTIONAL_LIGHTS
                    {
                        let idx = lighting_uniforms.num_directional_lights as usize;
                        let shadow_index = light.shadow_atlas_index.unwrap_or(-1) as f32;
                        lighting_uniforms.directional_lights[idx] = DirectionalLightUniform {
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                0.0,
                            ],
                            color: d.color.with_alpha(d.intensity),
                            shadow_view_proj: light.shadow_view_proj.to_cols_array_2d(),
                            shadow_params: [shadow_index, d.shadow_bias, d.shadow_normal_bias, 0.0],
                        };
                        lighting_uniforms.num_directional_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Point(ref p) => {
                    if (lighting_uniforms.num_point_lights as usize) < MAX_POINT_LIGHTS {
                        let idx = lighting_uniforms.num_point_lights as usize;
                        let shadow_index = light.shadow_atlas_index.unwrap_or(-1) as f32;
                        lighting_uniforms.point_lights[idx] = PointLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                p.range,
                            ],
                            color: p.color.with_alpha(p.intensity),
                            shadow_params: [shadow_index, p.shadow_bias, p.shadow_normal_bias, 0.0],
                        };
                        lighting_uniforms.num_point_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Spot(ref s) => {
                    if (lighting_uniforms.num_spot_lights as usize) < MAX_SPOT_LIGHTS {
                        let idx = lighting_uniforms.num_spot_lights as usize;
                        let shadow_index = light.shadow_atlas_index.unwrap_or(-1) as f32;
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
                            shadow_view_proj: light.shadow_view_proj.to_cols_array_2d(),
                            shadow_params: [shadow_index, s.shadow_bias, s.shadow_normal_bias, 0.0],
                        };
                        lighting_uniforms.num_spot_lights += 1;
                    }
                }
            }
        }

        let (_lighting_bind_group, lighting_ring_buffer_id) = {
            let mut lock = self.lighting_ring.lock().unwrap();
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
        let gpu_mesh_assets = gpu_meshes.read().unwrap();

        // Pipeline binding logic moved before render pass to avoid issues
        let pipeline_id = self.pipeline.lock().unwrap().unwrap_or(RenderPipelineId(0));

        // Prepare Draw Commands
        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());

        let mut temp_buffers = Vec::new();
        let mut temp_bind_groups = Vec::new();

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // Create Per-Mesh Uniforms
                let model_mat = extracted_mesh.transform.to_matrix();

                // Strict check: if the matrix is not invertible, skip
                let normal_mat = if let Some(inverse) = model_mat.inverse() {
                    inverse.transpose()
                } else {
                    continue;
                };

                let mut base_color = khora_core::math::LinearRgba::WHITE;
                let mut emissive = khora_core::math::LinearRgba::BLACK;
                let mut specular_power = 32.0;

                if let Some(mat_handle) = &extracted_mesh.material {
                    base_color = mat_handle.base_color();
                    emissive = mat_handle.emissive_color();
                    specular_power = mat_handle.specular_power();
                }

                let model_uniforms = ModelUniforms {
                    model_matrix: model_mat.to_cols_array_2d(),
                    normal_matrix: normal_mat.to_cols_array_2d(),
                };

                let model_buffer = match device.create_buffer_with_data(
                    &BufferDescriptor {
                        label: None,
                        size: std::mem::size_of::<ModelUniforms>() as u64,
                        usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                        mapped_at_creation: false,
                    },
                    bytemuck::bytes_of(&model_uniforms),
                ) {
                    Ok(b) => {
                        temp_buffers.push(b);
                        b
                    }
                    Err(_) => continue,
                };

                let material_uniforms = MaterialUniforms {
                    base_color,
                    emissive: emissive.with_alpha(specular_power),
                    ambient: khora_core::math::LinearRgba::new(0.1, 0.1, 0.1, 0.0),
                };
                let material_buffer = match device.create_buffer_with_data(
                    &BufferDescriptor {
                        label: None,
                        size: std::mem::size_of::<MaterialUniforms>() as u64,
                        usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                        mapped_at_creation: false,
                    },
                    bytemuck::bytes_of(&material_uniforms),
                ) {
                    Ok(b) => {
                        temp_buffers.push(b);
                        b
                    }
                    Err(_) => continue,
                };

                // Create Bind Groups 1 & 2
                let mut model_bg = None;
                if let Some(layout) = *self.model_layout.lock().unwrap() {
                    if let Ok(bg) = device.create_bind_group(&BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &[BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: model_buffer,
                                offset: 0,
                                size: None,
                            }),
                            _phantom: std::marker::PhantomData,
                        }],
                    }) {
                        model_bg = Some(bg);
                        temp_bind_groups.push(bg);
                    }
                }

                let mut material_bg = None;
                if let Some(layout) = *self.material_layout.lock().unwrap() {
                    if let Ok(bg) = device.create_bind_group(&BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &[BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: material_buffer,
                                offset: 0,
                                size: None,
                            }),
                            _phantom: std::marker::PhantomData,
                        }],
                    }) {
                        material_bg = Some(bg);
                        temp_bind_groups.push(bg);
                    }
                }

                draw_commands.push(khora_core::renderer::api::command::DrawCommand {
                    pipeline: pipeline_id,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_format: gpu_mesh_handle.index_format,
                    index_count: gpu_mesh_handle.index_count,
                    model_bind_group: model_bg,
                    model_offset: 0,
                    material_bind_group: material_bg,
                    material_offset: 0,
                });
            }
        }

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

        // Build Lighting Bind Group with Shadow Atlas
        // The pipeline's group 3 layout expects 3 bindings: uniform buffer + shadow atlas + shadow sampler.
        // All 3 must be present for the bind group to be valid.
        let final_lighting_bind_group = if let Some(layout) = *self.light_layout.lock().unwrap() {
            match (render_ctx.shadow_atlas, render_ctx.shadow_sampler) {
                (Some(atlas), Some(sampler)) => {
                    let entries = [
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: lighting_ring_buffer_id,
                                offset: 0,
                                size: None,
                            }),
                            _phantom: std::marker::PhantomData,
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(*atlas),
                            _phantom: std::marker::PhantomData,
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: BindingResource::Sampler(*sampler),
                            _phantom: std::marker::PhantomData,
                        },
                    ];

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
                }
                _ => {
                    log::warn!(
                        "LitForwardLane: shadow atlas/sampler not available, skipping lit render"
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
            if current_pipeline != Some(pipeline_id) {
                render_pass.set_pipeline(&pipeline_id);
                current_pipeline = Some(pipeline_id);
            }

            if let Some(bg) = &cmd.model_bind_group {
                render_pass.set_bind_group(1, bg, &[]);
            }

            if let Some(bg) = &cmd.material_bind_group {
                render_pass.set_bind_group(2, bg, &[]);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);

            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
        }

        // Clean up temporary resources (they remain alive on the GPU until the command buffer finishes)
        for bg in temp_bind_groups {
            let _ = device.destroy_bind_group(bg);
        }
        for buf in temp_buffers {
            let _ = device.destroy_buffer(buf);
        }
    }

    fn estimate_render_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        let gpu_mesh_assets = gpu_meshes.read().unwrap();

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
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::shaders::LIT_FORWARD_WGSL;
        use khora_core::renderer::api::{
            command::{
                BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
            },
            core::{ShaderModuleDescriptor, ShaderSourceData},
            pipeline::enums::{CompareFunction, VertexFormat, VertexStepMode},
            pipeline::state::{ColorWrites, DepthBiasState, StencilFaceState},
            pipeline::{
                ColorTargetStateDescriptor, DepthStencilStateDescriptor,
                MultisampleStateDescriptor, PrimitiveStateDescriptor, RenderPipelineDescriptor,
                VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
            },
            util::{SampleCount, ShaderStageFlags, TextureFormat},
        };
        use std::borrow::Cow;

        log::info!("LitForwardLane: Initializing GPU resources...");

        // 1. Create Bind Group Layouts

        // Group 0: Camera
        let camera_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_camera_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Group 1: Model
        let model_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_model_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false, // Using simple uniform for now, could be dynamic later
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Group 2: Material
        let material_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_material_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Group 3: Lights
        use khora_core::renderer::api::command::{SamplerBindingType, TextureSampleType};
        let light_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_light_layout"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStageFlags::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStageFlags::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Depth,
                            view_dimension:
                                khora_core::renderer::api::command::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStageFlags::FRAGMENT,
                        ty: BindingType::Sampler(SamplerBindingType::Comparison),
                    },
                ],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // 2. Create Shader Module
        let shader_module = device
            .create_shader_module(&ShaderModuleDescriptor {
                label: Some("lit_forward_shader"),
                source: ShaderSourceData::Wgsl(Cow::Borrowed(LIT_FORWARD_WGSL)),
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // 3. Create Pipeline
        let vertex_attributes = vec![
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
        ];

        let vertex_layout = VertexBufferLayoutDescriptor {
            array_stride: 32, // 3+3+2 floats * 4 bytes
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vertex_attributes),
        };

        let pipeline_layout_ids = vec![camera_layout, model_layout, material_layout, light_layout];
        // Store bind group layouts for creating bind groups during rendering.
        *self.camera_layout.lock().unwrap() = Some(camera_layout);
        *self.model_layout.lock().unwrap() = Some(model_layout);
        *self.material_layout.lock().unwrap() = Some(material_layout);
        *self.light_layout.lock().unwrap() = Some(light_layout);

        // Create the pipeline layout from our bind group layouts.
        let pipeline_layout_desc = khora_core::renderer::api::pipeline::PipelineLayoutDescriptor {
            label: Some(Cow::Borrowed("LitForward Pipeline Layout")),
            bind_group_layouts: &pipeline_layout_ids,
        };

        let pipeline_layout_id = device
            .create_pipeline_layout(&pipeline_layout_desc)
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let pipeline_desc = RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("LitForward Pipeline")),
            layout: Some(pipeline_layout_id),
            vertex_shader_module: shader_module,
            vertex_entry_point: Cow::Borrowed("vs_main"),
            fragment_shader_module: Some(shader_module),
            fragment_entry_point: Some(Cow::Borrowed("fs_main")),
            vertex_buffers_layout: Cow::Owned(vec![vertex_layout]),
            primitive_state: PrimitiveStateDescriptor {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil_state: Some(DepthStencilStateDescriptor {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil_front: StencilFaceState::default(),
                stencil_back: StencilFaceState::default(),
                stencil_read_mask: 0,
                stencil_write_mask: 0,
                bias: DepthBiasState::default(),
            }),
            color_target_states: Cow::Owned(vec![ColorTargetStateDescriptor {
                format: device
                    .get_surface_format()
                    .unwrap_or(TextureFormat::Rgba8UnormSrgb),
                blend: None,
                write_mask: ColorWrites::ALL,
            }]),
            multisample_state: MultisampleStateDescriptor {
                count: SampleCount::X1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        };

        let pipeline_id = device
            .create_render_pipeline(&pipeline_desc)
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let mut pipeline_lock = self.pipeline.lock().unwrap();
        *pipeline_lock = Some(pipeline_id);

        // 4. Create Persistent Ring Buffers for camera and lighting uniforms.
        // This eliminates per-frame buffer allocation in the render hot path.

        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>() as u64,
            "Camera Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // The lighting ring buffer needs its own 1-binding layout (just the uniform buffer).
        // The full 3-binding light_layout (uniform + shadow atlas + shadow sampler) is used
        // for the pipeline and per-frame bind group creation in render().
        let lighting_buffer_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_lighting_buffer_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.lighting_buffer_layout.lock().unwrap() = Some(lighting_buffer_layout);

        let lighting_ring = UniformRingBuffer::new(
            device,
            lighting_buffer_layout,
            0,
            std::mem::size_of::<LightingUniforms>() as u64,
            "Lighting Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.camera_ring.lock().unwrap() = Some(camera_ring);
        *self.lighting_ring.lock().unwrap() = Some(lighting_ring);

        log::info!(
            "LitForwardLane: Persistent ring buffers created (camera: {} bytes, lighting: {} bytes, {} slots each)",
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>(),
            std::mem::size_of::<LightingUniforms>(),
            khora_core::renderer::api::core::MAX_FRAMES_IN_FLIGHT,
        );

        Ok(())
    }

    fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        // Destroy ring buffers first (they own buffers + bind groups)
        if let Some(ring) = self.camera_ring.lock().unwrap().take() {
            ring.destroy(device);
        }
        if let Some(ring) = self.lighting_ring.lock().unwrap().take() {
            ring.destroy(device);
        }

        let mut pipeline_lock = self.pipeline.lock().unwrap();
        if let Some(id) = pipeline_lock.take() {
            let _ = device.destroy_render_pipeline(id);
        }
        if let Some(id) = self.camera_layout.lock().unwrap().take() {
            let _ = device.destroy_bind_group_layout(id);
        }
        if let Some(id) = self.model_layout.lock().unwrap().take() {
            let _ = device.destroy_bind_group_layout(id);
        }
        if let Some(id) = self.material_layout.lock().unwrap().take() {
            let _ = device.destroy_bind_group_layout(id);
        }
        if let Some(id) = self.light_layout.lock().unwrap().take() {
            let _ = device.destroy_bind_group_layout(id);
        }
        if let Some(id) = self.lighting_buffer_layout.lock().unwrap().take() {
            let _ = device.destroy_bind_group_layout(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_lane::world::ExtractedMesh;
    use khora_core::lane::Lane;
    use khora_core::{
        asset::{AssetHandle, AssetUUID},
        math::{affine_transform::AffineTransform, Mat4},
        renderer::{
            api::{pipeline::enums::PrimitiveTopology, resource::BufferId, util::IndexFormat},
            light::DirectionalLight,
        },
    };
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
        use crate::render_lane::world::ExtractedLight;
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
        use crate::render_lane::world::ExtractedLight;
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
