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

//! Standard PBR rendering lane — Cook-Torrance BRDF, multi-light, with
//! shadow sampling. Alternative strategy to `LitForwardLane` /
//! `ForwardPlusLane`, registered under
//! [`RenderAgent`](khora_agents::render_agent).
//!
//! The pipeline is composed from `khora::pipelines::standard_pbr`,
//! which shares the same group(3) uniform / shadow layout as
//! `LitForward` — the only difference is the BRDF in the fragment
//! shader. Consequently this lane reuses the lit lane's render path
//! one-for-one, plumbed through a shared free function so the two
//! lanes never drift apart structurally.

use khora_core::math::Mat4;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::pipeline::{
    LayoutKey, LayoutSpec, PipelineSpec, RenderPipelineId, ShaderVariantKey,
};
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer;
use khora_core::renderer::{
    api::{
        command::{
            LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
            RenderPassDescriptor, StoreOp,
        },
        core::RenderContext,
        pipeline::enums::PrimitiveTopology,
        scene::{
            DirectionalLightUniform, GpuMesh, LightingUniforms, ModelUniforms, PointLightUniform,
            SpotLightUniform, MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
        },
    },
    traits::CommandEncoder,
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::{Mutex, OnceLock, RwLock};

/// Full PBR (Cook-Torrance) lit lane.
///
/// Per CLAD this struct holds only the lane's persistent state — the
/// init / render bodies are private free functions in this module, the
/// trait impl below dispatches to them. No inherent methods other than
/// what the `Lane` and `Default` traits require.
#[derive(Default)]
pub struct StandardPbrLane {
    pipeline: OnceLock<RenderPipelineId>,
    /// Backend pipeline system, retained so the render path can resolve the
    /// per-material-variant pipeline (cheap cache hit). Set in init.
    pipeline_system: OnceLock<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    model_layout: OnceLock<BindGroupLayoutId>,
    material_layout: OnceLock<BindGroupLayoutId>,
    /// Full lighting + shadows bind group layout (group 3).
    light_layout: OnceLock<BindGroupLayoutId>,
    /// Single-binding layout used by the lighting ring buffer.
    lighting_buffer_layout: OnceLock<BindGroupLayoutId>,
    camera_ring: Mutex<Option<UniformRingBuffer>>,
    lighting_ring: Mutex<Option<UniformRingBuffer>>,
    /// Per-frame model (transform) uniforms — written once, bound at a
    /// dynamic offset per draw (no per-draw buffer/bind-group churn).
    model_ring: Mutex<Option<DynamicUniformRingBuffer>>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// The declarative pipeline spec for StandardPbr under a given material
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
        label: "StandardPbr Pipeline",
        shader: "khora::pipelines::standard_pbr",
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

fn init_gpu_resources(
    lane: &StandardPbrLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::resource::CameraUniformData;

    let variant = ShaderVariantKey::empty();
    // Canonical layouts come from the backend's LayoutCache (deduped across
    // lit lanes + shared with the material projection's group-2 bind group).
    let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
    let model_layout = pipeline_system.layout(device, LayoutKey::Model, &variant)?;
    let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
    let light_layout = pipeline_system.layout(device, LayoutKey::Lighting, &variant)?;
    let lighting_buffer_layout =
        pipeline_system.layout(device, LayoutKey::LightingBuffer, &variant)?;

    // Warm the empty-variant pipeline (untextured materials). Textured
    // variants are compiled lazily in the render path on first use, keyed by
    // `GpuMaterial::variant`.
    let pipeline_id =
        pipeline_system.pipeline(device, &pipeline_spec(device, ShaderVariantKey::empty(), false))?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.model_layout.set(model_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.light_layout.set(light_layout);
    let _ = lane.pipeline.set(pipeline_id);

    // Ring buffers (per-lane GPU buffers) built against the shared layouts.
    let camera_ring = UniformRingBuffer::new(
        device,
        camera_layout,
        0,
        std::mem::size_of::<CameraUniformData>() as u64,
        "StandardPbr Camera Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let _ = lane.lighting_buffer_layout.set(lighting_buffer_layout);

    let lighting_ring = UniformRingBuffer::new(
        device,
        lighting_buffer_layout,
        0,
        std::mem::size_of::<LightingUniforms>() as u64,
        "StandardPbr Lighting Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let model_ring = DynamicUniformRingBuffer::new(
        device,
        model_layout,
        0,
        std::mem::size_of::<ModelUniforms>() as u32,
        khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
        khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
        "StandardPbr Model Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    use crate::render_lane::util::lock::mutex_lock_render;
    *mutex_lock_render(&lane.camera_ring, "StandardPbr init.camera_ring")? = Some(camera_ring);
    *mutex_lock_render(&lane.lighting_ring, "StandardPbr init.lighting_ring")? =
        Some(lighting_ring);
    *mutex_lock_render(&lane.model_ring, "StandardPbr init.model_ring")? = Some(model_ring);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_pbr(
    lane: &StandardPbrLane,
    render_world: &RenderWorld,
    shadow_entries: &khora_data::render::ShadowEntries,
    shadow_bindings: Option<khora_data::render::ShadowGpuBindings>,
    device: &dyn khora_core::renderer::GraphicsDevice,
    encoder: &mut dyn CommandEncoder,
    render_ctx: &RenderContext,
    gpu_meshes: &RwLock<Assets<GpuMesh>>,
) {
    use khora_core::renderer::api::{
        command::{BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding},
        resource::CameraUniformData,
    };

    let Some(view) = render_world.views.first() else {
        // No camera — still emit a clear pass so downstream agents see
        // a deterministic depth buffer.
        let color_attachment = RenderPassColorAttachment {
            view: render_ctx.color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(render_ctx.clear_color),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
        };
        let clear_desc = RenderPassDescriptor {
            label: Some("StandardPbr Clear-Only Pass"),
            color_attachments: &[color_attachment],
            depth_stencil_attachment: render_ctx.depth_target.map(|d| {
                RenderPassDepthStencilAttachment {
                    view: d,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                    base_array_layer: 0,
                }
            }),
        };
        let _ = encoder.begin_render_pass(&clear_desc);
        return;
    };

    // Camera ring write.
    let camera_uniforms = CameraUniformData {
        view_projection: view.view_proj.to_cols_array_2d(),
        camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
    };
    let camera_bind_group = {
        let mut lock = crate::lock_or_log!(
            lane.camera_ring.lock(),
            "StandardPbrLane::render camera_ring"
        );
        let ring = match lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("StandardPbrLane: camera ring buffer not initialized");
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

    // Lighting struct (same layout as LitForward).
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
        let (atlas2d_view_proj, atlas2d_index) = match shadow {
            Some(khora_data::render::ShadowEntry::Atlas2D {
                view_proj,
                atlas_index,
            }) => (*view_proj, *atlas_index as f32),
            _ => (Mat4::IDENTITY, -1.0),
        };
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
                if (lighting_uniforms.num_directional_lights as usize) < MAX_DIRECTIONAL_LIGHTS {
                    let idx = lighting_uniforms.num_directional_lights as usize;
                    lighting_uniforms.directional_lights[idx] = DirectionalLightUniform {
                        direction: [light.direction.x, light.direction.y, light.direction.z, 0.0],
                        color: d.color.with_alpha(d.intensity),
                        shadow_view_proj: atlas2d_view_proj.to_cols_array_2d(),
                        shadow_params: [atlas2d_index, d.shadow_bias, d.shadow_normal_bias, 0.0],
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
                        shadow_params: [atlas2d_index, s.shadow_bias, s.shadow_normal_bias, 0.0],
                    };
                    lighting_uniforms.num_spot_lights += 1;
                }
            }
        }
    }

    let (_lighting_bind_group, lighting_ring_buffer_id) = {
        let mut lock = crate::lock_or_log!(
            lane.lighting_ring.lock(),
            "StandardPbrLane::render lighting_ring"
        );
        let ring = match lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("StandardPbrLane: lighting ring buffer not initialized");
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

    let gpu_mesh_assets =
        crate::lock_or_log!(gpu_meshes.read(), "StandardPbrLane::render gpu_meshes");
    // Fallback pipeline (empty variant) used only if the per-variant resolve
    // fails.
    let fallback_pipeline = lane
        .pipeline
        .get()
        .copied()
        .unwrap_or(khora_core::renderer::api::pipeline::RenderPipelineId(0));

    let mut draw_commands = Vec::with_capacity(render_world.meshes.len());
    let mut temp_bind_groups = Vec::new();

    // Model transforms go through the per-frame dynamic ring: one buffer,
    // one bind group, a per-draw offset — no per-draw allocation.
    let mut model_ring_lock =
        crate::lock_or_log!(lane.model_ring.lock(), "StandardPbrLane::render model_ring");
    let model_ring = match model_ring_lock.as_mut() {
        Some(r) => r,
        None => {
            log::warn!("StandardPbrLane: model ring buffer not initialized");
            return;
        }
    };
    model_ring.advance();

    for extracted_mesh in &render_world.meshes {
        let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) else {
            continue;
        };
        // The material projection tags every rendered entity with a
        // `GpuMaterial` before `RenderFlow` runs; skip a draw until its
        // material has been uploaded.
        let Some(gpu_material) = &extracted_mesh.gpu_material else {
            continue;
        };
        // Resolve the pipeline for this material's texture variant (cheap
        // cache hit after first compile; the lane never retains a per-variant
        // `RenderPipelineId`).
        let pipeline_id = lane
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
                log::error!("StandardPbrLane: pipeline resolve failed: {:?}", e);
                None
            })
            .unwrap_or(fallback_pipeline);
        let model_mat = extracted_mesh.transform.to_matrix();
        let normal_mat = match model_mat.inverse() {
            Some(inv) => inv.transpose(),
            None => continue,
        };
        let model_uniforms = ModelUniforms {
            model_matrix: model_mat.to_cols_array_2d(),
            normal_matrix: normal_mat.to_cols_array_2d(),
        };
        let model_offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
            Ok(offset) => offset,
            Err(e) => {
                log::error!("StandardPbrLane: failed to push model uniforms: {:?}", e);
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
    // pass — avoids per-draw pipeline thrash when materials mix variants.
    draw_commands.sort_by_key(|cmd| cmd.pipeline.0);

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
        label: Some("Standard PBR Pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: render_ctx.depth_target.map(|d| {
            RenderPassDepthStencilAttachment {
                view: d,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
                base_array_layer: 0,
            }
        }),
    };

    // Build the per-frame lighting bind group (group 3).
    let final_lighting_bind_group = if let Some(layout) = lane.light_layout.get().copied() {
        let Some(shadow_bindings) = shadow_bindings else {
            log::warn!("StandardPbrLane: ShadowGpuBindings not available, skipping render");
            return;
        };
        let mut entries = vec![BindGroupEntry {
            binding: khora_data::render::shadow_bindings::binding::LIGHTING_UNIFORMS,
            resource: BindingResource::Buffer(BufferBinding {
                buffer: lighting_ring_buffer_id,
                offset: 0,
                size: None,
            }),
            _phantom: std::marker::PhantomData,
        }];
        khora_data::render::shadow_bindings::fill_shadow_bind_group_entries(
            &shadow_bindings,
            &mut entries,
        );
        match device.create_bind_group(&BindGroupDescriptor {
            label: Some("standard_pbr_lighting_bind_group_dynamic"),
            layout,
            entries: &entries,
        }) {
            Ok(bg) => {
                temp_bind_groups.push(bg);
                bg
            }
            Err(e) => {
                log::error!(
                    "StandardPbrLane: failed to create lighting bind group: {:?}",
                    e
                );
                return;
            }
        }
    } else {
        log::warn!("StandardPbrLane: light layout not initialized");
        return;
    };

    let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
    render_pass.set_bind_group(0, &camera_bind_group, &[]);
    render_pass.set_bind_group(3, &final_lighting_bind_group, &[]);

    let mut current_pipeline = None;
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

    drop(render_pass);
    // Only the per-frame group-3 lighting bind group is transient now;
    // model uniforms live in the ring, materials in the GpuMaterial cache.
    for bg in temp_bind_groups {
        let _ = device.destroy_bind_group(bg);
    }
}

impl khora_core::lane::Lane for StandardPbrLane {
    fn strategy_name(&self) -> &'static str {
        "StandardPbr"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Render
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
        init_gpu_resources(self, device.as_ref(), pipeline_system.as_ref())
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

        let (shadow_entries, shadow_bindings) = ctx
            .get::<Slot<khora_core::lane::OutputDeck>>()
            .map(|s| {
                let frame = s
                    .get()
                    .slot::<khora_core::renderer::api::shadow::ShadowFrame>();
                (frame.entries.clone(), frame.bindings)
            })
            .unwrap_or_default();

        render_pbr(
            self,
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::Lane;

    #[test]
    fn standard_pbr_lane_strategy_name() {
        let lane = StandardPbrLane::default();
        assert_eq!(lane.strategy_name(), "StandardPbr");
        assert_eq!(lane.lane_kind(), khora_core::lane::LaneKind::Render);
    }
}
