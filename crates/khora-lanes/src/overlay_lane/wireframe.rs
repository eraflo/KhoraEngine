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
//! The pipeline is composed from `khora::pipelines::wireframe` and uses a
//! barycentric edge-detection fragment shader (it does not rely on
//! `Features::POLYGON_MODE_LINE`, which is not part of wgpu's downlevel-safe
//! baseline).
//!
//! Opt-in like the grid: a host application enables the [`WireframeView`]
//! it publishes (the editor drives it from a viewport toggle; the sandbox
//! leaves it off). The lane loads the scene's colour and reads its depth,
//! drawing after the scene pass so the wireframe overlays the lit image.

use std::time::Duration;

use crate::render_lane::util::DynamicUniformRingBuffer;
use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::renderer::api::command::{
    BindGroupId, BindGroupLayoutEntry, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::pipeline::{PipelineSpec, RenderPipelineId};
use khora_core::renderer::api::resource::{
    BufferId, BufferUsage, CameraUniformData, IndexFormat, TextureViewId,
};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::{CommandEncoder, PipelineSystem};
use khora_core::renderer::GraphicsDevice;
use khora_data::render::{RenderWorld, WireframeView};
use khora_data::AssetStore;
use khora_macros::LaneInputs;

use super::block::Block;
use super::WireframeOutputs;

/// Wireframe material uniform — matches `WireframeMaterialUniforms` in
/// `wireframe.wgsl` (line color + width, padded to 16 bytes).
#[repr(C, align(16))]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WireframeMaterialUniforms {
    color: [f32; 4],
    line_width: f32,
    _padding: [f32; 3],
}

const WIREFRAME_CAMERA_LAYOUT_LABEL: &str = "wireframe_camera_layout";
const WIREFRAME_MATERIAL_LAYOUT_LABEL: &str = "wireframe_material_layout";

/// What recording and uploading one mesh's draw costs on the host.
const MESH_CPU: Duration = Duration::from_micros(2);
/// What drawing one mesh's edges costs on the device.
const MESH_GPU: Duration = Duration::from_micros(4);

/// The bytes the first build holds: the camera and material blocks, and the
/// model ring's first chunk in every frame in flight.
const BLOCK_BYTES: u64 = (std::mem::size_of::<CameraUniformData>()
    + std::mem::size_of::<WireframeMaterialUniforms>()) as u64
    + DynamicUniformRingBuffer::first_bytes(
        std::mem::size_of::<ModelUniforms>() as u32,
        crate::render_lane::util::dynamic_uniform_ring_buffer::DEFAULT_MAX_ELEMENTS,
        crate::render_lane::util::dynamic_uniform_ring_buffer::MIN_UNIFORM_ALIGNMENT,
    );

/// Wireframe debug overlay lane. Holds what it built on the device; `None`
/// before the first prepare and after shutdown.
#[derive(Debug)]
pub struct WireframeLane {
    levels: [Level; 1],
    gpu: Option<WireframeGpu>,
}

impl Default for WireframeLane {
    fn default() -> Self {
        Self {
            levels: [Level {
                id: StrategyId::Balanced,
                name: "wireframe",
                impact: Impact::None,
                memory: PerDevice {
                    cpu: 0,
                    gpu: BLOCK_BYTES,
                    npu: 0,
                    other: 0,
                },
            }],
            gpu: None,
        }
    }
}

/// What the wireframe builds on the device.
#[derive(Debug)]
struct WireframeGpu {
    camera: Block,
    material: Block,
    /// Per-mesh model transforms — one buffer, a per-draw dynamic offset,
    /// same machinery the lit lanes use.
    model_ring: DynamicUniformRingBuffer,
    /// The pipeline and the target it was built for; `None` for a target
    /// without depth, which the wireframe cannot test against.
    pipeline: Option<(TargetKey, RenderPipelineId)>,
}

impl WireframeGpu {
    /// The bytes its buffers hold.
    fn bytes(&self) -> u64 {
        self.camera.size + self.material.size + self.model_ring.bytes()
    }

    fn destroy(self, device: &dyn GraphicsDevice) {
        self.camera.destroy(device);
        self.material.destroy(device);
        self.model_ring.destroy(device);
    }
}

/// One mesh's draw: its model offset and bind group, and its buffers.
struct Draw {
    offset: u32,
    model: BindGroupId,
    vertex: BufferId,
    index: BufferId,
    format: IndexFormat,
    count: u32,
}

/// What the wireframe lane reads each frame: whether the host shows the
/// wireframe and how, the meshes and the camera, the target, and the meshes
/// on the GPU.
#[derive(LaneInputs)]
pub struct WireframeInputs<'f> {
    /// The host's wireframe switch, colour and width.
    #[bus]
    pub wireframe: &'f WireframeView,
    /// The frame's meshes and views; the first view is the camera.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour and depth attachments.
    #[frame]
    pub target: Target,
    /// The store holding the meshes on the GPU.
    #[resource]
    pub assets: &'f AssetStore,
}

impl Lane for WireframeLane {
    type Family = super::WireframeFamily;
    type Inputs<'f> = WireframeInputs<'f>;

    fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// One draw per mesh on the GPU: its time on each device. A wireframe
    /// that draws nothing — off, without a camera or without depth — costs
    /// no time.
    fn cost(&self, _level: usize, inputs: &WireframeInputs<'_>) -> Cost {
        let draws = inputs.wireframe.0.enabled
            && !inputs.render_world.views.is_empty()
            && inputs.target.depth.is_some();
        let store = inputs.assets.store::<GpuMesh>();
        let uploaded = store.read().map_or(0, |meshes| {
            inputs
                .render_world
                .meshes
                .iter()
                .filter(|mesh| meshes.get(&mesh.cpu_mesh_uuid).is_some())
                .count()
        }) as u32
            * u32::from(draws);
        Cost {
            time: PerDevice {
                cpu: MESH_CPU * uploaded,
                gpu: MESH_GPU * uploaded,
                ..PerDevice::default()
            },
            memory: self.levels[0].memory,
            ..Cost::default()
        }
    }

    /// Builds the camera and material blocks and the model ring once, and
    /// the pipeline for the target's formats: a new target builds a new
    /// pipeline, a target without depth none.
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
        let built = match &mut self.gpu {
            Some(built) => built,
            empty => empty.insert(
                build_wireframe_gpu(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        self.levels[0].memory.gpu = built.bytes();
        let wanted = key.filter(|key| key.depth.is_some());
        if built.pipeline.map(|(built_for, _)| built_for) != wanted {
            built.pipeline = match wanted {
                Some(key) => Some((
                    key,
                    pipelines
                        .pipeline(device, &wireframe_pipeline_spec(key))
                        .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
                )),
                None => None,
            };
        }
        Ok(())
    }

    /// Draws the edges of every mesh already on the GPU when the host enabled
    /// the wireframe, the world has a camera and the target has depth;
    /// otherwise the pass stays empty.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut WireframeInputs<'_>,
        out: &mut WireframeOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let built = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let done = LaneReport {
            done: 1.0,
            stalled: false,
        };
        let config = &inputs.wireframe.0;
        if !config.enabled {
            return Ok(done);
        }
        let Some(view) = inputs.render_world.views.first() else {
            return Ok(done);
        };
        let (Some(depth), Some((_, pipeline))) = (inputs.target.depth, built.pipeline) else {
            return Ok(done);
        };

        let device = out.pass.device();
        let failed = |error| LaneError::ExecutionFailed(Box::new(error));
        built.camera.write_camera(device, view).map_err(failed)?;
        let material = WireframeMaterialUniforms {
            color: [
                config.line_color.r,
                config.line_color.g,
                config.line_color.b,
                config.line_color.a,
            ],
            line_width: config.line_width,
            _padding: [0.0; 3],
        };
        device
            .write_buffer(built.material.buffer, 0, bytemuck::bytes_of(&material))
            .map_err(failed)?;

        let store = inputs.assets.store::<GpuMesh>();
        let meshes = store.read().map_err(|_| LaneError::LockPoisoned {
            context: "the GPU meshes",
        })?;
        built.model_ring.advance();
        let draws = push_models(built, device, inputs.render_world, &meshes);
        drop(meshes);
        // The ring may have grown before a push was refused: the level
        // quotes what it holds either way.
        self.levels[0].memory.gpu = built.bytes();
        let draws = draws.map_err(failed)?;
        if draws.is_empty() {
            return Ok(done);
        }
        draw_wireframe(
            out.pass.encoder(),
            inputs.target.color,
            depth,
            pipeline,
            built,
            &draws,
        );
        Ok(done)
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(built), Some(device)) = (self.gpu.take(), gpu.device()) {
            built.destroy(device);
        }
        self.levels[0].memory.gpu = BLOCK_BYTES;
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// Pushes the model of every mesh on the GPU into the ring, in the world's
/// order, and returns their draws.
fn push_models(
    built: &mut WireframeGpu,
    device: &dyn GraphicsDevice,
    render_world: &RenderWorld,
    meshes: &khora_data::assets::Assets<GpuMesh>,
) -> Result<Vec<Draw>, khora_core::renderer::error::ResourceError> {
    let mut draws = Vec::with_capacity(render_world.meshes.len());
    for extracted in &render_world.meshes {
        let Some(gpu_mesh) = meshes.get(&extracted.cpu_mesh_uuid) else {
            continue;
        };
        let model = extracted.transform.to_matrix();
        let normal = model.inverse().map(|inv| inv.transpose()).unwrap_or(model);
        let uniforms = ModelUniforms {
            model_matrix: model.to_cols_array_2d(),
            normal_matrix: normal.to_cols_array_2d(),
        };
        let offset = built
            .model_ring
            .push(device, bytemuck::bytes_of(&uniforms))?;
        draws.push(Draw {
            offset,
            // The chunk the push landed in: a ring that grew mid-frame binds
            // the earlier draws through their own chunk.
            model: *built.model_ring.current_bind_group(),
            vertex: gpu_mesh.vertex_buffer,
            index: gpu_mesh.index_buffer,
            format: gpu_mesh.index_format,
            count: gpu_mesh.index_count,
        });
    }
    Ok(draws)
}

/// The camera and material blocks and the model ring. A refusal halfway
/// gives back what came before it.
fn build_wireframe_gpu(
    device: &dyn GraphicsDevice,
    pipelines: &dyn PipelineSystem,
) -> Result<WireframeGpu, RenderError> {
    use crate::render_lane::util::dynamic_uniform_ring_buffer::{
        DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
    };
    use khora_core::renderer::api::pipeline::LayoutKey;
    use khora_core::renderer::api::shader::ShaderVariantKey;
    use khora_core::renderer::api::util::ShaderStageFlags;

    // Group 0 (camera) + group 2 (material) are single bespoke uniforms;
    // group 1 (per-mesh model) reuses the canonical dynamic Model layout so
    // the ring matches the pipeline byte-for-byte. Layouts belong to the
    // PipelineSystem, which caches them.
    let camera_layout = pipelines.inline_layout(
        device,
        WIREFRAME_CAMERA_LAYOUT_LABEL,
        &uniform_layout_entries(ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT),
    )?;
    let material_layout = pipelines.inline_layout(
        device,
        WIREFRAME_MATERIAL_LAYOUT_LABEL,
        &uniform_layout_entries(ShaderStageFlags::FRAGMENT),
    )?;
    let model_layout = pipelines.layout(device, LayoutKey::Model, &ShaderVariantKey::empty())?;

    let camera = Block::camera(device, camera_layout, "wireframe_camera")
        .map_err(RenderError::ResourceError)?;
    let material = match Block::new(
        device,
        material_layout,
        std::mem::size_of::<WireframeMaterialUniforms>() as u64,
        BufferUsage::UNIFORM,
        "wireframe_material",
    ) {
        Ok(material) => material,
        Err(error) => {
            camera.destroy(device);
            return Err(RenderError::ResourceError(error));
        }
    };
    let model_ring = match DynamicUniformRingBuffer::new(
        device,
        model_layout,
        0,
        std::mem::size_of::<ModelUniforms>() as u32,
        DEFAULT_MAX_ELEMENTS,
        MIN_UNIFORM_ALIGNMENT,
        "Wireframe Model Ring",
    ) {
        Ok(ring) => ring,
        Err(error) => {
            material.destroy(device);
            camera.destroy(device);
            return Err(RenderError::ResourceError(error));
        }
    };
    Ok(WireframeGpu {
        camera,
        material,
        model_ring,
        pipeline: None,
    })
}

/// A layout of one uniform buffer at binding 0, seen by `visibility`.
fn uniform_layout_entries(
    visibility: khora_core::renderer::api::util::ShaderStageFlags,
) -> Vec<BindGroupLayoutEntry> {
    crate::render_lane::util::single_uniform_layout(visibility, false)
}

/// The declarative pipeline spec for the wireframe on a target of `key`'s
/// formats — the mesh vertex layout, alpha blended, depth-tested
/// (LessEqual) against the scene, never writing depth.
fn wireframe_pipeline_spec(key: TargetKey) -> PipelineSpec {
    use khora_core::renderer::api::gpu_scene::Mesh;
    use khora_core::renderer::api::pipeline::enums::CompareFunction;
    use khora_core::renderer::api::pipeline::state::{
        BlendStateDescriptor, ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutKey, LayoutSpec,
        MultisampleStateDescriptor, PrimitiveStateDescriptor,
    };
    use khora_core::renderer::api::shader::ShaderVariantKey;
    use khora_core::renderer::api::util::ShaderStageFlags;
    use std::borrow::Cow;

    PipelineSpec {
        label: "Wireframe Pipeline",
        shader: "khora::pipelines::wireframe",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: WIREFRAME_CAMERA_LAYOUT_LABEL,
                entries: Cow::Owned(uniform_layout_entries(
                    ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                )),
            },
            LayoutSpec::Named(LayoutKey::Model),
            LayoutSpec::Inline {
                label: WIREFRAME_MATERIAL_LAYOUT_LABEL,
                entries: Cow::Owned(uniform_layout_entries(ShaderStageFlags::FRAGMENT)),
            },
        ],
        vertex_buffers: vec![Mesh::vertex_buffer_layout()],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: key.depth.map(|format| DepthStencilStateDescriptor {
            format,
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
            format: key.color,
            blend: Some(BlendStateDescriptor::alpha_blending()),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: crate::render_lane::util::sample_count(key.samples),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// The wireframe pass — `LoadOp::Load` preserves the scene's colour and
/// depth.
fn draw_wireframe(
    encoder: &mut dyn CommandEncoder,
    color: TextureViewId,
    depth: TextureViewId,
    pipeline: RenderPipelineId,
    built: &WireframeGpu,
    draws: &[Draw],
) {
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
        label: Some("Wireframe Overlay Pass"),
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
    pass.set_bind_group(0, &built.camera.bind_group, &[]);
    pass.set_bind_group(2, &built.material.bind_group, &[]);
    for draw in draws {
        pass.set_bind_group(1, &draw.model, &[draw.offset]);
        pass.set_vertex_buffer(0, &draw.vertex, 0);
        pass.set_index_buffer(&draw.index, 0, draw.format);
        pass.draw_indexed(0..draw.count, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wireframe_material_uniforms_is_std140_sized() {
        assert_eq!(std::mem::size_of::<WireframeMaterialUniforms>(), 32);
        assert_eq!(std::mem::align_of::<WireframeMaterialUniforms>(), 16);
    }
}
