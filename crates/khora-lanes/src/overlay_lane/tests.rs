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

//! The overlay lanes draw from the Views the host publishes on the bus — the
//! grid, gizmo and wireframe Views, lent to the lane context by reference as
//! every bus View is — and from nothing else: no runtime resource is left
//! for a host to write the overlays through.

use std::sync::{Arc, Mutex, RwLock};

use khora_core::lane::{ColorTarget, DepthTarget, Lane, LaneContext};
use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::{ComputePipelineId, RenderPipelineId};
use khora_core::renderer::api::resource::{BufferId, IndexFormat, TextureViewId};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::editor::GizmoLineInstance;
use khora_data::assets::Assets;
use khora_data::render::{
    ExtractedView, GizmoFrame, GizmoView, GridConfig, GridView, RenderWorld, WireframeConfig,
    WireframeView,
};

use super::{GizmoLane, GridLane, WireframeLane};
use crate::tests::{ledger_context, LedgerDevice, LedgerPipelines};

/// The labels of the render passes an encoder began.
type PassLog = Arc<Mutex<Vec<String>>>;

/// An encoder that records the render passes begun through it.
struct PassRecorder {
    passes: PassLog,
}

struct IgnoredRenderPass;

impl RenderPass<'_> for IgnoredRenderPass {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {}
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

struct IgnoredComputePass;

impl ComputePass<'_> for IgnoredComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for PassRecorder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        self.passes
            .lock()
            .expect("the pass log")
            .push(desc.label.unwrap_or_default().to_owned());
        Box::new(IgnoredRenderPass)
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(IgnoredComputePass)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(IgnoredComputePass)
    }
    fn copy_buffer_to_buffer(
        &mut self,
        _src: &BufferId,
        _src_off: u64,
        _dst: &BufferId,
        _dst_off: u64,
        _size: u64,
    ) {
    }
    fn finish(self: Box<Self>) -> Option<CommandBufferId> {
        Some(CommandBufferId(0))
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The three overlay Views of one frame.
struct OverlayViews {
    grid: GridView,
    gizmo: GizmoView,
    wireframe: WireframeView,
}

impl OverlayViews {
    /// Every overlay on: the grid and the wireframe enabled, one gizmo line.
    fn all_on() -> Self {
        Self {
            grid: GridView(GridConfig { enabled: true }),
            gizmo: GizmoView(GizmoFrame {
                lines: vec![GizmoLineInstance::new(
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    [1.0, 0.0, 0.0, 1.0],
                )],
            }),
            wireframe: WireframeView(WireframeConfig {
                enabled: true,
                ..WireframeConfig::default()
            }),
        }
    }

    /// The engine's defaults: what a host that sets nothing publishes.
    fn empty() -> Self {
        Self {
            grid: GridView::default(),
            gizmo: GizmoView::default(),
            wireframe: WireframeView::default(),
        }
    }
}

/// Initialises `lane` on a ledger device, then runs it once over a frame
/// whose bus carries `views` and a scene seen from one camera. Returns the
/// render passes it began.
fn passes_drawn(lane: &dyn Lane, views: &OverlayViews) -> Vec<String> {
    let device = Arc::new(LedgerDevice::default());
    let pipelines = Arc::new(LedgerPipelines::default());
    lane.on_initialize(&mut ledger_context(&device, &pipelines))
        .unwrap_or_else(|e| panic!("{}: initialises: {e}", lane.strategy_name()));

    let mut render_world = RenderWorld::new();
    render_world.views.push(ExtractedView {
        view_proj: Mat4::IDENTITY,
        position: Vec3::new(0.0, 2.0, 5.0),
    });
    let gpu_meshes: Arc<RwLock<Assets<GpuMesh>>> = Arc::new(RwLock::new(Assets::new()));
    let passes: PassLog = Arc::default();
    let mut encoder: Box<dyn CommandEncoder> = Box::new(PassRecorder {
        passes: Arc::clone(&passes),
    });
    {
        let mut ctx = LaneContext::new();
        ctx.insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
        ctx.insert(Arc::clone(&pipelines) as Arc<dyn PipelineSystem>);
        ctx.insert(gpu_meshes);
        ctx.insert_slot(encoder.as_mut());
        ctx.insert_ref(&render_world);
        ctx.insert(ColorTarget(TextureViewId(1)));
        ctx.insert(DepthTarget(TextureViewId(2)));
        // Bus Views are lent by reference, as the agent lends `RenderWorld`.
        ctx.insert_ref(&views.grid);
        ctx.insert_ref(&views.gizmo);
        ctx.insert_ref(&views.wireframe);
        lane.execute(&mut ctx)
            .unwrap_or_else(|e| panic!("{}: executes: {e}", lane.strategy_name()));
    }
    let drawn = passes.lock().expect("the pass log").clone();
    drawn
}

/// **The overlays draw from the host's Views.** With the grid and wireframe
/// Views enabled and a gizmo View holding a line, each overlay lane begins its
/// pass; with the engine's empty Views, none does. And nothing else could have
/// told them to: the shared runtime resources the overlays used to be written
/// through are gone from the overlay lanes, the data layer, the agent and the
/// engine's boot.
#[test]
fn overlay_lanes_read_host_views() {
    let lanes: [Box<dyn Lane>; 3] = [
        Box::new(GridLane::default()),
        Box::new(GizmoLane::default()),
        Box::new(WireframeLane::default()),
    ];

    for lane in &lanes {
        let drawn = passes_drawn(lane.as_ref(), &OverlayViews::all_on());
        assert!(
            !drawn.is_empty(),
            "{}: draws when its host View asks it to",
            lane.strategy_name()
        );
    }
    for lane in &lanes {
        let drawn = passes_drawn(lane.as_ref(), &OverlayViews::empty());
        assert!(
            drawn.is_empty(),
            "{}: the engine's empty View draws nothing, it began {drawn:?}",
            lane.strategy_name()
        );
    }

    // Spelled in two halves so this file never matches itself.
    let retired = [
        concat!("Shared", "GridConfig"),
        concat!("Shared", "GizmoFrame"),
        concat!("Shared", "WireframeConfig"),
    ];
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates folder");
    for file in [
        "khora-lanes/src/overlay_lane/grid.rs",
        "khora-lanes/src/overlay_lane/gizmo.rs",
        "khora-lanes/src/overlay_lane/wireframe.rs",
        "khora-data/src/render/mod.rs",
        "khora-data/src/render/grid.rs",
        "khora-data/src/render/gizmo.rs",
        "khora-data/src/render/wireframe.rs",
        "khora-agents/src/overlay_agent/agent.rs",
        "khora-sdk/src/engine/bootstrap.rs",
    ] {
        let source = std::fs::read_to_string(crates.join(file))
            .unwrap_or_else(|e| panic!("reads {file}: {e}"));
        for name in retired {
            assert!(
                !source.contains(name),
                "{file} still names the runtime resource `{name}`"
            );
        }
    }
}
