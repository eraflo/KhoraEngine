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
//! Renders `khora::pipelines::grid` (a fullscreen-triangle infinite grid) on
//! top of the frame's target, depth-tested against the scene so closer
//! geometry occludes it; it reads the depth and never writes it. The grid
//! family runs in the OUTPUT phase after the renderer, and `LoadOp::Load`
//! preserves what the scene drew.
//!
//! The grid is editor viz: a host application opts in by enabling the
//! [`GridView`] it publishes. The sandbox leaves it disabled — no grid.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::renderer::api::command::{
    BindGroupLayoutEntry, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::{PipelineSpec, RenderPipelineId};
use khora_core::renderer::api::resource::{CameraUniformData, TextureViewId};
use khora_core::renderer::traits::CommandEncoder;
use khora_data::render::{GridView, RenderWorld};
use khora_macros::LaneInputs;

use super::block::Block;
use super::GridOutputs;

/// Stable cache label for the grid camera layout.
const GRID_CAMERA_LAYOUT_LABEL: &str = "grid_camera_layout";

/// What recording the grid's two triangles costs on the host.
const RECORD_CPU: Duration = Duration::from_micros(15);
/// What shading one pixel of the grid costs on the device.
const PIXEL_GPU_SECS: f64 = 1e-10;

/// The lane's one level: its camera block is the memory it holds.
const LEVEL: Level = Level {
    id: StrategyId::Balanced,
    name: "grid",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: std::mem::size_of::<CameraUniformData>() as u64,
        npu: 0,
        other: 0,
    },
};

/// Infinite editor ground-grid overlay lane. Holds what it built on the
/// device; `None` before the first prepare and after shutdown.
#[derive(Debug)]
pub struct GridLane {
    levels: [Level; 1],
    gpu: Option<GridGpu>,
}

impl Default for GridLane {
    fn default() -> Self {
        Self {
            levels: [LEVEL],
            gpu: None,
        }
    }
}

/// What the grid builds on the device.
#[derive(Debug)]
struct GridGpu {
    camera: Block,
    /// The pipeline and the target it was built for; `None` for a target
    /// without depth, which the grid cannot test against.
    pipeline: Option<(TargetKey, RenderPipelineId)>,
}

/// What the grid lane reads each frame: whether the host shows the grid, the
/// camera it is seen from, and the target it is drawn into.
#[derive(LaneInputs)]
pub struct GridInputs<'f> {
    /// The host's grid switch.
    #[bus]
    pub grid: &'f GridView,
    /// The frame's views; the first is the camera the grid is seen from.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour and depth attachments.
    #[frame]
    pub target: Target,
}

impl Lane for GridLane {
    type Family = super::GridFamily;
    type Inputs<'f> = GridInputs<'f>;

    fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Two triangles on the host; on the device, every pixel of the target
    /// is shaded. A grid that draws nothing — off, without a camera or
    /// without depth — costs no time.
    fn cost(&self, _level: usize, inputs: &GridInputs<'_>) -> Cost {
        let draws = inputs.grid.0.enabled
            && !inputs.render_world.views.is_empty()
            && inputs.target.depth.is_some();
        let size = inputs.target.size;
        let pixels = f64::from(size.width) * f64::from(size.height);
        let time = if draws {
            PerDevice {
                cpu: RECORD_CPU,
                gpu: Duration::from_secs_f64(pixels * PIXEL_GPU_SECS),
                ..PerDevice::default()
            }
        } else {
            PerDevice::default()
        };
        Cost {
            time,
            memory: self.levels[0].memory,
            ..Cost::default()
        }
    }

    /// Builds the camera block once, and the pipeline for the target's
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
        let built = match &mut self.gpu {
            Some(built) => built,
            empty => {
                let layout = pipelines
                    .inline_layout(device, GRID_CAMERA_LAYOUT_LABEL, &camera_layout_entries())
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
                let camera = Block::camera(device, layout, "grid_camera")
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
                empty.insert(GridGpu {
                    camera,
                    pipeline: None,
                })
            }
        };
        self.levels[0].memory.gpu = built.camera.size;
        let wanted = key.filter(|key| key.depth.is_some());
        if built.pipeline.map(|(built_for, _)| built_for) != wanted {
            built.pipeline = match wanted {
                Some(key) => Some((
                    key,
                    pipelines
                        .pipeline(device, &grid_pipeline_spec(key))
                        .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
                )),
                None => None,
            };
        }
        Ok(())
    }

    /// Draws the grid when the host enabled it, the world has a camera and
    /// the target has depth; otherwise the pass stays empty.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut GridInputs<'_>,
        out: &mut GridOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let built = self.gpu.as_ref().ok_or(LaneError::NotInitialized)?;
        let done = LaneReport {
            done: 1.0,
            stalled: false,
        };
        if !inputs.grid.0.enabled {
            return Ok(done);
        }
        let Some(view) = inputs.render_world.views.first() else {
            return Ok(done);
        };
        let (Some(depth), Some((_, pipeline))) = (inputs.target.depth, built.pipeline) else {
            return Ok(done);
        };
        built
            .camera
            .write_camera(out.pass.device(), view)
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        draw_grid(
            out.pass.encoder(),
            inputs.target.color,
            depth,
            pipeline,
            built,
        );
        Ok(done)
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(built), Some(device)) = (self.gpu.take(), gpu.device()) {
            built.camera.destroy(device);
        }
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// Group-0 camera layout: a single uniform buffer (vertex + fragment).
fn camera_layout_entries() -> Vec<BindGroupLayoutEntry> {
    crate::render_lane::util::single_uniform_layout(
        khora_core::renderer::api::util::ShaderStageFlags::VERTEX
            | khora_core::renderer::api::util::ShaderStageFlags::FRAGMENT,
        false,
    )
}

/// The declarative pipeline spec for the grid on a target of `key`'s
/// formats — fullscreen triangles, alpha-blended, depth-tested (LessEqual)
/// against the scene buffer, which it never writes.
fn grid_pipeline_spec(key: TargetKey) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        BlendFactor, BlendOperation, CompareFunction, PrimitiveTopology,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendComponentDescriptor, BlendStateDescriptor, ColorWrites, DepthBiasState,
        StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
        MultisampleStateDescriptor, PrimitiveStateDescriptor,
    };
    use khora_core::renderer::api::shader::ShaderVariantKey;
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
            entries: Cow::Owned(camera_layout_entries()),
        }],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        // The grid fragment shader writes `@builtin(frag_depth)` so the
        // LessEqual test against the scene lets closer geometry occlude it;
        // the pass only reads the depth, so the pipeline never writes it.
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
            blend: Some(blend),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: crate::render_lane::util::sample_count(key.samples),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// The grid pass — `LoadOp::Load` preserves the scene's colour and depth.
fn draw_grid(
    encoder: &mut dyn CommandEncoder,
    color: TextureViewId,
    depth: TextureViewId,
    pipeline: RenderPipelineId,
    built: &GridGpu,
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
        label: Some("Grid Overlay Pass"),
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
    // Fullscreen triangle pair — the vertex shader derives positions from
    // `vertex_index` (no vertex buffer bound).
    pass.draw(0..6, 0..1);
}
