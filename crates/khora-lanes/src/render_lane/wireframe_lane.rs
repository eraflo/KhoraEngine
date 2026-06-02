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
//! **Status — Phase 3 scaffold.** The pipeline is created at init via
//! the ShaderRegistry; `execute()` is a no-op until a debug flag /
//! filter exists in the engine context to gate the pass. Adding that
//! flag is a follow-up — the lane is in place so wiring + the agent
//! contract are exercised.

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
    shader_registry: &std::sync::Arc<std::sync::Mutex<crate::render_lane::ShaderRegistry>>,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::{
        command::{
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
        },
        pipeline::enums::{
            BlendFactor, BlendOperation, CompareFunction, VertexFormat, VertexStepMode,
        },
        pipeline::state::{
            BlendComponentDescriptor, BlendStateDescriptor, ColorWrites, DepthBiasState,
            StencilFaceState,
        },
        pipeline::{
            ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
            PrimitiveStateDescriptor, RenderPipelineDescriptor, VertexAttributeDescriptor,
            VertexBufferLayoutDescriptor,
        },
        util::{SampleCount, ShaderStageFlags, TextureFormat},
    };
    use std::borrow::Cow;

    let camera_layout = device
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("wireframe_camera_layout"),
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
            label: Some("wireframe_model_layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStageFlags::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            }],
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;
    let material_layout = device
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("wireframe_material_layout"),
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

    let shader_module = {
        let mut registry = crate::lock_or_log!(
            shader_registry.lock(),
            "WireframeLane on_gpu_init.shader_registry",
            Err(khora_core::renderer::error::RenderError::ResourceError(
                khora_core::renderer::ResourceError::BackendError(
                    "shader_registry mutex poisoned".to_owned()
                )
            ))
        );
        registry
            .create_module(
                device,
                "khora::pipelines::wireframe",
                Some("wireframe_shader"),
            )
            .map_err(|e| {
                khora_core::renderer::error::RenderError::ResourceError(
                    khora_core::renderer::ResourceError::BackendError(format!(
                        "ShaderRegistry compose failed: {}",
                        e
                    )),
                )
            })?
    };

    let pipeline_layout_ids = vec![camera_layout, model_layout, material_layout];
    let pipeline_layout_id = device
        .create_pipeline_layout(
            &khora_core::renderer::api::pipeline::PipelineLayoutDescriptor {
                label: Some(Cow::Borrowed("Wireframe Pipeline Layout")),
                bind_group_layouts: &pipeline_layout_ids,
            },
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

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
        array_stride: 32,
        step_mode: VertexStepMode::Vertex,
        attributes: Cow::Owned(vertex_attributes),
    };

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

    let pipeline_desc = RenderPipelineDescriptor {
        label: Some(Cow::Borrowed("Wireframe Pipeline")),
        layout: Some(pipeline_layout_id),
        vertex_shader_module: shader_module,
        vertex_entry_point: Cow::Borrowed("vs_main"),
        fragment_shader_module: Some(shader_module),
        fragment_entry_point: Some(Cow::Borrowed("fs_main")),
        vertex_buffers_layout: Cow::Owned(vec![vertex_layout]),
        primitive_state: PrimitiveStateDescriptor::default(),
        depth_stencil_state: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: false,
            depth_compare: CompareFunction::LessEqual,
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
            blend: Some(blend),
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

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.model_layout.set(model_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
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
        let registry = ctx
            .get::<std::sync::Arc<std::sync::Mutex<crate::render_lane::ShaderRegistry>>>()
            .ok_or(LaneError::missing("Arc<Mutex<ShaderRegistry>>"))?
            .clone();
        init_gpu_resources(self, device.as_ref(), &registry)
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
