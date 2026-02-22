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

use super::RenderWorld;
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
            scene::GpuMesh,
        },
        traits::CommandEncoder,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

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
    pipeline: std::sync::Mutex<Option<RenderPipelineId>>,
    camera_layout: std::sync::Mutex<Option<khora_core::renderer::api::command::BindGroupLayoutId>>,
    model_layout: std::sync::Mutex<Option<khora_core::renderer::api::command::BindGroupLayoutId>>,
    camera_ring:
        std::sync::Mutex<Option<khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer>>,
    model_ring: std::sync::Mutex<
        Option<khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer>,
    >,
    material_layout: std::sync::Mutex<Option<khora_core::renderer::api::command::BindGroupLayoutId>>,
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
            pipeline: std::sync::Mutex::new(None),
            camera_layout: std::sync::Mutex::new(None),
            model_layout: std::sync::Mutex::new(None),
            camera_ring: std::sync::Mutex::new(None),
            model_ring: std::sync::Mutex::new(None),
            material_layout: std::sync::Mutex::new(None),
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
        let render_world = match ctx.get::<khora_core::lane::Slot<crate::render_lane::RenderWorld>>() {
            Some(slot) => slot.get_ref(),
            None => return 1.0,
        };
        let gpu_meshes = match ctx.get::<std::sync::Arc<std::sync::RwLock<khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>>>>() {
            Some(arc) => arc,
            None => return 1.0,
        };
        self.estimate_render_cost(render_world, gpu_meshes)
    }

    fn on_initialize(&self, ctx: &mut khora_core::lane::LaneContext) -> Result<(), khora_core::lane::LaneError> {
        let device = ctx.get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(khora_core::lane::LaneError::missing("Arc<dyn GraphicsDevice>"))?;
        self.on_gpu_init(device.as_ref()).map_err(|e| {
            khora_core::lane::LaneError::InitializationFailed(Box::new(e))
        })
    }

    fn execute(&self, ctx: &mut khora_core::lane::LaneContext) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Slot};
        let device = ctx.get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?.clone();
        let gpu_meshes = ctx.get::<std::sync::Arc<std::sync::RwLock<khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>>>>()
            .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?.clone();
        let encoder = ctx.get::<Slot<dyn khora_core::renderer::traits::CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?.get();
        let render_world = ctx.get::<Slot<crate::render_lane::RenderWorld>>()
            .ok_or(LaneError::missing("Slot<RenderWorld>"))?.get_ref();
        let color_target = ctx.get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?.0;
        let depth_target = ctx.get::<khora_core::lane::DepthTarget>()
            .ok_or(LaneError::missing("DepthTarget"))?.0;
        let clear_color = ctx.get::<khora_core::lane::ClearColor>()
            .ok_or(LaneError::missing("ClearColor"))?.0;
        let shadow_atlas = ctx.get::<khora_core::lane::ShadowAtlasView>().map(|v| v.0);
        let shadow_sampler = ctx.get::<khora_core::lane::ShadowComparisonSampler>().map(|v| v.0);

        let mut render_ctx = khora_core::renderer::api::core::RenderContext::new(
            &color_target, Some(&depth_target), clear_color,
        );
        render_ctx.shadow_atlas = shadow_atlas.as_ref();
        render_ctx.shadow_sampler = shadow_sampler.as_ref();

        self.render(render_world, device.as_ref(), encoder, &render_ctx, &gpu_meshes);
        Ok(())
    }

    fn on_shutdown(&self, ctx: &mut khora_core::lane::LaneContext) {
        if let Some(device) = ctx.get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>() {
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
        // Return the stored pipeline, or 0 if not initialized.
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
        use khora_core::renderer::api::{resource::CameraUniformData, scene::ModelUniforms};

        // 1. Get Active Camera View
        let view = if let Some(first_view) = render_world.views.first() {
            first_view
        } else {
            return; // No camera, nothing to render
        };

        // 2. Prepare Camera Uniforms via Persistent Ring Buffer
        let camera_uniforms = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = {
            let mut lock = self.camera_ring.lock().unwrap();
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("SimpleUnlitLane: camera ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group()
        };

        // Lock the model ring buffer and advance it for this frame
        let mut model_ring_lock = self.model_ring.lock().unwrap();
        let model_ring = match model_ring_lock.as_mut() {
            Some(mr) => {
                mr.advance();
                mr
            }
            None => return,
        };

        let mut material_ring_lock = self.material_ring.lock().unwrap();
        let material_ring = match material_ring_lock.as_mut() {
            Some(mr) => {
                mr.advance();
                mr
            }
            None => return,
        };

        // Acquire read locks on the caches
        let gpu_mesh_assets = gpu_meshes.read().unwrap();

        // 3. Prepare Draw Commands
        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // Get the pre-computed pipeline for this mesh
                let pipeline = self.get_pipeline_for_material(extracted_mesh.material.as_ref());

                // Create Per-Mesh Uniforms
                let model_mat = extracted_mesh.transform.to_matrix();

                let normal_mat = if let Some(inverse) = model_mat.inverse() {
                    inverse.transpose()
                } else {
                    continue; // Skip if degenerate transform
                };

                let mut base_color = khora_core::math::LinearRgba::WHITE;
                if let Some(mat_handle) = &extracted_mesh.material {
                    base_color = mat_handle.base_color();
                }

                let model_uniforms = ModelUniforms {
                    model_matrix: model_mat.to_cols_array_2d(),
                    normal_matrix: normal_mat.to_cols_array_2d(),
                };

                let offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                    Ok(off) => off,
                    Err(_) => continue,
                };
                let model_bg = *model_ring.current_bind_group();

                // Build MaterialUniforms
                let material_uniforms = khora_core::renderer::api::scene::MaterialUniforms {
                    base_color,
                    emissive: khora_core::math::LinearRgba::BLACK,
                    ambient: khora_core::math::LinearRgba::BLACK,
                };

                let mat_offset =
                    match material_ring.push(device, bytemuck::bytes_of(&material_uniforms)) {
                        Ok(off) => off,
                        Err(_) => continue,
                    };
                let material_bg = *material_ring.current_bind_group();

                draw_commands.push(khora_core::renderer::api::command::DrawCommand {
                    pipeline,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_format: gpu_mesh_handle.index_format,
                    index_count: gpu_mesh_handle.index_count,
                    model_bind_group: Some(model_bg),
                    model_offset: offset,
                    material_bind_group: Some(material_bg),
                    material_offset: mat_offset,
                });
            }
        }

        // Configure the render pass to render into the provided color target
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
            label: Some("Simple Unlit Pass"),
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

        // Begin the render pass
        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Bind global camera
        render_pass.set_bind_group(0, &camera_bind_group, &[]);

        // Track the last pipeline we bound to avoid redundant state changes
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
                render_pass.set_bind_group(2, bg, &[cmd.material_offset]);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
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

    fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::shaders::UNLIT_WGSL;
        use khora_core::renderer::api::{
            command::{
                BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
            },
            core::{ShaderModuleDescriptor, ShaderSourceData},
            resource::CameraUniformData,
            pipeline::enums::{CompareFunction, VertexFormat, VertexStepMode},
            pipeline::state::{ColorWrites, DepthBiasState, StencilFaceState},
            pipeline::{
                ColorTargetStateDescriptor, DepthStencilStateDescriptor,
                MultisampleStateDescriptor, PrimitiveStateDescriptor, RenderPipelineDescriptor,
                VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
            },
            scene::ModelUniforms,
            util::uniform_ring_buffer::UniformRingBuffer,
            util::{SampleCount, ShaderStageFlags},
        };
        use std::borrow::Cow;

        log::info!("SimpleUnlitLane: Initializing GPU resources...");

        // 1. Create Bind Group Layouts

        let camera_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("simple_unlit_camera_layout"),
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

        let model_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("simple_unlit_model_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<ModelUniforms>() as u64,
                        ),
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let material_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("simple_unlit_material_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::FRAGMENT, // Material uniforms primarily in FS
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<
                            khora_core::renderer::api::scene::MaterialUniforms,
                        >()
                            as u64),
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.camera_layout.lock().unwrap() = Some(camera_layout);
        *self.model_layout.lock().unwrap() = Some(model_layout);
        *self.material_layout.lock().unwrap() = Some(material_layout);

        // 2. Create Shader Module
        let shader_src = UNLIT_WGSL.to_string();
        let shader_module = device
            .create_shader_module(&ShaderModuleDescriptor {
                label: Some("simple_unlit_shader"),
                source: ShaderSourceData::Wgsl(Cow::Owned(shader_src)),
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // 3. Define Vertex Layout (matching our standard vertex buffer)
        // Attribute 0: Position (vec3<f32>)
        // Attribute 1: Normal (vec3<f32>)
        // Attribute 2: UV (vec2<f32>)
        let vertex_attributes = vec![
            VertexAttributeDescriptor {
                format: VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            VertexAttributeDescriptor {
                format: VertexFormat::Float32x3,
                offset: 12, // 3 * size_of<f32>
                shader_location: 1,
            },
            VertexAttributeDescriptor {
                format: VertexFormat::Float32x2,
                offset: 24, // 6 * size_of<f32>
                shader_location: 2,
            },
        ];

        let vertex_layout = VertexBufferLayoutDescriptor {
            array_stride: 32, // 3*4 + 3*4 + 2*4
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vertex_attributes),
        };

        // 4. Create Pipeline Layout
        let pipeline_layout_ids = vec![camera_layout, model_layout, material_layout];
        let pipeline_layout_desc = khora_core::renderer::api::pipeline::PipelineLayoutDescriptor {
            label: Some(Cow::Borrowed("SimpleUnlit Pipeline Layout")),
            bind_group_layouts: &pipeline_layout_ids,
        };

        let pipeline_layout_id = device
            .create_pipeline_layout(&pipeline_layout_desc)
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // 5. Create Render Pipeline
        let pipeline_desc = RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("SimpleUnlit Pipeline")),
            vertex_shader_module: shader_module,
            vertex_entry_point: Cow::Borrowed("vs_main"),
            fragment_shader_module: Some(shader_module),
            fragment_entry_point: Some(Cow::Borrowed("fs_main")),
            vertex_buffers_layout: Cow::Owned(vec![vertex_layout]),
            layout: Some(pipeline_layout_id),
            primitive_state: PrimitiveStateDescriptor {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil_state: Some(DepthStencilStateDescriptor {
                format: khora_core::renderer::api::util::TextureFormat::Depth32Float,
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
                    .unwrap_or(khora_core::renderer::api::util::TextureFormat::Rgba8UnormSrgb),
                blend: None, // REPLACE
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

        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "Camera Uniform Ring Runlit",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.camera_ring.lock().unwrap() = Some(camera_ring);

        let model_ring =
            khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer::new(
                device,
                model_layout,
                0,
                std::mem::size_of::<ModelUniforms>() as u32,
                khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
                khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
                "Model Dynamic Ring Runlit",
            )
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.model_ring.lock().unwrap() = Some(model_ring);

        let material_ring =
            khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer::new(
                device,
                material_layout,
                0, // Binding size
                std::mem::size_of::<khora_core::renderer::api::scene::MaterialUniforms>() as u32,
                khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
                khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
                "Material Dynamic Ring Runlit",
            )
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *self.material_ring.lock().unwrap() = Some(material_ring);

        Ok(())
    }

    fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        if let Some(ring) = self.camera_ring.lock().unwrap().take() {
            ring.destroy(device);
        }
        if let Some(ring) = self.model_ring.lock().unwrap().take() {
            ring.destroy(device);
        }
        if let Some(ring) = self.material_ring.lock().unwrap().take() {
            ring.destroy(device);
        }
        let mut pipeline_lock = self.pipeline.lock().unwrap();
        if let Some(id) = pipeline_lock.take() {
            let _ = device.destroy_render_pipeline(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::Lane;
    use khora_core::{
        asset::AssetHandle,
        renderer::api::{
            pipeline::enums::PrimitiveTopology, resource::BufferId, util::IndexFormat,
        },
    };
    use std::sync::Arc;

    #[test]
    fn test_simple_unlit_lane_creation() {
        let lane = SimpleUnlitLane::new();
        assert_eq!(lane.strategy_name(), "SimpleUnlit");
    }

    #[test]
    fn test_default_construction() {
        let lane = SimpleUnlitLane::new();
        assert_eq!(lane.strategy_name(), "SimpleUnlit");
    }

    #[test]
    fn test_cost_estimation_empty_world() {
        let lane = SimpleUnlitLane::new();
        let render_world = RenderWorld::default();
        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);
        assert_eq!(cost, 0.0, "Empty world should have zero cost");
    }

    #[test]
    fn test_cost_estimation_triangle_list() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();

        // Create a GPU mesh with 300 indices (100 triangles) using TriangleList
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 300,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };
        let gpu_mesh_handle = AssetHandle::new(gpu_mesh);
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, gpu_mesh_handle.clone());

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: gpu_mesh_handle,
            material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Expected: 100 triangles * 0.001 + 1 draw call * 0.1 = 0.1 + 0.1 = 0.2
        assert_eq!(
            cost, 0.2,
            "Cost should be 0.2 for 100 triangles + 1 draw call"
        );
    }

    #[test]
    fn test_cost_estimation_triangle_strip() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();

        // Create a GPU mesh with 52 indices (50 triangles) using TriangleStrip
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 52,
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };
        let gpu_mesh_handle = AssetHandle::new(gpu_mesh);
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, gpu_mesh_handle.clone());

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: gpu_mesh_handle,
            material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Expected: 50 triangles * 0.001 + 1 draw call * 0.1 = 0.05 + 0.1 = 0.15
        assert_eq!(
            cost, 0.15,
            "Cost should be 0.15 for 50 triangles + 1 draw call"
        );
    }

    #[test]
    fn test_cost_estimation_lines_and_points() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();

        // Create meshes with non-triangle topologies
        let line_uuid = AssetUUID::new();
        let point_uuid = AssetUUID::new();

        let line_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 100,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::LineList,
        };

        let point_mesh = GpuMesh {
            vertex_buffer: BufferId(2),
            index_buffer: BufferId(3),
            index_count: 50,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::PointList,
        };
        let line_mesh_handle = AssetHandle::new(line_mesh);
        let point_mesh_handle = AssetHandle::new(point_mesh);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(line_uuid, line_mesh_handle.clone());
        gpu_meshes.insert(point_uuid, point_mesh_handle.clone());

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: line_uuid,
            gpu_mesh: line_mesh_handle,
            material: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: point_uuid,
            gpu_mesh: point_mesh_handle,
            material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Expected: 0 triangles * 0.001 + 2 draw calls * 0.1 = 0.0 + 0.2 = 0.2
        assert_eq!(
            cost, 0.2,
            "Cost should be 0.2 for 2 draw calls with no triangles"
        );
    }

    #[test]
    fn test_cost_estimation_multiple_meshes() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();

        // Create 3 different meshes
        let mesh1_uuid = AssetUUID::new();
        let mesh2_uuid = AssetUUID::new();
        let mesh3_uuid = AssetUUID::new();

        let mesh1 = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 600, // 200 triangles
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };

        let mesh2 = GpuMesh {
            vertex_buffer: BufferId(2),
            index_buffer: BufferId(3),
            index_count: 102, // 100 triangles (strip)
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };

        let mesh3 = GpuMesh {
            vertex_buffer: BufferId(4),
            index_buffer: BufferId(5),
            index_count: 150, // 50 triangles
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh1_uuid, AssetHandle::new(mesh1));
        gpu_meshes.insert(mesh2_uuid, AssetHandle::new(mesh2));
        gpu_meshes.insert(mesh3_uuid, AssetHandle::new(mesh3));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh1_uuid,
            gpu_mesh: AssetHandle::new(create_test_mesh(600)),
            material: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh2_uuid,
            gpu_mesh: AssetHandle::new(create_test_mesh(102)),
            material: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh3_uuid,
            gpu_mesh: AssetHandle::new(create_test_mesh(150)),
            material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Expected: (200 + 100 + 50) triangles * 0.001 + 3 draw calls * 0.1
        //         = 350 * 0.001 + 3 * 0.1 = 0.35 + 0.3 = 0.65
        assert!(
            (cost - 0.65).abs() < 0.0001,
            "Cost should be approximately 0.65 for 350 triangles + 3 draw calls, got {}",
            cost
        );
    }

    // Helper to create test mesh
    fn create_test_mesh(index_count: u32) -> GpuMesh {
        GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }
    }

    #[test]
    fn test_cost_estimation_missing_mesh() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();
        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

        // Reference a mesh that doesn't exist in the cache
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: AssetUUID::new(),
            gpu_mesh: AssetHandle::new(create_test_mesh(300)),
            material: None,
        });

        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);

        // Expected: 0 cost since mesh is not found
        assert_eq!(cost, 0.0, "Missing mesh should contribute zero cost");
    }

    #[test]
    fn test_cost_estimation_degenerate_triangle_strip() {
        use crate::render_lane::world::ExtractedMesh;
        use khora_core::asset::AssetUUID;

        let lane = SimpleUnlitLane::new();

        // Create a triangle strip with only 2 indices (not enough for a triangle)
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 2,
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };

        let handle = AssetHandle::new(gpu_mesh);
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, handle.clone());

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            cpu_mesh_uuid: mesh_uuid,
            gpu_mesh: handle,
            material: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

        // Expected: 0 triangles + 1 draw call * 0.1 = 0.1
        assert_eq!(
            cost, 0.1,
            "Degenerate triangle strip should only cost draw call overhead"
        );
    }

    #[test]
    fn test_get_pipeline_for_material_with_none() {
        let lane = SimpleUnlitLane::new();

        let pipeline = lane.get_pipeline_for_material(None);
        assert_eq!(
            pipeline,
            RenderPipelineId(0),
            "None material should use default pipeline"
        );
    }

    #[test]
    fn test_get_pipeline_for_material_not_found() {
        let lane = SimpleUnlitLane::new();

        // Since there is no registry anymore, we just test with None or a dummy handle.
        // The old test "missing material" is now redundant with "None material".
        let pipeline = lane.get_pipeline_for_material(None);
        assert_eq!(
            pipeline,
            RenderPipelineId(0),
            "Missing material should use default pipeline"
        );
    }
}
