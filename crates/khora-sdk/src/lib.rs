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

//! The public-facing Software Development Kit (SDK) for the Khora Engine.
//!
//! This is the **only** crate that should be used by game developers.
//! All internal crates (khora-agents, khora-control, etc.) are implementation details.

#![warn(missing_docs)]

mod game_world;
mod vessel;

pub use game_world::GameWorld;
pub use vessel::{spawn_cube_at, spawn_plane, spawn_sphere, Vessel};

use anyhow::Result;
use khora_control::{DccConfig, DccService};
use khora_core::platform::KhoraWindow;
use khora_core::renderer::api::scene::RenderObject;
use khora_core::renderer::api::core::RenderSettings;
use khora_core::renderer::traits::RenderSystem;
use khora_core::telemetry::MonitoredResourceType;
use khora_core::ServiceRegistry;
use khora_infra::platform::input::translate_winit_input;
use khora_infra::platform::window::{WinitWindow, WinitWindowBuilder};
use khora_infra::telemetry::memory_monitor::MemoryMonitor;
use khora_infra::{GpuMonitor, WgpuRenderSystem};
use khora_telemetry::TelemetryService;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

pub mod prelude {
    //! Common imports for convenience.
    pub use khora_core::asset::{AssetHandle, AssetMetadata, AssetSource, AssetUUID};
    pub use khora_core::renderer::api::{
        core::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData},
        pipeline::{
            ColorTargetStateDescriptor, ColorWrites, CompareFunction, DepthStencilStateDescriptor,
            MultisampleStateDescriptor, PipelineLayoutDescriptor, RenderPipelineDescriptor,
            RenderPipelineId, VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
            VertexFormat, VertexStepMode,
        },
        pipeline::state::{DepthBiasState, StencilFaceState},
        resource::{BufferDescriptor, BufferId, BufferUsage},
        scene::RenderObject,
        util::{IndexFormat, SampleCount, ShaderStageFlags as ShaderStage, TextureFormat},
    };
    pub use khora_core::EngineContext;
    pub use khora_data::allocators::SaaTrackingAllocator;
    pub use khora_data::ecs::HandleComponent;
    pub use khora_infra::platform::input::MouseButton;

    pub mod ecs {
        //! ECS types exposed through the SDK.
        pub use khora_core::ecs::entity::EntityId;
        pub use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
        pub use khora_data::ecs::{
            Camera, Component, ComponentBundle, GlobalTransform, Light, MaterialComponent,
            Transform,
        };
    }

    pub mod materials {
        //! Built-in material types.
        pub use khora_core::asset::{
            EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
        };
    }

    pub mod shaders {
        //! Built-in engine shaders.
        pub use khora_lanes::render_lane::shaders::*;
    }

    pub mod math {
        //! Math types and utilities.
        pub use khora_core::math::*;
    }
}

pub use khora_core::EngineContext;
pub use khora_infra::platform::input::InputEvent;

/// Application trait for user-defined game logic.
///
/// The engine manages the internal state. Users interact through
/// `&mut GameWorld` - no direct access to internal engine types.
pub trait Application: Sized + 'static {
    /// Called once at initialization with the graphics context.
    fn new(context: EngineContext) -> Self;

    /// Called once after construction for scene setup.
    fn setup(&mut self, _world: &mut GameWorld) {}

    /// Called every frame for game logic.
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}

    /// Called every frame to produce render objects.
    fn render(&mut self) -> Vec<RenderObject> {
        Vec::new()
    }
}

/// Internal engine state.
struct EngineState<A: Application> {
    app: Option<A>,
    game_world: Option<GameWorld>,
    window: Option<WinitWindow>,
    renderer: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    /// Cached graphics device (extracted from renderer at init).
    graphics_device: Option<Arc<dyn khora_core::renderer::GraphicsDevice>>,
    telemetry: Option<TelemetryService>,
    dcc: Option<DccService>,
    render_settings: RenderSettings,
    simulation_started: bool,
    running: Arc<AtomicBool>,
    /// Accumulated input events for the current frame.
    input_events: VecDeque<InputEvent>,
}

impl<A: Application> EngineState<A> {
    fn log_telemetry_summary(&self) {
        if let Some(telemetry) = &self.telemetry {
            log::info!("--- Telemetry Summary ---");
            for monitor in telemetry.monitor_registry().get_all_monitors() {
                let report = monitor.get_usage_report();
                match monitor.resource_type() {
                    MonitoredResourceType::SystemRam => {
                        let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                        let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                        log::info!("  RAM: {:.2} MB (Peak: {:.2} MB)", current_mb, peak_mb);
                    }
                    MonitoredResourceType::Vram => {
                        let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                        let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                        log::info!("  VRAM: {:.2} MB (Peak: {:.2} MB)", current_mb, peak_mb);
                    }
                    MonitoredResourceType::Gpu => {
                        if let Some(gpu_monitor) = monitor.as_any().downcast_ref::<GpuMonitor>() {
                            if let Some(gpu_report) = gpu_monitor.get_gpu_report() {
                                log::info!(
                                    "  GPU: {:.3} ms (Frame: {})",
                                    gpu_report.frame_total_duration_us().unwrap_or(0) as f32
                                        / 1000.0,
                                    gpu_report.frame_number
                                );
                            }
                        }
                    }
                    MonitoredResourceType::Hardware => {
                        log::info!("  Hardware: Active");
                    }
                }
            }
            log::info!("-------------------------");
        }
    }
}

impl<A: Application> Drop for EngineState<A> {
    fn drop(&mut self) {
        log::info!("EngineState: Shutting down...");
        self.running.store(false, Ordering::SeqCst);
        if let Some(renderer) = self.renderer.take() {
            if let Ok(mut r) = renderer.lock() {
                r.shutdown();
            }
        }
        log::info!("Engine shutdown complete.");
    }
}

impl<A: Application> ApplicationHandler for EngineState<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Engine: Initializing...");

        let window = WinitWindowBuilder::new().build(event_loop).unwrap();
        let mut renderer: Box<dyn RenderSystem> = Box::new(WgpuRenderSystem::new());
        let renderer_monitors = renderer.init(&window).unwrap();

        let (mut dcc, dcc_rx) = DccService::new(DccConfig::default());
        let telemetry =
            TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());

        dcc.start(dcc_rx);

        for monitor in renderer_monitors {
            telemetry.monitor_registry().register(monitor);
        }

        let memory_monitor = Arc::new(MemoryMonitor::new("System_RAM".to_string()));
        telemetry.monitor_registry().register(memory_monitor);

        let graphics_device = renderer.graphics_device();

        // Build a minimal EngineContext for Application::new().
        let mut services = ServiceRegistry::new();
        services.insert(graphics_device.clone());
        let context = EngineContext {
            world: None,
            services,
        };

        let mut app = A::new(context);
        let mut game_world = GameWorld::new();
        app.setup(&mut game_world);

        // Register default agents with their execution priorities
        // Higher priority = executed first in the update loop
        Self::register_default_agents(&dcc, graphics_device.clone());

        let _ = dcc
            .event_sender()
            .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                "boot".to_string(),
            ));

        // Wrap renderer in Arc<Mutex<...>> for ServiceRegistry sharing.
        let renderer = Arc::new(Mutex::new(renderer));

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.graphics_device = Some(graphics_device);
        self.telemetry = Some(telemetry);
        self.dcc = Some(dcc);
        self.game_world = Some(game_world);
        self.render_settings = RenderSettings::default();
        self.simulation_started = false;
        self.app = Some(app);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Shutdown requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_ref() {
                    log::info!("Window resized: {}x{}", size.width, size.height);
                    if let Ok(mut r) = renderer.lock() {
                        r.resize(size.width, size.height);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.handle_frame(event_loop);
            }
            _ => {
                // Log raw events to debug
                if !matches!(event, WindowEvent::CursorMoved { .. }) {
                    log::info!("Raw event: {:?}", event);
                }
                if let Some(input_event) = translate_winit_input(&event) {
                    log::info!("Input: {:?}", input_event);
                    self.input_events.push_back(input_event);
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl<A: Application> EngineState<A> {
    fn register_default_agents(
        dcc: &DccService,
        _graphics_device: Arc<dyn khora_core::renderer::GraphicsDevice>,
    ) {
        // Priorities: higher = executed first
        // Renderer: 1.0 (critical for visual feedback)
        // Physics: 0.9 (critical for gameplay)
        // Ecs: 0.8 (garbage collection, less critical)
        // Asset: 0.5 (background loading)

        let render_agent = khora_agents::render_agent::RenderAgent::new()
            .with_telemetry_sender(dcc.event_sender());
        dcc.register_agent(Arc::new(Mutex::new(render_agent)), 1.0);

        let gc_agent = khora_agents::ecs_agent::GarbageCollectorAgent::new()
            .with_dcc_sender(dcc.event_sender());
        dcc.register_agent(Arc::new(Mutex::new(gc_agent)), 0.8);

        log::info!("Engine: Registered {} default agents", dcc.agent_count());
    }

    fn handle_frame(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let Some(telemetry) = self.telemetry.as_mut() else {
            return;
        };

        if !self.simulation_started {
            if let Some(dcc) = &self.dcc {
                let _ =
                    dcc.event_sender()
                        .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                            "simulation".to_string(),
                        ));
            }
            self.simulation_started = true;
        }

        let should_log_summary = telemetry.tick();

        let Some(app) = self.app.as_mut() else { return };

        // Collect inputs for this frame
        let inputs: Vec<InputEvent> = self.input_events.drain(..).collect();

        // User update with inputs (logic first - move camera, etc.)
        if let Some(gw) = self.game_world.as_mut() {
            app.update(gw, &inputs);
        }

        // Build the ServiceRegistry for this frame.
        let mut services = ServiceRegistry::new();
        if let Some(device) = &self.graphics_device {
            services.insert(device.clone());
        }
        services.insert(Arc::clone(renderer));

        // Update all agents in priority order (handled by DCC).
        // The RenderAgent will extract scene data, prepare the frame,
        // and render — all within its update() method.
        if let (Some(dcc), Some(gw)) = (&self.dcc, self.game_world.as_mut()) {
            let mut context = gw.as_engine_context(services);
            dcc.update_agents(&mut context);
        }

        if should_log_summary {
            self.log_telemetry_summary();
        }
    }
}

/// The main entry point for the Khora Engine.
pub struct Engine;

impl Engine {
    /// Runs the engine with the specified application.
    ///
    /// This is the primary entry point for game developers.
    /// All engine systems are initialized and managed internally.
    pub fn run<A: Application>() -> Result<()> {
        log::info!("Khora Engine SDK: Starting...");
        let event_loop = EventLoop::new()?;

        let mut app_state = EngineState::<A> {
            app: None,
            game_world: None,
            window: None,
            renderer: None,
            graphics_device: None,
            telemetry: None,
            dcc: None,
            render_settings: RenderSettings::default(),
            simulation_started: false,
            running: Arc::new(AtomicBool::new(true)),
            input_events: VecDeque::new(),
        };

        event_loop.run_app(&mut app_state)?;
        Ok(())
    }
}
