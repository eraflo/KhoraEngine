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

//! Emissive overlay lane — additive pass over the main render for
//! meshes flagged as emissive.
//!
//! Registered under [`OverlayAgent`](khora_agents::overlay_agent), runs
//! every frame after the main lit pass. Uses the
//! `khora::pipelines::emissive` shader resolved through the
//! `PipelineSystem` backend.
//!
//! The pipeline is created at init through the `PipelineSystem` (so the
//! shader is validated and the GPU resources are ready), but `execute()`
//! is a no-op until a CPU-side `MaterialKind::Emissive` flag exists in
//! `khora-data` to filter the `RenderWorld.meshes` iterator. Adding that
//! flag is a follow-up; the lane is in place so wiring + the agent
//! contract are exercised by the existing test harness.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind};
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use std::sync::OnceLock;

/// Additive emissive overlay lane.
///
/// Per CLAD this struct holds only persistent state — the GPU init
/// body is a private free function in this module; the `Lane` impl
/// dispatches to it. No inherent methods, construction via `Default`.
#[derive(Debug, Default)]
pub struct EmissiveLane {
    pipeline: OnceLock<RenderPipelineId>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    model_layout: OnceLock<BindGroupLayoutId>,
    material_layout: OnceLock<BindGroupLayoutId>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &EmissiveLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    // Bespoke layouts resolved + cached by the PipelineSystem so the pipeline
    // and any future bind groups share one layout id.
    let camera_layout = pipeline_system.inline_layout(
        device,
        EMISSIVE_CAMERA_LAYOUT_LABEL,
        &emissive_uniform_layout_entries(
            khora_core::renderer::api::util::ShaderStageFlags::VERTEX
                | khora_core::renderer::api::util::ShaderStageFlags::FRAGMENT,
        ),
    )?;
    let model_layout = pipeline_system.inline_layout(
        device,
        EMISSIVE_MODEL_LAYOUT_LABEL,
        &emissive_uniform_layout_entries(khora_core::renderer::api::util::ShaderStageFlags::VERTEX),
    )?;
    let material_layout = pipeline_system.inline_layout(
        device,
        EMISSIVE_MATERIAL_LAYOUT_LABEL,
        &emissive_uniform_layout_entries(
            khora_core::renderer::api::util::ShaderStageFlags::FRAGMENT,
        ),
    )?;

    // Pipeline — compiled + cached by the backend from
    // `khora::pipelines::emissive`.
    let pipeline_id = pipeline_system.pipeline(device, &emissive_pipeline_spec(device))?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.model_layout.set(model_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Stable cache labels for the emissive lane's bespoke layouts.
const EMISSIVE_CAMERA_LAYOUT_LABEL: &str = "emissive_camera_layout";
const EMISSIVE_MODEL_LAYOUT_LABEL: &str = "emissive_model_layout";
const EMISSIVE_MATERIAL_LAYOUT_LABEL: &str = "emissive_material_layout";

/// A single uniform-buffer layout entry visible to `visibility`.
fn emissive_uniform_layout_entries(
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

/// The declarative pipeline spec for the emissive overlay — additive blend,
/// depth read-only (LessEqual), standard mesh vertex layout.
fn emissive_pipeline_spec(
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

    // Additive blend — emissive adds to the existing color.
    let additive = BlendStateDescriptor {
        color: BlendComponentDescriptor {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponentDescriptor {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
    };

    PipelineSpec {
        label: "Emissive Pipeline",
        shader: "khora::pipelines::emissive",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: EMISSIVE_CAMERA_LAYOUT_LABEL,
                entries: Cow::Owned(emissive_uniform_layout_entries(
                    ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                )),
            },
            LayoutSpec::Inline {
                label: EMISSIVE_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(emissive_uniform_layout_entries(ShaderStageFlags::VERTEX)),
            },
            LayoutSpec::Inline {
                label: EMISSIVE_MATERIAL_LAYOUT_LABEL,
                entries: Cow::Owned(emissive_uniform_layout_entries(ShaderStageFlags::FRAGMENT)),
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
            depth_write_enabled: false, // overlay — depth read-only
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
            blend: Some(additive),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

impl Lane for EmissiveLane {
    fn strategy_name(&self) -> &'static str {
        "Emissive"
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
        // TODO — iterate `RenderWorld.meshes` and draw the subset whose
        // material is flagged emissive. Requires `MaterialKind::Emissive`
        // in `khora-data`; tracked as a follow-up. Until then this lane
        // is a no-op so the OverlayAgent integration is exercised.
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
    fn emissive_lane_strategy_name() {
        let lane = EmissiveLane::default();
        assert_eq!(lane.strategy_name(), "Emissive");
        assert_eq!(lane.lane_kind(), LaneKind::Render);
    }
}
