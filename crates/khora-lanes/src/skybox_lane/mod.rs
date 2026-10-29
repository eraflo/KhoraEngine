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
//! plane) into the frame's colour target. It is depth-tested (`LessEqual`)
//! against the scene's depth buffer with depth-write disabled, so it paints
//! only the pixels **the opaque geometry left at the far plane**. The visible
//! sky is therefore exactly the environment the lit surfaces reflect.
//!
//! # Where it sits, and why that word matters
//!
//! Between the scene's two passes: after the opaque draws that give it a depth
//! buffer to test against, and **before** the blended ones.
//!
//! A blended surface must not write depth — two of them could not composite
//! if it did — so it leaves the pixel at the far plane, and a sky drawn after
//! it would repaint it: a glass sphere against the sky would vanish while the
//! half of it overlapping the floor survived. The scene is therefore two
//! contributions ([`ScenePassSlot`](khora_data::render::ScenePassSlot) and
//! [`TransparentPassSlot`](khora_data::render::TransparentPassSlot)) and this
//! pass is folded between them.
//!
//! The family registers in the OUTPUT phase, after the renderer; its pass
//! slot writes the colour and reads the depth, and `LoadOp::Load` preserves
//! what the opaque pass drew. The pipeline is built for the target's formats,
//! so the editor's viewport and the window each get the one they need.

mod skybox_family;

pub use skybox_family::*;

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::{BufferId, TextureViewId};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{CommandEncoder, PipelineSystem};
use khora_core::renderer::GraphicsDevice;
use khora_data::render::RenderWorld;
use khora_data::IblBaker;
use khora_macros::LaneInputs;

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

/// The lane's one level: the uniform block is the memory it holds.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "skybox",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: std::mem::size_of::<SkyUniforms>() as u64,
        npu: 0,
        other: 0,
    },
}];

/// What recording the one triangle costs on the host, whatever the target.
const RECORD_CPU: Duration = Duration::from_micros(20);
/// What painting one pixel of sky costs on the device.
const PIXEL_GPU_SECS: f64 = 5e-11;

/// Environment-background lane. Holds what it built on the device; `None`
/// before the first prepare and after shutdown.
#[derive(Debug, Default)]
pub struct SkyboxLane {
    gpu: Option<SkyGpu>,
}

/// What the skybox builds on the device.
#[derive(Debug)]
struct SkyGpu {
    /// The group-1 layout the env bind group is made with.
    env_layout: BindGroupLayoutId,
    /// The uniform block.
    sky_buffer: BufferId,
    /// The group-0 bind group over the uniform block.
    sky_bind_group: BindGroupId,
    /// The pipeline and the target it was built for; `None` for a target
    /// without depth, which the sky cannot test against.
    pipeline: Option<(TargetKey, RenderPipelineId)>,
    /// The env-cube bind group and the cube view it binds.
    env_bind_group: Option<(TextureViewId, BindGroupId)>,
}

/// What the skybox lane reads each frame: the camera, the target it paints
/// and the environment the IBL bake published.
#[derive(LaneInputs)]
pub struct SkyboxInputs<'f> {
    /// The frame's views; the first is the camera the sky is seen from.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour and depth attachments.
    #[frame]
    pub target: Target,
    /// The bake whose environment cube the sky samples.
    #[resource]
    pub ibl: &'f IblBaker,
}

impl Lane for SkyboxLane {
    type Family = SkyboxFamily;
    type Inputs<'f> = SkyboxInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// One triangle on the host; on the device, every pixel of the target is
    /// tested and the sky's share painted.
    fn cost(&self, _level: usize, inputs: &SkyboxInputs<'_>) -> Cost {
        let size = inputs.target.size;
        let pixels = f64::from(size.width) * f64::from(size.height);
        Cost {
            time: PerDevice {
                cpu: RECORD_CPU,
                gpu: Duration::from_secs_f64(pixels * PIXEL_GPU_SECS),
                ..PerDevice::default()
            },
            memory: LEVELS[0].memory,
            ..Cost::default()
        }
    }

    /// Builds the uniform block once, and the pipeline for the target's
    /// formats: a new target builds a new pipeline, a target without depth
    /// none.
    fn prepare(
        &mut self,
        _level: usize,
        gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        let device = gpu.device().ok_or(LaneError::missing("GraphicsDevice"))?;
        let pipelines = gpu
            .pipelines()
            .ok_or(LaneError::missing("PipelineSystem"))?;
        let sky = match &mut self.gpu {
            Some(sky) => sky,
            empty => empty.insert(
                build_sky_gpu(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        let wanted = key.filter(|key| key.depth.is_some());
        if sky.pipeline.map(|(built_for, _)| built_for) != wanted {
            sky.pipeline = match wanted {
                Some(key) => {
                    let pipeline = pipelines
                        .pipeline(device, &skybox_pipeline_spec(key))
                        .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
                    Some((key, pipeline))
                }
                None => None,
            };
        }
        Ok(())
    }

    /// Paints the sky when the bake has published its cube, the world has a
    /// camera whose view-projection inverts, and the target has depth; any
    /// of them missing leaves the pass empty.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut SkyboxInputs<'_>,
        out: &mut SkyboxOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let sky = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let done = LaneReport {
            done: 1.0,
            stalled: false,
        };
        let Some(ibl) = inputs.ibl.bindings() else {
            return Ok(done);
        };
        let Some(view) = inputs.render_world.views.first() else {
            return Ok(done);
        };
        let (Some(depth), Some((_, pipeline))) = (inputs.target.depth, sky.pipeline) else {
            return Ok(done);
        };
        // The fragment reconstructs its world ray from the inverse
        // view-projection: a camera without one sees no sky.
        let Some(inv_view_proj) = view.view_proj.inverse() else {
            return Ok(done);
        };

        let device = out.pass.device();
        let env_bind_group = env_bind_group(sky, device, &ibl)
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        let uniforms = SkyUniforms {
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            camera_pos: [view.position.x, view.position.y, view.position.z, 1.0],
        };
        device
            .write_buffer(sky.sky_buffer, 0, bytemuck::bytes_of(&uniforms))
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        draw_sky(
            out.pass.encoder(),
            inputs.target.color,
            depth,
            pipeline,
            sky.sky_bind_group,
            env_bind_group,
        );
        Ok(done)
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(sky), Some(device)) = (self.gpu.take(), gpu.device()) {
            destroy_sky_gpu(sky, device);
        }
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// Builds the layouts, the uniform block and its bind group. A refused bind
/// group gives the buffer back.
fn build_sky_gpu(
    device: &dyn GraphicsDevice,
    pipeline_system: &dyn PipelineSystem,
) -> Result<SkyGpu, RenderError> {
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding,
    };
    use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage};
    use std::borrow::Cow;

    // Layouts belong to the PipelineSystem, which caches them.
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
        .map_err(RenderError::ResourceError)?;
    let sky_bind_group = device.create_bind_group(&BindGroupDescriptor {
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
    });
    match sky_bind_group {
        Ok(sky_bind_group) => Ok(SkyGpu {
            env_layout,
            sky_buffer,
            sky_bind_group,
            pipeline: None,
            env_bind_group: None,
        }),
        Err(error) => {
            if let Err(e) = device.destroy_buffer(sky_buffer) {
                log::warn!("SkyboxLane: could not destroy the uniform buffer: {e:?}");
            }
            Err(RenderError::ResourceError(error))
        }
    }
}

/// Gives back what the lane built. The pipeline and the layouts are the
/// `PipelineSystem`'s, so they stay.
fn destroy_sky_gpu(sky: SkyGpu, device: &dyn GraphicsDevice) {
    if let Some((_, group)) = sky.env_bind_group {
        if let Err(e) = device.destroy_bind_group(group) {
            log::warn!("SkyboxLane: could not destroy the environment bind group: {e:?}");
        }
    }
    if let Err(e) = device.destroy_bind_group(sky.sky_bind_group) {
        log::warn!("SkyboxLane: could not destroy the uniform bind group: {e:?}");
    }
    if let Err(e) = device.destroy_buffer(sky.sky_buffer) {
        log::warn!("SkyboxLane: could not destroy the uniform buffer: {e:?}");
    }
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

/// The declarative pipeline spec for the skybox on a target of `key`'s
/// formats — fullscreen triangle, no vertex buffer, depth-tested `LessEqual`
/// with **no depth write** so it sits behind every mesh.
fn skybox_pipeline_spec(key: TargetKey) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
    use khora_core::renderer::api::pipeline::state::{
        ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
        MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor,
    };
    use khora_core::renderer::api::shader::ShaderVariantKey;
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
        depth_stencil: key.depth.map(|format| DepthStencilStateDescriptor {
            format,
            depth_write_enabled: false,
            depth_compare: CompareFunction::LessEqual,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState::default(),
        }),
        color_targets: vec![ColorTargetStateDescriptor {
            format: key.color,
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: crate::render_lane::util::sample_count(key.samples),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// The env-cube bind group for `ibl`'s env cube: the cached one while the
/// cube is the same, else a new one (the previous is destroyed — its cube
/// belongs to a bake that was replaced).
fn env_bind_group(
    sky: &mut SkyGpu,
    device: &dyn GraphicsDevice,
    ibl: &IblGpuBindings,
) -> Result<BindGroupId, ResourceError> {
    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupEntry, BindingResource,
    };
    if let Some((cube, group)) = sky.env_bind_group {
        if cube == ibl.env_cube {
            return Ok(group);
        }
    }
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("skybox_env_bg"),
        layout: sky.env_layout,
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
    })?;
    if let Some((_, stale)) = sky.env_bind_group.replace((ibl.env_cube, group)) {
        if let Err(e) = device.destroy_bind_group(stale) {
            log::warn!("SkyboxLane: destroying the previous env bind group failed: {e:?}");
        }
    }
    Ok(group)
}

/// The background pass — `LoadOp::Load` preserves the scene's colour and
/// depth; the depth buffer is loaded (not cleared) so the `LessEqual` test
/// rejects pixels the geometry already owns. Depth is not written.
fn draw_sky(
    encoder: &mut dyn CommandEncoder,
    color: TextureViewId,
    depth: TextureViewId,
    pipeline: RenderPipelineId,
    sky_bind_group: BindGroupId,
    env_bind_group: BindGroupId,
) {
    use khora_core::renderer::api::command::{
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
        RenderPassDescriptor, StoreOp,
    };

    let color_attachment = RenderPassColorAttachment {
        view: &color,
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
            view: &depth,
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
    pass.set_bind_group(0, &sky_bind_group, &[]);
    pass.set_bind_group(1, &env_bind_group, &[]);
    // Fullscreen triangle — the vertex shader derives positions from
    // `vertex_index` (no vertex buffer bound).
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_uniforms_are_std140_sized() {
        // mat4 (64) + vec4 (16) = 80 bytes, 16-aligned.
        assert_eq!(std::mem::size_of::<SkyUniforms>(), 80);
        assert_eq!(std::mem::align_of::<SkyUniforms>(), 16);
    }
}
