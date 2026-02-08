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

use crate::render_lane::RenderLane;
use khora_core::renderer::api::BindGroupLayoutId;

use super::RenderWorld;
use khora_core::{
    asset::{AssetUUID, Material},
    renderer::{
        api::{
            command::{
                LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
                RenderPassDescriptor, StoreOp,
            },
            DirectionalLightUniform, LightingUniforms, MaterialUniforms, ModelUniforms,
            PointLightUniform, PrimitiveTopology, SpotLightUniform, MAX_DIRECTIONAL_LIGHTS,
            MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
        },
        traits::CommandEncoder,
        GpuMesh, RenderContext, RenderPipelineId,
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
    /// Layout for Lighting (Group 3)
    light_layout: std::sync::Mutex<Option<BindGroupLayoutId>>,
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

impl RenderLane for LitForwardLane {
    fn strategy_name(&self) -> &'static str {
        "LitForward"
    }

    fn get_pipeline_for_material(
        &self,
        _material_uuid: Option<AssetUUID>,
        _materials: &Assets<Box<dyn Material>>,
    ) -> RenderPipelineId {
        // Return the stored pipeline, or fallback to 1 (placeholder)
        self.pipeline.lock().unwrap().unwrap_or(RenderPipelineId(1))
    }

    fn render(
        &self,
        render_world: &RenderWorld,
        device: &dyn khora_core::renderer::GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
        _materials: &RwLock<Assets<Box<dyn Material>>>,
    ) {
        use khora_core::renderer::api::{
            BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding, BufferDescriptor,
            BufferUsage,
        };

        // 1. Get Active Camera View
        let view = if let Some(first_view) = render_world.views.first() {
            first_view
        } else {
            return; // No camera, nothing to render
        };

        // 2. Prepare Global Uniforms (Camera & Lights)

        // Camera Uniforms
        let camera_uniforms = khora_core::renderer::api::CameraUniformData {
            view_projection: view.view_proj,
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_buffer = match device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(std::borrow::Cow::Borrowed("Camera Uniform Buffer")),
                size: std::mem::size_of::<khora_core::renderer::api::CameraUniformData>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(&camera_uniforms),
        ) {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to create camera buffer: {:?}", e);
                return;
            }
        };

        // Lighting Uniforms
        let mut lighting_uniforms = LightingUniforms {
            directional_lights: [DirectionalLightUniform {
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
            }; MAX_DIRECTIONAL_LIGHTS],
            point_lights: [PointLightUniform {
                position: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
            }; MAX_POINT_LIGHTS],
            spot_lights: [SpotLightUniform {
                position: [0.0; 4],
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                params: [0.0; 4],
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
                        lighting_uniforms.directional_lights[idx] = DirectionalLightUniform {
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                0.0,
                            ],
                            color: d.color.with_alpha(d.intensity),
                        };
                        lighting_uniforms.num_directional_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Point(ref p) => {
                    if (lighting_uniforms.num_point_lights as usize) < MAX_POINT_LIGHTS {
                        let idx = lighting_uniforms.num_point_lights as usize;
                        lighting_uniforms.point_lights[idx] = PointLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                p.range,
                            ],
                            color: p.color.with_alpha(p.intensity),
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
                        };
                        lighting_uniforms.num_spot_lights += 1;
                    }
                }
            }
        }

        let lighting_buffer = match device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(std::borrow::Cow::Borrowed("Lighting Uniform Buffer")),
                size: std::mem::size_of::<LightingUniforms>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(&lighting_uniforms),
        ) {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to create lighting buffer: {:?}", e);
                return;
            }
        };

        // Create Bind Groups (Global)
        let camera_layout_lock = self.camera_layout.lock().unwrap();
        let light_layout_lock = self.light_layout.lock().unwrap();

        let camera_bind_group = if let Some(layout_id) = *camera_layout_lock {
            match device.create_bind_group(&BindGroupDescriptor {
                label: Some("Camera Bind Group"),
                layout: layout_id,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: camera_buffer,
                        offset: 0,
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            }) {
                Ok(bg) => Some(bg),
                Err(e) => {
                    log::error!("Failed to create camera bind group: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        let light_bind_group = if let Some(layout_id) = *light_layout_lock {
            match device.create_bind_group(&BindGroupDescriptor {
                label: Some("Light Bind Group"),
                layout: layout_id,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: lighting_buffer,
                        offset: 0,
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            }) {
                Ok(bg) => Some(bg),
                Err(e) => {
                    log::error!("Failed to create light bind group: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        // Acquire locks
        let gpu_mesh_assets = gpu_meshes.read().unwrap();

        // Pipeline binding logic moved before render pass to avoid issues
        let pipeline_id = self.pipeline.lock().unwrap().unwrap_or(RenderPipelineId(0));

        // Prepare Draw Commands
        struct DrawCommand {
            model_bind_group: Option<khora_core::renderer::BindGroupId>,
            material_bind_group: Option<khora_core::renderer::BindGroupId>,
            index_count: u32,
            vertex_buffer: khora_core::renderer::api::buffer::BufferId,
            index_buffer: khora_core::renderer::api::buffer::BufferId,
            index_format: khora_core::renderer::api::IndexFormat,
        }

        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.gpu_mesh_uuid) {
                // Create Per-Mesh Uniforms
                let model_mat = extracted_mesh.transform.to_matrix();

                // Strict check: if the matrix is not invertible, skip
                let normal_mat = if let Some(inverse) = model_mat.inverse() {
                    inverse.transpose()
                } else {
                    continue;
                };

                let m_cols = model_mat.cols;
                let n_cols = normal_mat.cols;

                let model_uniforms = ModelUniforms {
                    model_matrix: [
                        [m_cols[0].x, m_cols[0].y, m_cols[0].z, m_cols[0].w],
                        [m_cols[1].x, m_cols[1].y, m_cols[1].z, m_cols[1].w],
                        [m_cols[2].x, m_cols[2].y, m_cols[2].z, m_cols[2].w],
                        [m_cols[3].x, m_cols[3].y, m_cols[3].z, m_cols[3].w],
                    ],
                    normal_matrix: [
                        [n_cols[0].x, n_cols[0].y, n_cols[0].z, n_cols[0].w],
                        [n_cols[1].x, n_cols[1].y, n_cols[1].z, n_cols[1].w],
                        [n_cols[2].x, n_cols[2].y, n_cols[2].z, n_cols[2].w],
                        [n_cols[3].x, n_cols[3].y, n_cols[3].z, n_cols[3].w],
                    ],
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
                    Ok(b) => b,
                    Err(_) => continue,
                };

                let material_uniforms = MaterialUniforms {
                    base_color: khora_core::math::LinearRgba::WHITE,
                    emissive: khora_core::math::LinearRgba::BLACK.with_alpha(32.0),
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
                    Ok(b) => b,
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
                    }
                }

                draw_commands.push(DrawCommand {
                    model_bind_group: model_bg,
                    material_bind_group: material_bg,
                    index_count: gpu_mesh_handle.index_count,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_format: gpu_mesh_handle.index_format,
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
                }
            }),
        };

        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        if let Some(bg) = &camera_bind_group {
            render_pass.set_bind_group(0, bg);
        }
        if let Some(bg) = &light_bind_group {
            render_pass.set_bind_group(3, bg);
        }

        let mut current_pipeline: Option<RenderPipelineId> = None;

        for cmd in &draw_commands {
            if current_pipeline != Some(pipeline_id) {
                render_pass.set_pipeline(&pipeline_id);
                current_pipeline = Some(pipeline_id);
            }

            if let Some(bg) = &cmd.model_bind_group {
                render_pass.set_bind_group(1, bg);
            }

            if let Some(bg) = &cmd.material_bind_group {
                render_pass.set_bind_group(2, bg);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);

            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
        }
    }

    fn estimate_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        let gpu_mesh_assets = gpu_meshes.read().unwrap();

        let mut total_triangles = 0u32;
        let mut draw_call_count = 0u32;

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh) = gpu_mesh_assets.get(&extracted_mesh.gpu_mesh_uuid) {
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

    fn on_initialize(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::shaders::LIT_FORWARD_WGSL;
        use khora_core::renderer::api::{
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
            ColorTargetStateDescriptor, ColorWrites, CompareFunction, DepthBiasState,
            DepthStencilStateDescriptor, MultisampleStateDescriptor, PrimitiveStateDescriptor,
            RenderPipelineDescriptor, SampleCount, ShaderModuleDescriptor, ShaderSourceData,
            ShaderStageFlags, StencilFaceState, TextureFormat, VertexAttributeDescriptor,
            VertexBufferLayoutDescriptor, VertexFormat, VertexStepMode,
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
        let light_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("lit_forward_light_layout"),
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
            VertexAttributeDescriptor {
                format: VertexFormat::Float32x4,
                offset: 32,
                shader_location: 3,
            }, // Color
        ];

        let vertex_layout = VertexBufferLayoutDescriptor {
            array_stride: 48, // 3+3+2+4 floats * 4 bytes
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vertex_attributes),
        };

        let pipeline_layout_ids = vec![camera_layout, model_layout, material_layout, light_layout];

        // We need a pipeline layout to create the pipeline?
        // GraphicsDevice::create_render_pipeline takes RenderPipelineDescriptor
        // which usually includes layout. But khora-core abstraction might handle it differently.
        // Checking khora-core... RenderPipelineDescriptor usually has `layout`.
        // If not, it uses implicit layout. But we want explicit.
        // Assuming khora-core RenderPipelineDescriptor structure.
        // If khora-core doesn't expose pipeline layout creation in the descriptor, it might generate it from shader...
        // But we want to store bind group layouts.

        // The previous code didn't set layout in descriptor.
        // Let's assume for now we just pass layouts if supported, or rely on implicit.
        // BUT we need the layouts for creating bind groups later!

        // Storing layouts
        *self.camera_layout.lock().unwrap() = Some(camera_layout);
        *self.model_layout.lock().unwrap() = Some(model_layout);
        *self.material_layout.lock().unwrap() = Some(material_layout);
        *self.light_layout.lock().unwrap() = Some(light_layout);

        // Create the pipeline layout
        let pipeline_layout_desc = khora_core::renderer::PipelineLayoutDescriptor {
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

        // Note: RenderPipelineDescriptor in wgpu requires `layout`.
        // If khora-core API doesn't have it, it might use `auto_layout`.
        // Let's check khora-core if possible, or assume auto.
        // For now, ignoring explicit layout in pipeline creation call,
        // but we saved the layouts to create bind groups.
        // Wait, if we use auto layout, the bind groups we create from MANUALLY created layouts might not be compatible!
        // This is a risk.
        // However, I can't see RenderPipelineDescriptor definition right now.
        // In step 2076, lines 445+ show usage of RenderPipelineDescriptor. It does NOT have a `layout` field.
        // So khora-core likely uses `layout: None` (auto) internally.
        // If so, we should query the layout from the pipeline? Or ensure our manually created layouts match.
        // WGPU says: "If the layout is `None`, the layout will be derived from the shaders."
        // We can create bind groups from the pipeline's get_bind_group_layout() if available.
        // But `RenderPipelineId` is an opaque handle.

        // Strategy: Use implicit layout (auto) for the pipeline.
        // But accessing the layouts:
        // Does khora-core allow getting bind group layouts from pipeline ID? Not obviously.
        // If I create layouts manually, I should pass them.
        // If checking `khora-core` is too slow, I'll assume implicit is fine for now,
        // and I'll create bind groups using layouts derived from the pipeline if possible,
        // or just accept that I might have a mismatch if I'm not careful.

        // Actually, strictly matching WGSL bindings usually works with manually created layouts too if they match.

        let pipeline_id = device
            .create_render_pipeline(&pipeline_desc)
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let mut pipeline_lock = self.pipeline.lock().unwrap();
        *pipeline_lock = Some(pipeline_id);

        Ok(())
    }

    fn on_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_lane::world::ExtractedMesh;
    use khora_core::{
        asset::{AssetHandle, AssetUUID},
        math::affine_transform::AffineTransform,
        renderer::{api::PrimitiveTopology, light::DirectionalLight, BufferId, IndexFormat},
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

        let cost = lane.estimate_cost(&render_world, &gpu_meshes);
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
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
        use khora_core::{math::Vec3, renderer::light::LightType};

        let lane = LitForwardLane::new();

        // Create a GPU mesh
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = create_test_gpu_mesh(300);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::default(),
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        // Add 4 directional lights
        for _ in 0..4 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Directional(DirectionalLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::new(0.0, -1.0, 0.0),
            });
        }

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));

        let unlit_lane = LitForwardLane::with_complexity(ShaderComplexity::Unlit);
        let simple_lane = LitForwardLane::with_complexity(ShaderComplexity::SimpleLit);
        let pbr_lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);

        let unlit_cost = unlit_lane.estimate_cost(&render_world, &gpu_meshes_lock);
        let simple_cost = simple_lane.estimate_cost(&render_world, &gpu_meshes_lock);
        let pbr_cost = pbr_lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
            });
        }

        // Add 3 point lights (max is 4)
        for _ in 0..3 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Point(PointLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::ZERO,
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
        let materials = Assets::<Box<dyn Material>>::new();

        // No material should return pipeline ID 1 (lit default)
        let pipeline = lane.get_pipeline_for_material(None, &materials);
        assert_eq!(pipeline, RenderPipelineId(1));

        // Non-existent material should also return default
        let pipeline = lane.get_pipeline_for_material(Some(AssetUUID::new()), &materials);
        assert_eq!(pipeline, RenderPipelineId(1));
    }
}
