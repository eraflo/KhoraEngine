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

//! Wireframe overlay lane — debug visualization drawing every scene mesh as
//! edge lines for triangle-edge visibility.
//!
//! Registered under [`OverlayAgent`](khora_agents::overlay_agent). The pipeline
//! is composed from `khora::pipelines::wireframe` and uses a barycentric
//! edge-detection fragment shader (it does not rely on
//! `Features::POLYGON_MODE_LINE`, which is not part of wgpu's downlevel-safe
//! baseline).
//!
//! Opt-in like the grid: a host application enables the shared
//! [`WireframeConfig`](khora_data::render::WireframeConfig) runtime resource
//! (the editor drives it from a viewport toggle; the sandbox leaves it off).
//! The lane loads the scene's color + depth read-only and draws after the scene
//! pass, so the wireframe overlays the lit image.
//!
//! Per CLAD this struct holds only persistent state; init / render bodies are
//! private free functions in this module.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::assets::Assets;
use khora_data::render::{RenderWorld, WireframeConfig};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// Shared wireframe-overlay config — the host app enables it, `WireframeLane`
/// reads it.
pub type SharedWireframeConfig = Arc<Mutex<WireframeConfig>>;

/// Wireframe material uniform — matches `WireframeMaterialUniforms` in
/// `wireframe.wgsl` (line color + width, padded to 16 bytes).
#[repr(C, align(16))]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WireframeMaterialUniforms {
    color: [f32; 4],
    line_width: f32,
    _padding: [f32; 3],
}

/// Wireframe debug overlay lane.
#[derive(Debug, Default)]
pub struct WireframeLane {
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    material_layout: OnceLock<BindGroupLayoutId>,
    camera_buffer: OnceLock<BufferId>,
    camera_bind_group: OnceLock<BindGroupId>,
    material_buffer: OnceLock<BufferId>,
    material_bind_group: OnceLock<BindGroupId>,
    /// Per-mesh model transforms — one buffer, a per-draw dynamic offset, same
    /// machinery the lit lanes use.
    model_ring: Mutex<Option<DynamicUniformRingBuffer>>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &WireframeLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding,
    };
    use khora_core::renderer::api::pipeline::{LayoutKey, ShaderVariantKey};
    use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage, CameraUniformData};
    use khora_core::renderer::api::scene::ModelUniforms;
    use khora_core::renderer::api::util::dynamic_uniform_buffer::{
        DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    use std::borrow::Cow;

    // Group 0 (camera) + group 2 (material) are single bespoke uniforms; group
    // 1 (per-mesh model) reuses the canonical dynamic Model layout so the ring
    // matches the pipeline byte-for-byte.
    let camera_layout = pipeline_system.inline_layout(
        device,
        WIREFRAME_CAMERA_LAYOUT_LABEL,
        &wireframe_uniform_layout_entries(ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT),
    )?;
    let material_layout = pipeline_system.inline_layout(
        device,
        WIREFRAME_MATERIAL_LAYOUT_LABEL,
        &wireframe_uniform_layout_entries(ShaderStageFlags::FRAGMENT),
    )?;
    let model_layout =
        pipeline_system.layout(device, LayoutKey::Model, &ShaderVariantKey::empty())?;

    let pipeline_id = pipeline_system.pipeline(device, &wireframe_pipeline_spec(device))?;

    let camera_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("wireframe_camera_ubo")),
            size: std::mem::size_of::<CameraUniformData>() as u64,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;
    let camera_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("wireframe_camera_bg"),
            layout: camera_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: camera_buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let material_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("wireframe_material_ubo")),
            size: std::mem::size_of::<WireframeMaterialUniforms>() as u64,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;
    let material_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("wireframe_material_bg"),
            layout: material_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: material_buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let model_ring = DynamicUniformRingBuffer::new(
        device,
        model_layout,
        0,
        std::mem::size_of::<ModelUniforms>() as u32,
        DEFAULT_MAX_ELEMENTS,
        MIN_UNIFORM_ALIGNMENT,
        "Wireframe Model Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.camera_buffer.set(camera_buffer);
    let _ = lane.camera_bind_group.set(camera_bind_group);
    let _ = lane.material_buffer.set(material_buffer);
    let _ = lane.material_bind_group.set(material_bind_group);
    *crate::render_lane::util::lock::mutex_lock_render(
        &lane.model_ring,
        "WireframeLane init.model_ring",
    )? = Some(model_ring);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Stable cache labels for the wireframe lane's bespoke layouts.
const WIREFRAME_CAMERA_LAYOUT_LABEL: &str = "wireframe_camera_layout";
const WIREFRAME_MATERIAL_LAYOUT_LABEL: &str = "wireframe_material_layout";

/// A single uniform-buffer layout entry visible to `visibility`.
fn wireframe_uniform_layout_entries(
    visibility: khora_core::renderer::api::util::ShaderStageFlags,
) -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// The declarative pipeline spec for the wireframe overlay — barycentric edge
/// detection, alpha-blended, depth read-only (LessEqual).
fn wireframe_pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        CompareFunction, VertexFormat, VertexStepMode,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendStateDescriptor, ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutKey, LayoutSpec,
        MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor, ShaderVariantKey,
        VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    PipelineSpec {
        label: "Wireframe Pipeline",
        shader: "khora::pipelines::wireframe",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: WIREFRAME_CAMERA_LAYOUT_LABEL,
                entries: Cow::Owned(wireframe_uniform_layout_entries(
                    ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                )),
            },
            LayoutSpec::Named(LayoutKey::Model),
            LayoutSpec::Inline {
                label: WIREFRAME_MATERIAL_LAYOUT_LABEL,
                entries: Cow::Owned(wireframe_uniform_layout_entries(ShaderStageFlags::FRAGMENT)),
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
            ]),
        }],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            // Overlay: depth-test against the scene so hidden edges are
            // occluded, but never write depth.
            depth_write_enabled: false,
            depth_compare: CompareFunction::LessEqual,
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
            blend: Some(BlendStateDescriptor::alpha_blending()),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn render_wireframe(
    lane: &WireframeLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    encoder: &mut dyn CommandEncoder,
    color_target: khora_core::renderer::api::resource::TextureViewId,
    depth_target: khora_core::renderer::api::resource::TextureViewId,
    view: &khora_data::render::ExtractedView,
    render_world: &RenderWorld,
    gpu_meshes: &RwLock<Assets<GpuMesh>>,
    config: &WireframeConfig,
) {
    use khora_core::renderer::api::command::{
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
        RenderPassDescriptor, StoreOp,
    };
    use khora_core::renderer::api::resource::CameraUniformData;
    use khora_core::renderer::api::scene::ModelUniforms;

    let (
        Some(pipeline),
        Some(camera_buffer),
        Some(camera_bg),
        Some(material_buffer),
        Some(material_bg),
    ) = (
        lane.pipeline.get().copied(),
        lane.camera_buffer.get().copied(),
        lane.camera_bind_group.get().copied(),
        lane.material_buffer.get().copied(),
        lane.material_bind_group.get().copied(),
    )
    else {
        log::warn!("WireframeLane: GPU resources not initialized, skipping");
        return;
    };

    // Camera + material uniforms (the latter picks up live editor changes to
    // color / width).
    let camera_uniforms = CameraUniformData {
        view_projection: view.view_proj.to_cols_array_2d(),
        camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
    };
    if let Err(e) = device.write_buffer(camera_buffer, 0, bytemuck::bytes_of(&camera_uniforms)) {
        log::error!("WireframeLane: camera buffer write failed: {:?}", e);
        return;
    }
    let material_uniforms = WireframeMaterialUniforms {
        color: [
            config.line_color.r,
            config.line_color.g,
            config.line_color.b,
            config.line_color.a,
        ],
        line_width: config.line_width,
        _padding: [0.0; 3],
    };
    if let Err(e) = device.write_buffer(material_buffer, 0, bytemuck::bytes_of(&material_uniforms))
    {
        log::error!("WireframeLane: material buffer write failed: {:?}", e);
        return;
    }

    let gpu_mesh_assets =
        crate::lock_or_log!(gpu_meshes.read(), "WireframeLane::render gpu_meshes");
    let mut model_ring_lock =
        crate::lock_or_log!(lane.model_ring.lock(), "WireframeLane::render model_ring");
    let model_ring = match model_ring_lock.as_mut() {
        Some(r) => r,
        None => {
            log::warn!("WireframeLane: model ring not initialized");
            return;
        }
    };
    model_ring.advance();

    // Build one draw per mesh up front (the ring's Copy handles only, no borrow
    // of the asset guard) so the render pass does not overlap the mutable push.
    use khora_core::renderer::api::util::IndexFormat;
    let mut draws: Vec<(u32, BufferId, BufferId, IndexFormat, u32)> =
        Vec::with_capacity(render_world.meshes.len());
    for extracted_mesh in &render_world.meshes {
        let Some(gpu_mesh) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) else {
            continue;
        };
        let model_mat = extracted_mesh.transform.to_matrix();
        let normal_mat = model_mat
            .inverse()
            .map(|inv| inv.transpose())
            .unwrap_or(model_mat);
        let model_uniforms = ModelUniforms {
            model_matrix: model_mat.to_cols_array_2d(),
            normal_matrix: normal_mat.to_cols_array_2d(),
        };
        match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
            Ok(offset) => draws.push((
                offset,
                gpu_mesh.vertex_buffer,
                gpu_mesh.index_buffer,
                gpu_mesh.index_format,
                gpu_mesh.index_count,
            )),
            Err(e) => {
                log::error!("WireframeLane: model push failed: {:?}", e);
                return;
            }
        }
    }
    let model_bg = *model_ring.current_bind_group();

    // Overlay pass — `LoadOp::Load` preserves the scene's color + depth.
    let color_attachment = RenderPassColorAttachment {
        view: &color_target,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
        base_array_layer: 0,
        base_mip_level: 0,
    };
    let pass_desc = RenderPassDescriptor {
        label: Some("Wireframe Overlay Pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &depth_target,
            depth_ops: Some(Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            }),
            stencil_ops: None,
            base_array_layer: 0,
        }),
    };

    let mut pass = encoder.begin_render_pass(&pass_desc);
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &camera_bg, &[]);
    pass.set_bind_group(2, &material_bg, &[]);
    for (offset, vertex_buffer, index_buffer, index_format, index_count) in &draws {
        pass.set_bind_group(1, &model_bg, &[*offset]);
        pass.set_vertex_buffer(0, vertex_buffer, 0);
        pass.set_index_buffer(index_buffer, 0, *index_format);
        pass.draw_indexed(0..*index_count, 0, 0..1);
    }
}

impl Lane for WireframeLane {
    fn strategy_name(&self) -> &'static str {
        "Wireframe"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Render
    }

    fn on_initialize(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let device = ctx
            .get::<Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let pipeline_system = ctx
            .get::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .ok_or(LaneError::missing("Arc<dyn PipelineSystem>"))?
            .clone();
        init_gpu_resources(self, device.as_ref(), pipeline_system.as_ref())
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        // Opt-in: a host application enables the shared `WireframeConfig`.
        // Absent or disabled ⇒ nothing to draw.
        let config = ctx
            .get::<SharedWireframeConfig>()
            .and_then(|cfg| cfg.lock().ok().map(|c| c.clone()));
        let Some(config) = config else {
            return Ok(());
        };
        if !config.enabled {
            return Ok(());
        }

        let Some(render_world) = ctx.get::<Ref<RenderWorld>>() else {
            return Ok(());
        };
        let render_world = render_world.get();
        let Some(view) = render_world.views.first() else {
            return Ok(());
        };
        let view = view.clone();

        let device = ctx
            .get::<Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let gpu_meshes = ctx
            .get::<Arc<RwLock<Assets<GpuMesh>>>>()
            .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?
            .clone();
        let encoder = ctx
            .get::<Slot<dyn CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
            .get();
        let color_target = ctx
            .get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?
            .0;
        // The wireframe depth-tests against the scene buffer — no depth target
        // ⇒ skip (cannot run the depth-enabled pipeline).
        let Some(depth_target) = ctx.get::<khora_core::lane::DepthTarget>().map(|d| d.0) else {
            return Ok(());
        };

        render_wireframe(
            self,
            device.as_ref(),
            encoder,
            color_target,
            depth_target,
            &view,
            render_world,
            gpu_meshes.as_ref(),
            &config,
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

    #[test]
    fn wireframe_lane_strategy_name() {
        let lane = WireframeLane::default();
        assert_eq!(lane.strategy_name(), "Wireframe");
        assert_eq!(lane.lane_kind(), LaneKind::Render);
    }

    #[test]
    fn wireframe_material_uniforms_is_std140_sized() {
        assert_eq!(std::mem::size_of::<WireframeMaterialUniforms>(), 32);
        assert_eq!(std::mem::align_of::<WireframeMaterialUniforms>(), 16);
    }
}
