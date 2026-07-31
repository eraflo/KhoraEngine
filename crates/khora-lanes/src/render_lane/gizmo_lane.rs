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

//! Gizmo overlay lane — editor / debug line gizmos.
//!
//! Reads a [`GizmoFrame`](khora_data::render::GizmoFrame) (shared via
//! an `Arc<Mutex<…>>` runtime resource — see `OverlayAgent`) and draws
//! each `GizmoLineInstance` as an alpha-blended line list on top of
//! the main render target. The host application (editor, debug tooling)
//! is the sole producer of the lines; the engine only provides the
//! mechanism — no `EditorAgent` in the engine, per CLAD.
//!
//! Per CLAD this struct holds only persistent state; the init / render
//! bodies are private free functions in this module.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::traits::CommandEncoder;
use khora_core::ui::editor::GizmoLineInstance;
use khora_data::render::{GizmoFrame, RenderWorld};
use std::sync::{Arc, Mutex, OnceLock};

/// Maximum number of gizmo line instances the storage buffer holds.
const GIZMO_CAPACITY: usize = 4096;

/// Shared gizmo line container — the host app writes, `GizmoLane` reads.
pub type SharedGizmoFrame = Arc<Mutex<GizmoFrame>>;

/// Editor / debug gizmo overlay lane.
#[derive(Debug)]
pub struct GizmoLane {
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    storage_layout: OnceLock<BindGroupLayoutId>,
    camera_buffer: OnceLock<BufferId>,
    storage_buffer: OnceLock<BufferId>,
    camera_bind_group: OnceLock<BindGroupId>,
    storage_bind_group: OnceLock<BindGroupId>,
    /// Maximum number of line instances the storage buffer can hold.
    capacity: usize,
}

impl Default for GizmoLane {
    fn default() -> Self {
        Self {
            pipeline: OnceLock::new(),
            camera_layout: OnceLock::new(),
            storage_layout: OnceLock::new(),
            camera_buffer: OnceLock::new(),
            storage_buffer: OnceLock::new(),
            camera_bind_group: OnceLock::new(),
            storage_bind_group: OnceLock::new(),
            capacity: GIZMO_CAPACITY,
        }
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &GizmoLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::{
        command::{BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding},
        resource::{BufferDescriptor, BufferUsage},
    };
    use std::borrow::Cow;

    // Bespoke layouts resolved + cached by the PipelineSystem so the pipeline
    // and the lane's bind groups share one layout id.
    let camera_layout = pipeline_system.inline_layout(
        device,
        GIZMO_CAMERA_LAYOUT_LABEL,
        &gizmo_camera_layout_entries(),
    )?;
    let storage_layout = pipeline_system.inline_layout(
        device,
        GIZMO_STORAGE_LAYOUT_LABEL,
        &gizmo_storage_layout_entries(),
    )?;

    let camera_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("gizmo_camera_ubo")),
            size: std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>()
                as u64,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;
    let storage_size = (lane.capacity * std::mem::size_of::<GizmoLineInstance>()) as u64;
    let storage_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("gizmo_storage_buffer")),
            size: storage_size,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let camera_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("gizmo_camera_bg"),
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
    let storage_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("gizmo_storage_bg"),
            layout: storage_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: storage_buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    // Pipeline — compiled + cached by the backend from
    // `khora::pipelines::gizmo`.
    let pipeline_id = pipeline_system.pipeline(device, &gizmo_pipeline_spec(device))?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.storage_layout.set(storage_layout);
    let _ = lane.camera_buffer.set(camera_buffer);
    let _ = lane.storage_buffer.set(storage_buffer);
    let _ = lane.camera_bind_group.set(camera_bind_group);
    let _ = lane.storage_bind_group.set(storage_bind_group);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Stable cache labels for the gizmo lane's bespoke layouts.
const GIZMO_CAMERA_LAYOUT_LABEL: &str = "gizmo_camera_layout";
const GIZMO_STORAGE_LAYOUT_LABEL: &str = "gizmo_storage_layout";

/// Group-0 camera layout: a single uniform buffer (vertex stage).
fn gizmo_camera_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Group-1 storage layout: the read-only line-instance storage buffer.
fn gizmo_storage_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// The declarative pipeline spec for the gizmo overlay — line-list, alpha
/// blended, no depth (always on top).
fn gizmo_pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        BlendFactor, BlendOperation, PrimitiveTopology,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendComponentDescriptor, BlendStateDescriptor, ColorWrites,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, LayoutSpec, MultisampleStateDescriptor, PipelineSpec,
        PrimitiveStateDescriptor, ShaderVariantKey,
    };
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    // Alpha blend; no depth attachment — gizmos always draw on top (the legacy
    // infra path used `CompareFunction::Always`, equivalent to no depth test
    // for an overlay).
    let blend = BlendStateDescriptor {
        color: BlendComponentDescriptor {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponentDescriptor {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
    };

    PipelineSpec {
        label: "Gizmo Pipeline",
        shader: "khora::pipelines::gizmo",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: GIZMO_CAMERA_LAYOUT_LABEL,
                entries: Cow::Owned(gizmo_camera_layout_entries()),
            },
            LayoutSpec::Inline {
                label: GIZMO_STORAGE_LAYOUT_LABEL,
                entries: Cow::Owned(gizmo_storage_layout_entries()),
            },
        ],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::LineList,
            ..Default::default()
        },
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: device
                .get_surface_format()
                .unwrap_or(TextureFormat::Rgba8UnormSrgb),
            blend: Some(blend),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

fn render_gizmos(
    lane: &GizmoLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    encoder: &mut dyn CommandEncoder,
    color_target: khora_core::renderer::api::resource::TextureViewId,
    view: &khora_data::render::ExtractedView,
    lines: &[GizmoLineInstance],
) {
    use khora_core::renderer::api::command::{
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
    };
    use khora_core::renderer::api::resource::CameraUniformData;

    let line_count = lines.len().min(lane.capacity);
    if line_count == 0 {
        return;
    }

    let (
        Some(pipeline),
        Some(camera_buffer),
        Some(storage_buffer),
        Some(camera_bg),
        Some(storage_bg),
    ) = (
        lane.pipeline.get().copied(),
        lane.camera_buffer.get().copied(),
        lane.storage_buffer.get().copied(),
        lane.camera_bind_group.get().copied(),
        lane.storage_bind_group.get().copied(),
    )
    else {
        log::warn!("GizmoLane: GPU resources not initialized, skipping");
        return;
    };

    // Upload camera + line data into the persistent buffers (the bind
    // groups created at init still point at them).
    let camera_uniforms = CameraUniformData {
        view_projection: view.view_proj.to_cols_array_2d(),
        camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
    };
    if let Err(e) = device.write_buffer(camera_buffer, 0, bytemuck::bytes_of(&camera_uniforms)) {
        log::error!("GizmoLane: camera buffer write failed: {:?}", e);
        return;
    }
    if let Err(e) = device.write_buffer(
        storage_buffer,
        0,
        bytemuck::cast_slice(&lines[..line_count]),
    ) {
        log::error!("GizmoLane: storage buffer write failed: {:?}", e);
        return;
    }

    // Overlay pass — LoadOp::Load preserves the main render's color.
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
        label: Some("Gizmo Overlay Pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: None,
    };

    let mut pass = encoder.begin_render_pass(&pass_desc);
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &camera_bg, &[]);
    pass.set_bind_group(1, &storage_bg, &[]);
    // Two vertices per line; the vertex shader derives endpoints from
    // `vertex_index` (no vertex buffer bound).
    pass.draw(0..(line_count as u32 * 2), 0..1);
}

impl Lane for GizmoLane {
    fn strategy_name(&self) -> &'static str {
        "Gizmo"
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
        // The shared `GizmoFrame` is published by the host application.
        // Absent ⇒ no gizmo producer this run ⇒ nothing to draw.
        let Some(shared) = ctx.get::<SharedGizmoFrame>() else {
            return Ok(());
        };
        let lines: Vec<GizmoLineInstance> = match shared.lock() {
            Ok(frame) => {
                if frame.lines.is_empty() {
                    return Ok(());
                }
                frame.lines.clone()
            }
            Err(_) => {
                log::warn!("GizmoLane: shared GizmoFrame mutex poisoned");
                return Ok(());
            }
        };

        // Camera comes from the primary extracted view — the editor
        // viewport override is already folded into `RenderWorld.views`
        // by `RenderFlow`.
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
        let encoder = ctx
            .get::<Slot<dyn CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
            .get();
        let color_target = ctx
            .get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?
            .0;

        render_gizmos(self, device.as_ref(), encoder, color_target, &view, &lines);
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
    fn gizmo_lane_strategy_name() {
        let lane = GizmoLane::default();
        assert_eq!(lane.strategy_name(), "Gizmo");
        assert_eq!(lane.lane_kind(), LaneKind::Render);
        assert_eq!(lane.capacity, GIZMO_CAPACITY);
    }
}
