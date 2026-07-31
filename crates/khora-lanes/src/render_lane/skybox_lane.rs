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

//! Skybox lane — draws the IBL environment cube as the scene background.
//!
//! Renders `khora::pipelines::skybox` (a fullscreen triangle pinned to the far
//! plane) into the main color target, **after** the scene pass. It is
//! depth-tested (`LessEqual`) against the scene's depth buffer with depth-write
//! disabled, so it only paints the pixels the geometry left at the far plane —
//! the sky shows through the background, the meshes are untouched. The visible
//! sky is therefore exactly the environment the lit surfaces reflect.
//!
//! Registered under [`SkyboxAgent`](khora_agents::skybox_agent), which runs in
//! the OUTPUT phase after `RenderAgent` and buffers this lane's pass into the
//! FrameGraph (`writes(Color).reads(Depth)`), mirroring how `OverlayAgent`
//! contributes the grid/gizmo overlays. `LoadOp::Load` preserves what the scene
//! drew.
//!
//! Per CLAD this struct holds only persistent state; init / render bodies are
//! private free functions in this module.

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::render::RenderWorld;
use std::sync::{Arc, OnceLock};

/// Stable cache label for the group-0 (inverse-VP + camera) layout.
const SKY_UNIFORM_LAYOUT_LABEL: &str = "skybox_uniform_layout";
/// Stable cache label for the group-1 (env cube + sampler) layout.
const SKY_ENV_LAYOUT_LABEL: &str = "skybox_env_layout";

/// Skybox uniform block — matches `SkyUniforms` in `skybox.wgsl`. The inverse
/// view-projection lets the fragment recover each pixel's world-space view ray;
/// `camera_pos` is the ray origin.
#[repr(C, align(16))]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniforms {
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
}

/// Environment-background lane.
///
/// Holds the pipeline, the two bespoke layouts, the per-frame uniform buffer +
/// its bind group (group 0), and — created lazily once the IBL bake is ready —
/// the static env-cube bind group (group 1). The env cube is baked once and
/// never changes, so its bind group is built a single time and cached.
#[derive(Debug, Default)]
pub struct SkyboxLane {
    pipeline: OnceLock<RenderPipelineId>,
    sky_layout: OnceLock<BindGroupLayoutId>,
    env_layout: OnceLock<BindGroupLayoutId>,
    sky_buffer: OnceLock<BufferId>,
    sky_bind_group: OnceLock<BindGroupId>,
    env_bind_group: OnceLock<BindGroupId>,
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

fn init_gpu_resources(
    lane: &SkyboxLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding,
    };
    use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage};
    use std::borrow::Cow;

    let sky_layout = pipeline_system.inline_layout(
        device,
        SKY_UNIFORM_LAYOUT_LABEL,
        &sky_uniform_layout_entries(),
    )?;
    let env_layout =
        pipeline_system.inline_layout(device, SKY_ENV_LAYOUT_LABEL, &env_layout_entries())?;

    let sky_buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("skybox_uniform_ubo")),
            size: std::mem::size_of::<SkyUniforms>() as u64,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let sky_bind_group = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("skybox_uniform_bg"),
            layout: sky_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: sky_buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        })
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let pipeline_id = pipeline_system.pipeline(device, &skybox_pipeline_spec(device))?;

    let _ = lane.sky_layout.set(sky_layout);
    let _ = lane.env_layout.set(env_layout);
    let _ = lane.sky_buffer.set(sky_buffer);
    let _ = lane.sky_bind_group.set(sky_bind_group);
    let _ = lane.pipeline.set(pipeline_id);
    Ok(())
}

/// Group-0 layout: a single uniform buffer (fragment-only — the vertex shader
/// derives clip positions from `vertex_index`).
fn sky_uniform_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Group-1 layout: environment cube at 0, filtering sampler at 1.
fn env_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, SamplerBindingType, TextureSampleType,
        TextureViewDimension,
    };
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::Cube,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
    ]
}

/// The declarative pipeline spec for the skybox — fullscreen triangle, no
/// vertex buffer, depth-tested `LessEqual` with **no depth write** so it sits
/// behind every mesh.
fn skybox_pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
    use khora_core::renderer::api::pipeline::state::{
        ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
        MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor, ShaderVariantKey,
    };
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    PipelineSpec {
        label: "Skybox Pipeline",
        shader: "khora::pipelines::skybox",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: SKY_UNIFORM_LAYOUT_LABEL,
                entries: Cow::Owned(sky_uniform_layout_entries()),
            },
            LayoutSpec::Inline {
                label: SKY_ENV_LAYOUT_LABEL,
                entries: Cow::Owned(env_layout_entries()),
            },
        ],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        // Depth-tested against the scene buffer (LessEqual); the sky is emitted
        // at the far plane (clip z = 1) and writes NO depth, so it fills only
        // the background pixels the geometry left untouched.
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

/// Returns the cached env-cube bind group, building it on first use once the
/// IBL bake has published its bindings. `None` only if the bind group could not
/// be created.
fn env_bind_group(
    lane: &SkyboxLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    ibl: &IblGpuBindings,
) -> Option<BindGroupId> {
    if let Some(bg) = lane.env_bind_group.get().copied() {
        return Some(bg);
    }
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupEntry, BindingResource,
    };
    let layout = lane.env_layout.get().copied()?;
    let bg = device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("skybox_env_bg"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(ibl.env_cube),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(ibl.sampler),
                    _phantom: std::marker::PhantomData,
                },
            ],
        })
        .map_err(|e| log::error!("SkyboxLane: env bind group creation failed: {e:?}"))
        .ok()?;
    let _ = lane.env_bind_group.set(bg);
    Some(bg)
}

#[allow(clippy::too_many_arguments)]
fn render_skybox(
    lane: &SkyboxLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    encoder: &mut dyn CommandEncoder,
    color_target: khora_core::renderer::api::resource::TextureViewId,
    depth_target: khora_core::renderer::api::resource::TextureViewId,
    view: &khora_data::render::ExtractedView,
    ibl: &IblGpuBindings,
) {
    use khora_core::renderer::api::command::{
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
        RenderPassDescriptor, StoreOp,
    };

    let (Some(pipeline), Some(sky_buffer), Some(sky_bg)) = (
        lane.pipeline.get().copied(),
        lane.sky_buffer.get().copied(),
        lane.sky_bind_group.get().copied(),
    ) else {
        log::warn!("SkyboxLane: GPU resources not initialized, skipping");
        return;
    };
    let Some(env_bg) = env_bind_group(lane, device, ibl) else {
        return;
    };

    // The fragment reconstructs its world ray from the inverse view-projection;
    // no inverse ⇒ a degenerate camera, skip this frame.
    let Some(inv_view_proj) = view.view_proj.inverse() else {
        return;
    };
    let uniforms = SkyUniforms {
        inv_view_proj: inv_view_proj.to_cols_array_2d(),
        camera_pos: [view.position.x, view.position.y, view.position.z, 1.0],
    };
    if let Err(e) = device.write_buffer(sky_buffer, 0, bytemuck::bytes_of(&uniforms)) {
        log::error!("SkyboxLane: uniform buffer write failed: {:?}", e);
        return;
    }

    // Background pass — `LoadOp::Load` preserves the scene's color + depth; the
    // depth buffer is loaded (not cleared) so the LessEqual test rejects pixels
    // the geometry already owns. Depth is not written.
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
        label: Some("Skybox Pass"),
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
    pass.set_bind_group(0, &sky_bg, &[]);
    pass.set_bind_group(1, &env_bg, &[]);
    // Fullscreen triangle — the vertex shader derives positions from
    // `vertex_index` (no vertex buffer bound).
    pass.draw(0..3, 0..1);
}

impl Lane for SkyboxLane {
    fn strategy_name(&self) -> &'static str {
        "Skybox"
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
        // The environment cube is published by the IBL bake once it has run.
        // Absent ⇒ nothing to draw yet (the scene's clear color shows through).
        let Some(ibl) = ctx.get::<IblGpuBindings>().copied() else {
            return Ok(());
        };

        // Camera comes from the primary extracted view (the editor viewport
        // override is folded into `RenderWorld.views` by `RenderFlow`).
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
        // The skybox depth-tests against the scene buffer — no depth target ⇒
        // skip (it would otherwise overwrite the geometry).
        let Some(depth_target) = ctx.get::<khora_core::lane::DepthTarget>().map(|d| d.0) else {
            return Ok(());
        };

        render_skybox(
            self,
            device.as_ref(),
            encoder,
            color_target,
            depth_target,
            &view,
            &ibl,
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
    fn skybox_lane_strategy_name() {
        let lane = SkyboxLane::default();
        assert_eq!(lane.strategy_name(), "Skybox");
        assert_eq!(lane.lane_kind(), LaneKind::Render);
    }

    #[test]
    fn sky_uniforms_are_std140_sized() {
        // mat4 (64) + vec4 (16) = 80 bytes, 16-aligned.
        assert_eq!(std::mem::size_of::<SkyUniforms>(), 80);
        assert_eq!(std::mem::align_of::<SkyUniforms>(), 16);
    }
}
