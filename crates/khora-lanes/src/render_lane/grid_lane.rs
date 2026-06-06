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

//! Grid overlay lane — the infinite editor ground grid.
//!
//! Renders `khora::pipelines::grid` (a fullscreen-triangle infinite
//! grid) on top of the main render target, depth-tested against the
//! scene so closer geometry occludes it. Registered under
//! [`OverlayAgent`](khora_agents::overlay_agent), it runs in the OUTPUT
//! phase **after** the scene pass — `LoadOp::Load` preserves what the
//! scene drew.
//!
//! The grid is debug / editor viz: a host application opts in by
//! enabling the shared [`GridConfig`](khora_data::render::GridConfig)
//! runtime resource. The sandbox leaves it disabled — no grid.
//!
//! Per CLAD this struct holds only persistent state; init / render
//! bodies are private free functions in this module.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::render::{GridConfig, RenderWorld};
use std::sync::{Arc, Mutex, OnceLock};

/// Shared editor-grid config — the host app enables it, `GridLane` reads it.
pub type SharedGridConfig = Arc<Mutex<GridConfig>>;

/// Infinite editor ground-grid overlay lane.
#[derive(Debug, Default)]
pub struct GridLane {
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    camera_buffer: OnceLock<BufferId>,
    camera_bind_group: OnceLock<BindGroupId>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &GridLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::{
        command::{BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding},
        resource::{BufferDescriptor, BufferUsage, CameraUniformData},
    };
    use std::borrow::Cow;

    // Group 0 — camera UBO (mat4 view-proj + vec4 position = 80 B). Bespoke
    // layout resolved + cached by the PipelineSystem so the pipeline and the
    // lane's bind group share one layout id.
    let camera_layout = pipeline_system.inline_layout(
        device,
        GRID_CAMERA_LAYOUT_LABEL,
        &grid_camera_layout_entries(),
    )?;

    let camera_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("grid_camera_ubo")),
            size: std::mem::size_of::<CameraUniformData>() as u64,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let camera_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("grid_camera_bg"),
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

    // Pipeline — compiled + cached by the backend from `khora::pipelines::grid`.
    let pipeline_id = pipeline_system.pipeline(device, &grid_pipeline_spec(device))?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.camera_buffer.set(camera_buffer);
    let _ = lane.camera_bind_group.set(camera_bind_group);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Stable cache label for the grid camera layout.
const GRID_CAMERA_LAYOUT_LABEL: &str = "grid_camera_layout";

/// Group-0 camera layout: a single uniform buffer (vertex + fragment).
fn grid_camera_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// The declarative pipeline spec for the grid overlay — fullscreen triangle,
/// alpha-blended, depth-tested (LessEqual) against the scene buffer.
fn grid_pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        BlendFactor, BlendOperation, CompareFunction, PrimitiveTopology,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendComponentDescriptor, BlendStateDescriptor, ColorWrites, DepthBiasState,
        StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
        MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor, ShaderVariantKey,
    };
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    // Alpha blend — antialiased grid lines fade against the scene.
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
        label: "Grid Pipeline",
        shader: "khora::pipelines::grid",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![LayoutSpec::Inline {
            label: GRID_CAMERA_LAYOUT_LABEL,
            entries: Cow::Owned(grid_camera_layout_entries()),
        }],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        // The grid fragment shader writes `@builtin(frag_depth)`; depth
        // testing against the scene buffer (LessEqual) lets closer geometry
        // occlude the grid.
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
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

fn render_grid(
    lane: &GridLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    encoder: &mut dyn CommandEncoder,
    color_target: khora_core::renderer::api::resource::TextureViewId,
    depth_target: khora_core::renderer::api::resource::TextureViewId,
    view: &khora_data::render::ExtractedView,
) {
    use khora_core::renderer::api::command::{
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
        RenderPassDescriptor, StoreOp,
    };
    use khora_core::renderer::api::resource::CameraUniformData;

    let (Some(pipeline), Some(camera_buffer), Some(camera_bg)) = (
        lane.pipeline.get().copied(),
        lane.camera_buffer.get().copied(),
        lane.camera_bind_group.get().copied(),
    ) else {
        log::warn!("GridLane: GPU resources not initialized, skipping");
        return;
    };

    // Upload the camera uniforms (view-projection + position).
    let camera_uniforms = CameraUniformData {
        view_projection: view.view_proj.to_cols_array_2d(),
        camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
    };
    if let Err(e) = device.write_buffer(camera_buffer, 0, bytemuck::bytes_of(&camera_uniforms)) {
        log::error!("GridLane: camera buffer write failed: {:?}", e);
        return;
    }

    // Overlay pass — `LoadOp::Load` preserves the scene's color + depth.
    let color_attachment = RenderPassColorAttachment {
        view: &color_target,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
        base_array_layer: 0,
    };
    let pass_desc = RenderPassDescriptor {
        label: Some("Grid Overlay Pass"),
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
    // Fullscreen triangle pair — the vertex shader derives positions
    // from `vertex_index` (no vertex buffer bound).
    pass.draw(0..6, 0..1);
}

impl Lane for GridLane {
    fn strategy_name(&self) -> &'static str {
        "Grid"
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
        // The grid is opt-in: a host application enables the shared
        // `GridConfig`. Absent or disabled ⇒ nothing to draw.
        let enabled = ctx
            .get::<SharedGridConfig>()
            .and_then(|cfg| cfg.lock().ok().map(|c| c.enabled))
            .unwrap_or(false);
        if !enabled {
            return Ok(());
        }

        // Camera comes from the primary extracted view (the editor
        // viewport override is folded into `RenderWorld.views` by
        // `RenderFlow`).
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
        // The grid depth-tests against the scene buffer — no depth
        // target ⇒ skip (cannot run the depth-enabled pipeline).
        let Some(depth_target) = ctx.get::<khora_core::lane::DepthTarget>().map(|d| d.0) else {
            return Ok(());
        };

        render_grid(
            self,
            device.as_ref(),
            encoder,
            color_target,
            depth_target,
            &view,
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
    fn grid_lane_strategy_name() {
        let lane = GridLane::default();
        assert_eq!(lane.strategy_name(), "Grid");
        assert_eq!(lane.lane_kind(), LaneKind::Render);
    }
}
