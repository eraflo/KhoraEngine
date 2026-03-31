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
use khora_agents::ui_agent::UiAgent;
use khora_control::{DccConfig, DccService};
use khora_core::asset::font::Font;
use khora_core::platform::KhoraWindow;
use khora_core::renderer::api::core::RenderSettings;
use khora_core::renderer::api::scene::RenderObject;
use khora_core::renderer::traits::RenderSystem;
use khora_core::telemetry::MonitoredResourceType;
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::editor::EditorCamera;
use khora_core::ui::editor_overlay::{EditorOverlay, OverlayScreenDescriptor};
use khora_core::ui::EditorShell;
use khora_core::ServiceRegistry;
use khora_data::assets::Assets;
use khora_infra::platform::input::translate_winit_input;
use khora_infra::platform::window::{WinitWindow, WinitWindowBuilder};
use khora_infra::telemetry::memory_monitor::MemoryMonitor;
use khora_infra::{GpuMonitor, StandardTextRenderer, TaffyLayoutSystem, WgpuRenderSystem};
use khora_telemetry::TelemetryService;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

pub mod prelude {
    //! Common imports for game development.
    //!
    //! This prelude contains the types most game developers need.
    //! For advanced rendering, use `khora_sdk::rendering`.
    //! For editor integration, use `khora_sdk::editor`.

    // SDK types
    pub use crate::{WindowConfig, WindowIcon, PRIMARY_VIEWPORT};

    // Assets
    pub use khora_core::asset::{AssetHandle, AssetUUID};

    // Memory tracking (for `#[global_allocator]`)
    pub use khora_core::memory::SaaTrackingAllocator;

    // Input
    pub use khora_infra::platform::input::{InputEvent, MouseButton};

    // ECS types
    pub mod ecs {
        //! Core ECS types for game logic.
        pub use khora_core::ecs::entity::EntityId;
        pub use khora_core::physics::{BodyType, ColliderShape};
        pub use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
        pub use khora_data::ecs::{
            AudioSource, Camera, Children, Collider, Component, ComponentBundle, GlobalTransform,
            Light, MaterialComponent, Name, Parent, ProjectionType, RigidBody, Transform, Without,
        };
    }

    // Materials
    pub mod materials {
        //! Built-in material types.
        pub use khora_core::asset::{
            EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
        };
    }

    // Math
    pub mod math {
        //! Math types and utilities.
        pub use khora_core::math::*;
    }
}

pub use khora_infra::platform::input::InputEvent;

/// Well-known viewport handle for the primary editor 3D viewport.
pub const PRIMARY_VIEWPORT: ViewportTextureHandle = ViewportTextureHandle(0);

/// Raw window icon data for native window creation.
#[derive(Clone, Debug)]
pub struct WindowIcon {
    /// RGBA8 pixel buffer stored row-major.
    pub rgba: Vec<u8>,
    /// Icon width in pixels.
    pub width: u32,
    /// Icon height in pixels.
    pub height: u32,
}

/// Window configuration for applications.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Window title shown by the platform window manager.
    pub title: String,
    /// Initial window width in pixels.
    pub width: u32,
    /// Initial window height in pixels.
    pub height: u32,
    /// Optional custom window icon.
    pub icon: Option<WindowIcon>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Khora Engine".to_owned(),
            width: 1024,
            height: 768,
            icon: None,
        }
    }
}

/// Application context provided during setup.
///
/// Gives game developers access to engine services (graphics, audio, etc.)
/// without exposing internal engine types like `EngineContext`.
pub struct AppContext {
    /// The engine service registry.
    pub services: ServiceRegistry,
}

/// Application trait for user-defined game logic.
///
/// The engine manages the internal state. Users interact through
/// `&mut GameWorld` - no direct access to internal engine types.
pub trait Application: Sized + 'static {
    /// Returns the default window configuration for this application.
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }

    /// Called once at initialization. Create your application struct here.
    fn new() -> Self;

    /// Called once after construction for scene setup.
    ///
    /// Use `ctx.services` to access engine services (graphics device, etc.)
    /// and cache them in your application struct.
    fn setup(&mut self, _world: &mut GameWorld, _ctx: &mut AppContext) {}

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
    /// Editor overlay (egui), if present.
    editor_overlay: Option<Box<dyn EditorOverlay>>,
    /// Editor shell (dock layout, menu, toolbar).
    editor_shell: Option<Arc<Mutex<Box<dyn EditorShell>>>>,
    /// Editor camera for the 3D viewport.
    editor_camera: Arc<Mutex<EditorCamera>>,
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

        let window_config = A::window_config();
        let mut window_builder = WinitWindowBuilder::new()
            .with_title(window_config.title)
            .with_dimensions(window_config.width, window_config.height);

        if let Some(icon) = window_config.icon {
            window_builder = window_builder.with_icon_rgba(icon.rgba, icon.width, icon.height);
        }

        let window = match window_builder.build(event_loop) {
            Ok(window) => window,
            Err(e) => {
                log::error!("Failed to create window: {e}");
                return;
            }
        };
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

        // Create the editor overlay + shell (egui) if the backend supports it.
        let (editor_overlay, editor_shell) =
            if let Some(wgpu_rs) = renderer.as_any_mut().downcast_mut::<WgpuRenderSystem>() {
                let theme = khora_core::ui::editor::EditorTheme::default();
                match wgpu_rs.create_editor_overlay_and_shell(
                    event_loop,
                    khora_lanes::render_lane::shaders::EGUI_WGSL,
                    khora_lanes::render_lane::shaders::GRID_WGSL,
                    theme,
                    PRIMARY_VIEWPORT,
                ) {
                    Ok((overlay, shell)) => {
                        log::info!("Editor overlay + shell created successfully.");
                        (
                            Some(Box::new(overlay) as Box<dyn EditorOverlay>),
                            Some(Arc::new(
                                Mutex::new(Box::new(shell) as Box<dyn EditorShell>),
                            )),
                        )
                    }
                    Err(e) => {
                        log::warn!("Failed to create editor overlay: {e}. Continuing without it.");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            };

        // Build an AppContext for Application::setup() with the core engine services.
        let mut services = ServiceRegistry::new();
        services.insert(graphics_device.clone());

        // Register UI and Text services
        let text_renderer = Arc::new(StandardTextRenderer::new(
            khora_lanes::render_lane::shaders::TEXT_WGSL.to_string(),
        ));
        services.insert(text_renderer as Arc<dyn khora_core::renderer::api::text::TextRenderer>);

        let layout_system = Arc::new(Mutex::new(
            Box::new(TaffyLayoutSystem::new()) as Box<dyn khora_core::ui::LayoutSystem>
        ));
        services.insert(layout_system);

        let font_assets = Arc::new(RwLock::new(Assets::<Font>::new()));
        services.insert(font_assets);

        // Register the editor shell if overlay+shell are available.
        if let Some(shell) = &editor_shell {
            services.insert(Arc::clone(shell));
        }

        // Register the editor camera (shared) and viewport handle.
        let editor_camera_shared: Arc<Mutex<EditorCamera>> =
            Arc::new(Mutex::new(EditorCamera::default()));
        services.insert(editor_camera_shared.clone());
        services.insert(PRIMARY_VIEWPORT);

        let mut app_ctx = AppContext { services };
        let mut app = A::new();
        let mut game_world = GameWorld::new();
        app.setup(&mut game_world, &mut app_ctx);

        // Register and initialize default agents with their execution priorities.
        // Higher priority = executed first in the update loop
        Self::register_default_agents(&dcc, graphics_device.clone());

        // Initialize all agents once with the core engine services.
        // This gives agents a chance to cache services and set up lanes
        // before the first frame, rather than lazy-initializing in execute().
        {
            let mut init_services = ServiceRegistry::new();
            init_services.insert(graphics_device.clone());

            // Re-register UI services for agent initialization.
            let text_renderer_init: Arc<dyn khora_core::renderer::api::text::TextRenderer> =
                Arc::new(StandardTextRenderer::new(
                    khora_lanes::render_lane::shaders::TEXT_WGSL.to_string(),
                ));
            init_services.insert(text_renderer_init);

            let layout_system_init: Arc<Mutex<Box<dyn khora_core::ui::LayoutSystem>>> =
                Arc::new(Mutex::new(
                    Box::new(TaffyLayoutSystem::new()) as Box<dyn khora_core::ui::LayoutSystem>
                ));
            init_services.insert(layout_system_init);

            let font_assets_init: Arc<RwLock<Assets<Font>>> =
                Arc::new(RwLock::new(Assets::<Font>::new()));
            init_services.insert(font_assets_init);

            let mut init_ctx = khora_core::EngineContext {
                world: None,
                services: init_services,
            };
            dcc.initialize_agents(&mut init_ctx);
        }

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
        self.editor_overlay = editor_overlay;
        self.editor_shell = editor_shell;
        self.editor_camera = editor_camera_shared;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Forward event to the editor overlay first. If consumed, skip game input.
        let _overlay_consumed =
            if let (Some(overlay), Some(window)) = (&mut self.editor_overlay, &self.window) {
                overlay.handle_window_event(
                    window.winit_window() as &dyn std::any::Any,
                    &event as &dyn std::any::Any,
                )
            } else {
                false
            };

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
                // Always forward input events to the Application so the
                // editor camera and other handlers can process them.
                // The Application decides whether to act based on
                // viewport hover state (overlay_consumed is true whenever
                // the pointer is over any egui panel, which covers the
                // entire window including the 3D viewport).
                if let Some(input_event) = translate_winit_input(&event) {
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
        // UI: 1.1 (high priority for responsiveness)

        let render_agent = khora_agents::render_agent::RenderAgent::new()
            .with_telemetry_sender(dcc.event_sender());
        dcc.register_agent(Arc::new(Mutex::new(render_agent)), 1.0);

        let ui_agent = UiAgent::new();
        dcc.register_agent(Arc::new(Mutex::new(ui_agent)), 1.1);

        let physics_provider =
            Box::new(khora_infra::physics::rapier::RapierPhysicsWorld::default());
        let physics_agent = khora_agents::physics_agent::PhysicsAgent::new(physics_provider);
        dcc.register_agent(Arc::new(Mutex::new(physics_agent)), 0.9);

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

        // Build the overlay screen descriptor from the window.
        let screen = self.window.as_ref().map(|w| {
            let (w_px, h_px) = w.inner_size();
            OverlayScreenDescriptor {
                width_px: w_px,
                height_px: h_px,
                scale_factor: w.scale_factor() as f32,
            }
        });

        // Begin the egui frame (overlay collects input and starts a UI pass).
        if let (Some(overlay), Some(window), Some(s)) =
            (&mut self.editor_overlay, &self.window, &screen)
        {
            overlay.begin_frame(window.winit_window() as &dyn std::any::Any, *s);
        }
        // Render the editor shell (menu bar, toolbar, docked panels).
        if let Some(shell) = &self.editor_shell {
            if let Ok(mut s) = shell.lock() {
                s.show_frame();
            }
        }
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
        if let (Some(dcc), Some(gw)) = (&self.dcc, self.game_world.as_mut()) {
            // Acquire the swapchain texture once for all agents this frame.
            if let Ok(mut rs) = renderer.lock() {
                if let Err(e) = rs.begin_frame() {
                    log::error!("begin_frame failed: {e:?}");
                    return;
                }

                let mut editor_view_info = None;

                // Render the offscreen viewport (clear + grid).
                if let Some(wgpu_rs) = rs.as_any_mut().downcast_mut::<WgpuRenderSystem>() {
                    let clear = khora_core::math::LinearRgba::new(0.15, 0.15, 0.18, 1.0);
                    let (vw, vh) = wgpu_rs.viewport_size();
                    let vi = if let Ok(cam) = self.editor_camera.lock() {
                        cam.view_info(vw as f32, vh as f32)
                    } else {
                        khora_core::renderer::api::resource::ViewInfo::default()
                    };
                    if let Err(e) = wgpu_rs.render_viewport(clear, &vi) {
                        log::error!("viewport render failed: {e:?}");
                    }

                    editor_view_info = Some(vi);
                }

                // Seed the main render view with the editor camera.
                // The RenderAgent may override this only when an active scene
                // camera exists (e.g. play mode).
                if let Some(vi) = editor_view_info.as_ref() {
                    rs.prepare_frame(vi);
                }

                // Redirect agent rendering to the offscreen viewport texture.
                rs.set_render_to_viewport(true);
            }

            gw.tick_maintenance();
            let mut context = gw.as_engine_context(services);
            dcc.execute_agents(&mut context);

            // Render the editor overlay on top of the 3D scene.
            if let (Some(overlay), Some(s)) = (&mut self.editor_overlay, screen) {
                if let Ok(mut rs) = renderer.lock() {
                    // Switch back to swapchain for the overlay pass.
                    rs.set_render_to_viewport(false);
                    if let Err(e) = rs.render_overlay(overlay.as_mut(), s) {
                        log::error!("render_overlay failed: {e:?}");
                    }
                }
            }

            // Present the single swapchain texture after all agents have encoded.
            if let Ok(mut rs) = renderer.lock() {
                if let Err(e) = rs.end_frame() {
                    log::error!("end_frame failed: {e:?}");
                }
            }
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
            editor_overlay: None,
            editor_shell: None,
            editor_camera: Arc::new(Mutex::new(EditorCamera::default())),
        };

        event_loop.run_app(&mut app_state)?;
        Ok(())
    }
}
