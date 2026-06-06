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

//! Wireframe overlay lane — debug visualization rendering meshes as
//! line lists for triangle-edge visibility.
//!
//! Registered under [`OverlayAgent`](khora_agents::overlay_agent). The
//! pipeline is composed from `khora::pipelines::wireframe` and uses a
//! barycentric edge-detection fragment shader (does not rely on
//! `Features::POLYGON_MODE_LINE`, which is not part of wgpu's
//! downlevel-safe baseline).
//!
//! The pipeline is created at init via the `PipelineSystem` backend;
//! `execute()` is a no-op until a debug flag / filter exists in the
//! engine context to gate the pass. Adding that flag is a follow-up —
//! the lane is in place so wiring + the agent contract are exercised.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind};
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use std::sync::OnceLock;

/// Wireframe debug overlay lane.
///
/// Per CLAD this struct holds only persistent state — the GPU init
/// body is a private free function in this module; the `Lane` impl
/// dispatches to it. No inherent methods, construction via `Default`.
#[derive(Debug, Default)]
pub struct WireframeLane {
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    model_layout: OnceLock<BindGroupLayoutId>,
    material_layout: OnceLock<BindGroupLayoutId>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &WireframeLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::util::ShaderStageFlags;

    // Bespoke layouts resolved + cached by the PipelineSystem so the pipeline
    // and any future bind groups share one layout id.
    let camera_layout = pipeline_system.inline_layout(
        device,
        WIREFRAME_CAMERA_LAYOUT_LABEL,
        &wireframe_uniform_layout_entries(ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT),
    )?;
    let model_layout = pipeline_system.inline_layout(
        device,
        WIREFRAME_MODEL_LAYOUT_LABEL,
        &wireframe_uniform_layout_entries(ShaderStageFlags::VERTEX),
    )?;
    let material_layout = pipeline_system.inline_layout(
        device,
        WIREFRAME_MATERIAL_LAYOUT_LABEL,
        &wireframe_uniform_layout_entries(ShaderStageFlags::FRAGMENT),
    )?;

    // Pipeline — compiled + cached by the backend from
    // `khora::pipelines::wireframe`.
    let pipeline_id = pipeline_system.pipeline(device, &wireframe_pipeline_spec(device))?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.model_layout.set(model_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Stable cache labels for the wireframe lane's bespoke layouts.
const WIREFRAME_CAMERA_LAYOUT_LABEL: &str = "wireframe_camera_layout";
const WIREFRAME_MODEL_LAYOUT_LABEL: &str = "wireframe_model_layout";
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
        BlendFactor, BlendOperation, CompareFunction, VertexFormat, VertexStepMode,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendComponentDescriptor, BlendStateDescriptor, ColorWrites, DepthBiasState,
        StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
        MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor, ShaderVariantKey,
        VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    // Alpha-blend so wireframe edges anti-alias against the underlying scene.
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
            LayoutSpec::Inline {
                label: WIREFRAME_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(wireframe_uniform_layout_entries(ShaderStageFlags::VERTEX)),
            },
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
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
            ]),
        }],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
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

impl Lane for WireframeLane {
    fn strategy_name(&self) -> &'static str {
        "Wireframe"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Render
    }

    fn on_initialize(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let pipeline_system = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .ok_or(LaneError::missing("Arc<dyn PipelineSystem>"))?
            .clone();
        init_gpu_resources(self, device.as_ref(), pipeline_system.as_ref())
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
        // TODO — iterate `RenderWorld.meshes` and draw via wireframe
        // shader once a debug-flag gate is exposed through the engine
        // context (e.g. `WireframeDebugFlag` in `FrameContext`). Tracked
        // as a follow-up; the lane is wired into `OverlayAgent` so the
        // pipeline gets validated at boot.
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
}
