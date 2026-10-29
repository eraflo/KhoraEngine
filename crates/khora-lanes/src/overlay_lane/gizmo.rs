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
//! Reads the [`GizmoView`] the host publishes and draws each
//! `GizmoLineInstance` as an alpha-blended line list on top of the frame's
//! colour, with no depth test: gizmos are always on top. The host
//! application (editor, debug tooling) is the sole producer of the lines;
//! the engine only provides the mechanism — no `EditorAgent` in the engine,
//! per CLAD.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::renderer::api::command::{
    BindGroupLayoutEntry, BindGroupLayoutId, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::{PipelineSpec, RenderPipelineId};
use khora_core::renderer::api::resource::{
    BufferUsage, CameraUniformData, TextureFormat, TextureViewId,
};
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::editor::GizmoLineInstance;
use khora_data::render::{GizmoView, RenderWorld};
use khora_macros::LaneInputs;

use super::block::Block;
use super::GizmoOutputs;

/// Line instances the storage buffer holds before it first grows.
const GIZMO_CAPACITY: usize = 4096;

/// Stable cache labels for the gizmo lane's bespoke layouts.
const GIZMO_CAMERA_LAYOUT_LABEL: &str = "gizmo_camera_layout";
const GIZMO_STORAGE_LAYOUT_LABEL: &str = "gizmo_storage_layout";

/// What recording and uploading one line costs on the host.
const LINE_CPU: Duration = Duration::from_nanos(40);
/// What drawing one line costs on the device.
const LINE_GPU: Duration = Duration::from_nanos(20);

/// The bytes the camera block and the first storage buffer hold.
const START_BYTES: u64 = (std::mem::size_of::<CameraUniformData>()
    + GIZMO_CAPACITY * std::mem::size_of::<GizmoLineInstance>()) as u64;

/// Editor / debug gizmo overlay lane. Holds what it built on the device;
/// `None` before the first prepare and after shutdown.
#[derive(Debug)]
pub struct GizmoLane {
    levels: [Level; 1],
    gpu: Option<GizmoGpu>,
}

impl Default for GizmoLane {
    fn default() -> Self {
        Self {
            levels: [Level {
                id: StrategyId::Balanced,
                name: "gizmos",
                impact: Impact::None,
                memory: PerDevice {
                    cpu: 0,
                    gpu: START_BYTES,
                    npu: 0,
                    other: 0,
                },
            }],
            gpu: None,
        }
    }
}

/// What the gizmos build on the device.
#[derive(Debug)]
struct GizmoGpu {
    camera: Block,
    /// The line storage, grown when a frame brings more lines than it holds.
    storage: Block,
    storage_layout: BindGroupLayoutId,
    /// The pipeline and the colour format and samples it was built for.
    pipeline: Option<((TextureFormat, u32), RenderPipelineId)>,
}

impl GizmoGpu {
    /// The bytes its buffers hold.
    fn bytes(&self) -> u64 {
        self.camera.size + self.storage.size
    }
}

/// What the gizmo lane reads each frame: the host's lines, the camera they
/// are seen from, and the target they are drawn into.
#[derive(LaneInputs)]
pub struct GizmoInputs<'f> {
    /// The lines the host publishes this frame.
    #[bus]
    pub gizmos: &'f GizmoView,
    /// The frame's views; the first is the camera the gizmos are seen from.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour attachment; the gizmos never test depth.
    #[frame]
    pub target: Target,
}

impl Lane for GizmoLane {
    type Family = super::GizmoFamily;
    type Inputs<'f> = GizmoInputs<'f>;

    fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Every line is uploaded and drawn: its time on each device. Without a
    /// camera nothing is drawn, and nothing costs time.
    fn cost(&self, _level: usize, inputs: &GizmoInputs<'_>) -> Cost {
        let draws = !inputs.render_world.views.is_empty();
        let lines = inputs.gizmos.0.lines.len() as u32 * u32::from(draws);
        Cost {
            time: PerDevice {
                cpu: LINE_CPU * lines,
                gpu: LINE_GPU * lines,
                ..PerDevice::default()
            },
            memory: self.levels[0].memory,
            ..Cost::default()
        }
    }

    /// Builds the camera block and the line storage once, and the pipeline
    /// for the target's colour format and samples: the gizmos test no depth,
    /// so a depth change builds nothing.
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
                build_gizmo_gpu(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        self.levels[0].memory.gpu = built.bytes();
        let wanted = key.map(|key| (key.color, key.samples));
        if built.pipeline.map(|(built_for, _)| built_for) != wanted {
            built.pipeline = match wanted {
                Some(formats) => Some((
                    formats,
                    pipelines
                        .pipeline(device, &gizmo_pipeline_spec(formats))
                        .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
                )),
                None => None,
            };
        }
        Ok(())
    }

    /// Draws every line the host published, from the first view; no line or
    /// no view leaves the pass empty. A frame with more lines than the
    /// storage holds grows it first.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut GizmoInputs<'_>,
        out: &mut GizmoOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let built = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let done = LaneReport {
            done: 1.0,
            stalled: false,
        };
        let lines = &inputs.gizmos.0.lines;
        if lines.is_empty() {
            return Ok(done);
        }
        let Some(view) = inputs.render_world.views.first() else {
            return Ok(done);
        };
        let Some((_, pipeline)) = built.pipeline else {
            return Ok(done);
        };
        let device = out.pass.device();
        let needed = std::mem::size_of_val(lines.as_slice()) as u64;
        if needed > built.storage.size {
            grow_storage(built, device, needed)
                .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
            self.levels[0].memory.gpu = built.bytes();
        }
        built
            .camera
            .write_camera(device, view)
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        device
            .write_buffer(built.storage.buffer, 0, bytemuck::cast_slice(lines))
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        draw_gizmos(
            out.pass.encoder(),
            inputs.target.color,
            pipeline,
            built,
            lines.len() as u32,
        );
        Ok(done)
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(built), Some(device)) = (self.gpu.take(), gpu.device()) {
            built.camera.destroy(device);
            built.storage.destroy(device);
        }
        self.levels[0].memory.gpu = START_BYTES;
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// The camera block and the first line storage. A refusal halfway gives
/// back what came before it.
fn build_gizmo_gpu(
    device: &dyn GraphicsDevice,
    pipelines: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<GizmoGpu, khora_core::renderer::error::RenderError> {
    use khora_core::renderer::error::RenderError;

    // Layouts belong to the PipelineSystem, which caches them.
    let camera_layout =
        pipelines.inline_layout(device, GIZMO_CAMERA_LAYOUT_LABEL, &camera_layout_entries())?;
    let storage_layout = pipelines.inline_layout(
        device,
        GIZMO_STORAGE_LAYOUT_LABEL,
        &storage_layout_entries(),
    )?;
    let camera =
        Block::camera(device, camera_layout, "gizmo_camera").map_err(RenderError::ResourceError)?;
    let storage = match storage_block(device, storage_layout, GIZMO_CAPACITY) {
        Ok(storage) => storage,
        Err(error) => {
            camera.destroy(device);
            return Err(RenderError::ResourceError(error));
        }
    };
    Ok(GizmoGpu {
        camera,
        storage,
        storage_layout,
        pipeline: None,
    })
}

/// A line storage holding `lines` instances.
fn storage_block(
    device: &dyn GraphicsDevice,
    layout: BindGroupLayoutId,
    lines: usize,
) -> Result<Block, khora_core::renderer::error::ResourceError> {
    Block::new(
        device,
        layout,
        (lines * std::mem::size_of::<GizmoLineInstance>()) as u64,
        BufferUsage::STORAGE,
        "gizmo_storage",
    )
}

/// Replaces the storage by one holding at least `needed` bytes, twice the
/// lines it held at least; the old one is given back once the new stands.
fn grow_storage(
    built: &mut GizmoGpu,
    device: &dyn GraphicsDevice,
    needed: u64,
) -> Result<(), khora_core::renderer::error::ResourceError> {
    let line = std::mem::size_of::<GizmoLineInstance>() as u64;
    let held = built.storage.size / line;
    let lines = (needed / line).max(held * 2) as usize;
    let grown = storage_block(device, built.storage_layout, lines)?;
    std::mem::replace(&mut built.storage, grown).destroy(device);
    Ok(())
}

/// Group-0 camera layout: a single uniform buffer (vertex stage).
fn camera_layout_entries() -> Vec<BindGroupLayoutEntry> {
    crate::render_lane::util::single_uniform_layout(
        khora_core::renderer::api::util::ShaderStageFlags::VERTEX,
        false,
    )
}

/// Group-1 storage layout: the read-only line-instance storage buffer.
fn storage_layout_entries() -> Vec<BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{BindingType, BufferBindingType};
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

/// The declarative pipeline spec for the gizmos on a target of `color`
/// with `samples` — line-list, alpha blended, no depth (always on top).
fn gizmo_pipeline_spec((color, samples): (TextureFormat, u32)) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        BlendFactor, BlendOperation, PrimitiveTopology,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendComponentDescriptor, BlendStateDescriptor, ColorWrites,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, LayoutSpec, MultisampleStateDescriptor,
        PrimitiveStateDescriptor,
    };
    use khora_core::renderer::api::shader::ShaderVariantKey;
    use std::borrow::Cow;

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
                entries: Cow::Owned(camera_layout_entries()),
            },
            LayoutSpec::Inline {
                label: GIZMO_STORAGE_LAYOUT_LABEL,
                entries: Cow::Owned(storage_layout_entries()),
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
            format: color,
            blend: Some(blend),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: crate::render_lane::util::sample_count(samples),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// The gizmo pass — `LoadOp::Load` preserves the frame's colour; no depth
/// attachment.
fn draw_gizmos(
    encoder: &mut dyn CommandEncoder,
    color: TextureViewId,
    pipeline: RenderPipelineId,
    built: &GizmoGpu,
    lines: u32,
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
        label: Some("Gizmo Overlay Pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: None,
    };
    let mut pass = encoder.begin_render_pass(&pass_desc);
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &built.camera.bind_group, &[]);
    pass.set_bind_group(1, &built.storage.bind_group, &[]);
    // Two vertices per line; the vertex shader derives endpoints from
    // `vertex_index` (no vertex buffer bound).
    pass.draw(0..lines * 2, 0..1);
}
