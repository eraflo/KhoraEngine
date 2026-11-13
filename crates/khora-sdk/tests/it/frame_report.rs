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

//! Each frame reports to the DCC how long it kept the host busy, the waits
//! for the display excluded, and the DCC's budget multiplier follows it.

use crate::protocol::{register_after_boot, report_of, Hand, Work};
use khora_sdk::khora_core::agent::gorna::protocol::{
    CostBasis, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::{Agent, Contention, Place};
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::math::Extent2D;
use khora_sdk::khora_core::platform::window::KhoraWindow;
use khora_sdk::khora_core::renderer::api::device::{
    GraphicsAdapterInfo, RenderSettings, RenderStats,
};
use khora_sdk::khora_core::renderer::api::gpu_scene::RenderObject;
use khora_sdk::khora_core::renderer::api::resource::{TextureFormat, TextureViewId, ViewInfo};
use khora_sdk::khora_core::renderer::error::RenderError;
use khora_sdk::khora_core::renderer::traits::FrameTargets;
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::khora_core::telemetry::ResourceMonitor;
use khora_sdk::khora_core::EngineContext;
use khora_sdk::{
    DccContext, EngineApp, EngineCore, ExecutionTiming, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// How long a slow `update` works, well over the 16.66 ms frame target.
const SLOW_UPDATE: Duration = Duration::from_millis(30);
/// How long the display keeps the frame waiting in `begin_frame`.
const DISPLAY_WAIT: Duration = Duration::from_millis(40);

/// An application whose `update` works [`SLOW_UPDATE`] while `slow` is set,
/// and returns at once otherwise.
struct Workload {
    slow: Arc<AtomicBool>,
}

impl Workload {
    fn slow() -> Self {
        Workload {
            slow: Arc::new(AtomicBool::new(true)),
        }
    }
}

/// `app`, booted over `runtime`, with a [`Graded`] agent registered beside
/// the engine's own.
fn booted(app: Workload, runtime: Runtime) -> EngineCore<Workload> {
    let mut engine = EngineCore::<Workload>::new();
    engine.bootstrap(app, runtime);
    register_after_boot(&engine, Graded::shared(), 0.5);
    engine
}

/// The two rows a [`Graded`] agent offers: a cheap floor and a dearer row
/// above it. With one above its floor, a lower frame target has something to
/// take back — what the frame governor needs before it lowers the multiplier.
fn graded_rows() -> Vec<OfferRow> {
    let row = |id, millis| OfferRow {
        id,
        cost: Cost {
            time: PerDevice {
                cpu: Duration::from_millis(millis),
                ..PerDevice::default()
            },
            ..Cost::default()
        },
        quality: 1.0,
        confidence: CostBasis::Quoted,
    };
    vec![row(StrategyId::LowPower, 1), row(StrategyId::Balanced, 4)]
}

/// An agent that does nothing but offer [`graded_rows`]: the lever the
/// governor lowers the multiplier through. Its place runs nothing.
struct Graded {
    place: Arc<Mutex<Place>>,
}

impl Graded {
    fn shared() -> Arc<Mutex<dyn Agent>> {
        Arc::new(Mutex::new(Graded {
            place: Arc::new(Mutex::new(Place::new(AgentId::new("graded")))),
        }))
    }
}

impl Agent for Graded {
    fn id(&self) -> AgentId {
        AgentId::new("graded")
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        Offer {
            agent: AgentId::new("graded"),
            epoch: request.epoch,
            rows: graded_rows(),
            floor: StrategyId::LowPower,
            current: StrategyId::Balanced,
            health: 1.0,
        }
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(AgentId::new("graded"), StrategyId::Balanced)
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
}
impl PhaseProvider for Workload {}
impl EngineApp for Workload {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Workload {
            slow: Arc::new(AtomicBool::new(false)),
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {
        if self.slow.load(Ordering::Relaxed) {
            std::thread::sleep(SLOW_UPDATE);
        }
    }
}

/// A renderer whose display keeps every frame waiting [`DISPLAY_WAIT`] in
/// `begin_frame`, as a vsync'd swapchain acquisition does.
#[derive(Debug)]
struct WaitingDisplay {
    stats: RenderStats,
}

impl khora_sdk::RenderSystem for WaitingDisplay {
    fn init(
        &mut self,
        _window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError> {
        Ok(Vec::new())
    }
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn prepare_frame(&mut self, _view_info: &ViewInfo) {}
    fn render(
        &mut self,
        _renderables: &[RenderObject],
        _view_info: &ViewInfo,
        _settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError> {
        Ok(RenderStats::default())
    }
    fn get_last_frame_stats(&self) -> &RenderStats {
        &self.stats
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo> {
        None
    }
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice> {
        unimplemented!("the engine reaches the device through the backends")
    }
    fn begin_frame(&mut self) -> Result<FrameTargets, RenderError> {
        std::thread::sleep(DISPLAY_WAIT);
        Ok(FrameTargets {
            color: TextureViewId(1),
            depth: None,
            size: Extent2D {
                width: 800,
                height: 600,
            },
            color_format: TextureFormat::Rgba8UnormSrgb,
            depth_format: None,
        })
    }
    fn end_frame(&mut self) -> Result<RenderStats, RenderError> {
        Ok(RenderStats::default())
    }
    fn shutdown(&mut self) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The DCC's budget multiplier, read from the live context the engine shares
/// among the runtime's resources.
fn multiplier<A: EngineApp>(engine: &EngineCore<A>) -> f32 {
    engine
        .runtime()
        .resources
        .get::<Arc<RwLock<DccContext>>>()
        .expect("the bootstrap shares the DCC's context")
        .read()
        .expect("the context lock")
        .global_budget_multiplier
}

/// Ticks until the multiplier drops below `below` or `secs` seconds have
/// passed; the multiplier it dropped to, if it did.
fn tick_until_below<A: EngineApp>(
    engine: &mut EngineCore<A>,
    below: f32,
    secs: u64,
) -> Option<f32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        engine.tick();
        let now = multiplier(engine);
        if now < below {
            return Some(now);
        }
    }
    None
}

/// **A slow frame pulls the DCC's multiplier down.** An application whose
/// `update` works 30 ms, with no renderer: each frame reports the host busy
/// at least that long, over the 16.66 ms target, and within a few seconds of
/// frames the DCC pulls the budget multiplier below 1.0.
#[test]
fn a_slow_frame_pulls_the_multiplier_down() {
    let mut engine = booted(Workload::slow(), Runtime::new());
    assert_eq!(multiplier(&engine), 1.0, "the DCC starts at full budget");

    let dropped = tick_until_below(&mut engine, 1.0, 10);
    engine.shutdown();

    assert!(
        dropped.is_some(),
        "30 ms frames pull the multiplier below 1.0, still {}",
        multiplier(&engine)
    );
}

/// **The display's wait is not the frame's work.** A renderer whose
/// `begin_frame` waits 40 ms for the display, under a fast application:
/// thirty frames — far more samples than the DCC waits for, over many of its
/// ticks — leave the multiplier at 1.0 (within 0.1, for a loaded machine's
/// jitter; counted, the 40 ms would pull it near 0.4). The DCC was listening:
/// once `update` works 30 ms, the multiplier drops.
#[test]
fn the_display_wait_is_not_counted_in_the_frame() {
    let mut runtime = Runtime::new();
    let renderer: Arc<Mutex<Box<dyn khora_sdk::RenderSystem>>> =
        Arc::new(Mutex::new(Box::new(WaitingDisplay {
            stats: RenderStats::default(),
        })));
    runtime.backends.insert(renderer);
    let slow = Arc::new(AtomicBool::new(false));
    let mut engine = booted(
        Workload {
            slow: Arc::clone(&slow),
        },
        runtime,
    );

    let mut lowest = multiplier(&engine);
    for _ in 0..30 {
        engine.tick();
        lowest = lowest.min(multiplier(&engine));
    }
    // A few more DCC ticks over the last reports.
    let settle = Instant::now() + Duration::from_millis(200);
    while Instant::now() < settle {
        std::thread::sleep(Duration::from_millis(10));
        lowest = lowest.min(multiplier(&engine));
    }

    slow.store(true, Ordering::Relaxed);
    let control = tick_until_below(&mut engine, 0.9, 15);
    engine.shutdown();

    assert!(
        lowest >= 0.9,
        "a fast frame waiting 40 ms for the display keeps the multiplier at 1.0, lowest {lowest}"
    );
    assert!(
        control.is_some(),
        "the frames reach the DCC: 30 ms of work pull the multiplier below 0.9, still {}",
        multiplier(&engine)
    );
}

/// How long the display keeps the frame waiting in `end_frame`.
const PRESENT_WAIT: Duration = Duration::from_millis(40);

/// A renderer whose display keeps every frame waiting [`PRESENT_WAIT`] in
/// `end_frame`, as a vsync'd present does; acquiring the image is immediate.
#[derive(Debug)]
struct WaitingPresent {
    stats: RenderStats,
}

impl khora_sdk::RenderSystem for WaitingPresent {
    fn init(
        &mut self,
        _window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError> {
        Ok(Vec::new())
    }
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn prepare_frame(&mut self, _view_info: &ViewInfo) {}
    fn render(
        &mut self,
        _renderables: &[RenderObject],
        _view_info: &ViewInfo,
        _settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError> {
        Ok(RenderStats::default())
    }
    fn get_last_frame_stats(&self) -> &RenderStats {
        &self.stats
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo> {
        None
    }
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice> {
        unimplemented!("the engine reaches the device through the backends")
    }
    fn begin_frame(&mut self) -> Result<FrameTargets, RenderError> {
        Ok(FrameTargets {
            color: TextureViewId(1),
            depth: None,
            size: Extent2D {
                width: 800,
                height: 600,
            },
            color_format: TextureFormat::Rgba8UnormSrgb,
            depth_format: None,
        })
    }
    fn end_frame(&mut self) -> Result<RenderStats, RenderError> {
        std::thread::sleep(PRESENT_WAIT);
        Ok(RenderStats::default())
    }
    fn shutdown(&mut self) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **The present's wait is not the frame's work.** A renderer whose
/// `end_frame` waits 40 ms for the display, under a fast application: thirty
/// frames leave the multiplier at 1.0 (within 0.1). The DCC was listening:
/// once `update` works 30 ms, the multiplier drops.
#[test]
fn the_present_wait_is_not_counted_in_the_frame() {
    let mut runtime = Runtime::new();
    let renderer: Arc<Mutex<Box<dyn khora_sdk::RenderSystem>>> =
        Arc::new(Mutex::new(Box::new(WaitingPresent {
            stats: RenderStats::default(),
        })));
    runtime.backends.insert(renderer);
    let slow = Arc::new(AtomicBool::new(false));
    let mut engine = booted(
        Workload {
            slow: Arc::clone(&slow),
        },
        runtime,
    );

    let mut lowest = multiplier(&engine);
    for _ in 0..30 {
        engine.tick();
        lowest = lowest.min(multiplier(&engine));
    }
    let settle = Instant::now() + Duration::from_millis(200);
    while Instant::now() < settle {
        std::thread::sleep(Duration::from_millis(10));
        lowest = lowest.min(multiplier(&engine));
    }

    slow.store(true, Ordering::Relaxed);
    let control = tick_until_below(&mut engine, 0.9, 15);
    engine.shutdown();

    assert!(
        lowest >= 0.9,
        "a fast frame waiting 40 ms to present keeps the multiplier at 1.0, lowest {lowest}"
    );
    assert!(
        control.is_some(),
        "the frames reach the DCC: 30 ms of work pull the multiplier below 0.9, still {}",
        multiplier(&engine)
    );
}

/// How long the slow agent works each time it runs.
const SLOW_AGENT: Duration = Duration::from_millis(30);

/// An agent whose layer works [`SLOW_AGENT`] each time the scheduler runs
/// it, and counts its runs.
fn slow_agent(runs: &Arc<AtomicUsize>) -> Arc<Mutex<dyn Agent>> {
    let runs = Arc::clone(runs);
    let layer = Work::new(Contention::none(), move |_: &mut EngineContext<'_>| {
        runs.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(SLOW_AGENT);
    });
    Hand::new(
        AgentId::new("slow_agent"),
        ExecutionTiming::default(),
        layer,
    )
    .shared()
}

/// A fast application whose one agent, registered once it booted, works
/// [`SLOW_AGENT`] in the scheduler.
struct SlowScheduled {
    runs: Arc<AtomicUsize>,
}

impl PhaseProvider for SlowScheduled {}
impl EngineApp for SlowScheduled {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        SlowScheduled {
            runs: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **The agents' work is the frame's.** A fast `update`, but an agent working
/// 30 ms in the scheduler: the frame closes after the scheduler ran, so each
/// frame reports the host busy at least that long and the DCC pulls the
/// multiplier below 1.0.
#[test]
fn a_slow_agent_pulls_the_multiplier_down() {
    let app = SlowScheduled::new();
    let runs = Arc::clone(&app.runs);
    let mut engine = EngineCore::<SlowScheduled>::new();
    engine.bootstrap(app, Runtime::new());
    register_after_boot(&engine, slow_agent(&runs), 0.5);
    // Its work is the frame's, but no row of its own is a lever: the
    // governor lowers the multiplier through this one's.
    register_after_boot(&engine, Graded::shared(), 0.5);

    let dropped = tick_until_below(&mut engine, 1.0, 10);
    engine.shutdown();

    assert!(
        runs.load(Ordering::SeqCst) > 0,
        "the scheduler runs the agent"
    );
    assert!(
        dropped.is_some(),
        "frames whose agent works 30 ms pull the multiplier below 1.0, still {}",
        multiplier(&engine)
    );
}
