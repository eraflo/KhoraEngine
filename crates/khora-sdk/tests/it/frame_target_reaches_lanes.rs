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

//! The frame's target reaches a lane: beginning a frame publishes a `Target`
//! in the `FrameContext`, its key carrying the formats the backend reported,
//! and a `LaneAgent` whose lane declares `#[frame] Target` or `ClearColor`
//! lends them from there.
//!
//! The probe lanes below are run directly through `LaneAgent`, never
//! submitted to the registry, so no engine of this binary boots them.

use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::registration::AgentRegistration;
use khora_sdk::khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_sdk::khora_core::gpu::DeviceCaps;
use khora_sdk::khora_core::lane::contract::{Lane, LaneReport};
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::lane::family::{LaneFamily, Semantics};
use khora_sdk::khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind, SourceKind};
use khora_sdk::khora_core::lane::io::{
    Declaration, FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink, Target, TargetKey,
};
use khora_sdk::khora_core::lane::level::{Impact, Level};
use khora_sdk::khora_core::lane::{ClearColor, LaneBus, LaneError, LaneKind, OutputDeck};
use khora_sdk::khora_core::math::{Extent2D, LinearRgba};
use khora_sdk::khora_core::renderer::api::frame::FrameContext;
use khora_sdk::khora_core::renderer::api::resource::{TextureFormat, TextureViewId};
use khora_sdk::khora_core::{EngineContext, Runtime, WorldAccess};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    WindowConfig,
};

use crate::render_doubles::{insert_frame_context, install_renderer, RecordingRenderer};

// ─── The probe family ───────────────────────────────────────────────────────

/// What a probe lane was handed: the target, the clear colour, or neither.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Seen {
    pub color: Option<TextureViewId>,
    pub depth: Option<TextureViewId>,
    pub size: Option<Extent2D>,
    pub key: Option<TargetKey>,
    pub clear: Option<LinearRgba>,
}

/// The probe family's outputs: what its lane saw.
pub struct SeenOutputs {
    seen: Seen,
}

impl<'f> LaneOutputs<'f> for SeenOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Seen>("seen", SinkKind::Deck);
    }

    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            seen: Seen::default(),
        })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("seen", self.seen)
    }
}

/// A family whose lanes say what the frame handed them.
pub struct SeenFamily;

impl LaneFamily for SeenFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "frame probe";
    const AGENT: AgentId = AgentId::Asset;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = SeenOutputs;
}

/// The frame's target, as `#[frame] target: Target` derives it.
pub struct TargetInput {
    target: Target,
}

impl<'f> LaneInputs<'f> for TargetInput {
    fn declare(d: &mut Declaration) {
        d.read::<Target>("target", SourceKind::Frame);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            target: src.target("target")?,
        })
    }

    fn target_key(&self) -> Option<TargetKey> {
        Some(self.target.key)
    }
}

/// The frame's clear colour, as `#[frame] clear: ClearColor` derives it.
pub struct ClearInput {
    clear: ClearColor,
}

impl<'f> LaneInputs<'f> for ClearInput {
    fn declare(d: &mut Declaration) {
        d.read::<ClearColor>("clear", SourceKind::Frame);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            clear: src.clear_color("clear")?,
        })
    }
}

const LEVELS: &[Level] = &[Level {
    id: StrategyId::Balanced,
    name: "probe",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// Says which target it was handed.
#[derive(Default)]
pub struct TargetProbeLane;

impl Lane for TargetProbeLane {
    type Family = SeenFamily;
    type Inputs<'f> = TargetInput;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TargetInput) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut TargetInput,
        out: &mut SeenOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.seen = Seen {
            color: Some(inputs.target.color),
            depth: inputs.target.depth,
            size: Some(inputs.target.size),
            key: Some(inputs.target.key),
            clear: None,
        };
        Ok(LaneReport::default())
    }
}

/// Says which clear colour it was handed.
#[derive(Default)]
pub struct ClearProbeLane;

impl Lane for ClearProbeLane {
    type Family = SeenFamily;
    type Inputs<'f> = ClearInput;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &ClearInput) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut ClearInput,
        out: &mut SeenOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.seen = Seen {
            clear: Some(inputs.clear.0),
            ..Seen::default()
        };
        Ok(LaneReport::default())
    }
}

static TARGET_PROBE: AgentRegistration = khora_sdk::khora_core::register_agent! {
    family: SeenFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [TargetProbeLane],
};

static CLEAR_PROBE: AgentRegistration = khora_sdk::khora_core::register_agent! {
    family: SeenFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [ClearProbeLane],
};

/// Runs `agent` once over a bus for frame 1, as the scheduler runs it: with
/// its own declaration as the permit and no world, committing into a fresh
/// deck it returns.
fn run_once(agent: &mut LaneAgent, runtime: &Arc<Runtime>) -> OutputDeck {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(runtime),
        &bus,
        &mut deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
    deck
}

// ─── An engine with nothing of its own ──────────────────────────────────────

struct Bare;

impl AgentProvider for Bare {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Bare {}
impl EngineApp for Bare {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Bare
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// An engine booted over `renderer`, its frame context among the resources,
/// the tokio runtime that context was made with kept alive beside it.
fn engine_over(renderer: RecordingRenderer) -> (EngineCore<Bare>, tokio::runtime::Runtime) {
    let mut runtime = Runtime::new();
    let tokio = insert_frame_context(&mut runtime);
    install_renderer(&mut runtime, renderer);
    let mut engine = EngineCore::<Bare>::new();
    engine.bootstrap(Bare, runtime);
    (engine, tokio)
}

/// The `Target` the frame context of `runtime` holds.
fn published_target(runtime: &Runtime) -> Target {
    let context = runtime
        .resources
        .get::<Arc<FrameContext>>()
        .expect("the frame context is among the resources");
    let target = context
        .get::<Target>()
        .expect("beginning a frame publishes the frame's Target");
    *target
}

// ─── The SDK publishes the target ───────────────────────────────────────────

/// **Beginning a frame publishes its target with the backend's formats.** The
/// renderer acquired a 16-bit float colour attachment and a 24-bit depth
/// attachment: the frame context holds a `Target` on those two views, at the
/// colour attachment's size, its key carrying both formats and one sample.
#[test]
fn beginning_a_frame_publishes_the_target_with_the_backend_formats() {
    let (renderer, _) = RecordingRenderer::into_target(1024, 768);
    let renderer = renderer.with_formats(
        TextureFormat::Rgba16Float,
        Some((TextureViewId(2), TextureFormat::Depth24Plus)),
    );
    let (mut engine, _tokio) = engine_over(renderer);
    let runtime = Arc::clone(engine.runtime());

    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );

    let target = published_target(&runtime);
    assert_eq!(target.color, TextureViewId(1), "the colour attachment");
    assert_eq!(target.depth, Some(TextureViewId(2)), "the depth attachment");
    assert_eq!(
        target.size,
        Extent2D {
            width: 1024,
            height: 768
        }
    );
    assert_eq!(
        target.key,
        TargetKey {
            color: TextureFormat::Rgba16Float,
            depth: Some(TextureFormat::Depth24Plus),
            samples: 1,
        },
        "the key carries the formats the backend reported"
    );
}

/// A frame without a depth attachment publishes a target without depth, its
/// key naming no depth format.
#[test]
fn a_frame_without_depth_publishes_a_target_without_depth() {
    let (renderer, _) = RecordingRenderer::into_target(640, 480);
    let (mut engine, _tokio) = engine_over(renderer);
    let runtime = Arc::clone(engine.runtime());

    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );

    let target = published_target(&runtime);
    assert_eq!(target.depth, None);
    assert_eq!(
        target.key,
        TargetKey {
            color: TextureFormat::Rgba8UnormSrgb,
            depth: None,
            samples: 1,
        }
    );
}

// ─── A lane agent lends them ────────────────────────────────────────────────

/// **The frame target reaches a lane that declares it.** After the engine
/// began a frame, a `LaneAgent` whose lane reads `#[frame] Target` declares a
/// read of the frame context, and its lane is handed exactly the target the
/// frame published: its views, its size and its key.
#[test]
fn the_frame_target_reaches_a_lane_that_declares_it() {
    let (renderer, _) = RecordingRenderer::into_target(1280, 720);
    let renderer = renderer.with_formats(
        TextureFormat::Bgra8UnormSrgb,
        Some((TextureViewId(5), TextureFormat::Depth32Float)),
    );
    let (mut engine, _tokio) = engine_over(renderer);
    let runtime = Arc::clone(engine.runtime());
    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );
    let mut agent = lane_agent(&TARGET_PROBE, &DeviceCaps::default());

    let deck = run_once(&mut agent, &runtime);

    assert_eq!(
        deck.get::<Seen>(),
        Some(&Seen {
            color: Some(TextureViewId(1)),
            depth: Some(TextureViewId(5)),
            size: Some(Extent2D {
                width: 1280,
                height: 720
            }),
            key: Some(TargetKey {
                color: TextureFormat::Bgra8UnormSrgb,
                depth: Some(TextureFormat::Depth32Float),
                samples: 1,
            }),
            clear: None,
        }),
        "the lane recorded with the frame's target; status: {:?}",
        agent.report_status().message
    );
}

/// **A lane agent lends the clear colour too**, from the same frame context:
/// a lane reading `#[frame] ClearColor` is handed the colour published there.
#[test]
fn the_lane_agent_lends_the_clear_colour() {
    let mut runtime = Runtime::new();
    let _tokio = insert_frame_context(&mut runtime);
    let clear = LinearRgba::new(0.25, 0.5, 0.75, 1.0);
    runtime
        .resources
        .get::<Arc<FrameContext>>()
        .expect("the frame context")
        .insert(ClearColor(clear));
    let runtime = Arc::new(runtime);
    let mut agent = lane_agent(&CLEAR_PROBE, &DeviceCaps::default());

    let deck = run_once(&mut agent, &runtime);

    assert_eq!(
        deck.get::<Seen>().and_then(|seen| seen.clear),
        Some(clear),
        "the lane recorded with the frame's clear colour; status: {:?}",
        agent.report_status().message
    );
}

/// Each run lends the target its frame context holds then: a target replaced
/// after one run is the one the next run lends, never the first one kept.
#[test]
fn each_run_lends_the_target_of_its_frame() {
    let (renderer, _) = RecordingRenderer::into_target(800, 600);
    let renderer = renderer.with_formats(
        TextureFormat::Rgba8UnormSrgb,
        Some((TextureViewId(5), TextureFormat::Depth32Float)),
    );
    let (mut engine, _tokio) = engine_over(renderer);
    let runtime = Arc::clone(engine.runtime());
    let mut agent = lane_agent(&TARGET_PROBE, &DeviceCaps::default());
    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );
    run_once(&mut agent, &runtime);

    let context = runtime
        .resources
        .get::<Arc<FrameContext>>()
        .expect("the frame context");
    let mut resized = published_target(&runtime);
    resized.size = Extent2D {
        width: 400,
        height: 300,
    };
    context.insert(resized);
    let deck = run_once(&mut agent, &runtime);

    assert_eq!(
        deck.get::<Seen>().and_then(|seen| seen.size),
        Some(Extent2D {
            width: 400,
            height: 300
        }),
        "the second run lends the second target"
    );
}

// ─── A frame whose target is not acquired ───────────────────────────────────

/// A renderer whose first frame acquires a target and whose next ones skip,
/// as a window minimised after its first frame does.
#[derive(Debug)]
struct AcquiresOnce {
    acquired: bool,
    stats: khora_sdk::khora_core::renderer::api::device::RenderStats,
}

impl khora_sdk::RenderSystem for AcquiresOnce {
    fn init(
        &mut self,
        _window: &dyn khora_sdk::khora_core::platform::window::KhoraWindow,
    ) -> Result<
        Vec<Arc<dyn khora_sdk::khora_core::telemetry::ResourceMonitor>>,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        Ok(Vec::new())
    }
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn prepare_frame(
        &mut self,
        _view_info: &khora_sdk::khora_core::renderer::api::resource::ViewInfo,
    ) {
    }
    fn render(
        &mut self,
        _renderables: &[khora_sdk::khora_core::renderer::api::gpu_scene::RenderObject],
        _view_info: &khora_sdk::khora_core::renderer::api::resource::ViewInfo,
        _settings: &khora_sdk::khora_core::renderer::api::device::RenderSettings,
    ) -> Result<
        khora_sdk::khora_core::renderer::api::device::RenderStats,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        Ok(Default::default())
    }
    fn get_last_frame_stats(&self) -> &khora_sdk::khora_core::renderer::api::device::RenderStats {
        &self.stats
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
    fn get_adapter_info(
        &self,
    ) -> Option<khora_sdk::khora_core::renderer::api::device::GraphicsAdapterInfo> {
        None
    }
    fn graphics_device(&self) -> Arc<dyn khora_sdk::khora_core::renderer::GraphicsDevice> {
        unimplemented!("the engine reaches the device through the backends")
    }
    fn begin_frame(
        &mut self,
    ) -> Result<
        khora_sdk::khora_core::renderer::traits::FrameTargets,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        if self.acquired {
            return Err(
                khora_sdk::khora_core::renderer::error::RenderError::SurfaceAcquisitionFailed(
                    "frame skipped (surface not ready)".to_string(),
                ),
            );
        }
        self.acquired = true;
        Ok(khora_sdk::khora_core::renderer::traits::FrameTargets {
            color: TextureViewId(1),
            depth: Some(TextureViewId(5)),
            size: Extent2D {
                width: 800,
                height: 600,
            },
            color_format: TextureFormat::Bgra8UnormSrgb,
            depth_format: Some(TextureFormat::Depth32Float),
        })
    }
    fn end_frame(
        &mut self,
    ) -> Result<
        khora_sdk::khora_core::renderer::api::device::RenderStats,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        Ok(Default::default())
    }
    fn shutdown(&mut self) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A frame whose target was not acquired lends no target.** The first
/// frame acquired the swapchain image and published its `Target`; the next
/// frame's acquisition is skipped (a minimised window). That frame has no
/// image to draw into: a lane reading `#[frame] Target` is refused, never
/// handed the previous frame's image, which was presented already.
#[test]
fn a_frame_whose_target_was_not_acquired_lends_no_target() {
    let mut runtime = Runtime::new();
    let _tokio = insert_frame_context(&mut runtime);
    let renderer: Arc<std::sync::Mutex<Box<dyn khora_sdk::RenderSystem>>> =
        Arc::new(std::sync::Mutex::new(Box::new(AcquiresOnce {
            acquired: false,
            stats: Default::default(),
        })));
    runtime.backends.insert(renderer);
    let mut engine = EngineCore::<Bare>::new();
    engine.bootstrap(Bare, runtime);
    let runtime = Arc::clone(engine.runtime());
    let mut agent = lane_agent(&TARGET_PROBE, &DeviceCaps::default());
    assert!(
        engine.begin_render_frame(&runtime),
        "the first frame acquires"
    );
    run_once(&mut agent, &runtime);

    assert!(
        !engine.begin_render_frame(&runtime),
        "the second frame's acquisition is skipped"
    );
    let deck = run_once(&mut agent, &runtime);

    assert_eq!(
        deck.get::<Seen>(),
        None,
        "a skipped frame hands the lane no target; status: {:?}",
        agent.report_status().message
    );
}
