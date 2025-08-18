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
//! This crate provides a simple and stable API for game developers to create
//! and run applications using Khora.

use anyhow::Result;
use khora_core::platform::window::KhoraWindow;
use khora_core::renderer::{RenderObject, RenderSettings, RenderSystem};
use khora_core::telemetry::MonitoredResourceType;
use khora_infra::platform::input::translate_winit_input;
use khora_infra::platform::window::{WinitWindow, WinitWindowBuilder};
use khora_infra::telemetry::memory_monitor::MemoryMonitor;
use khora_infra::{GpuMonitor, WgpuRenderSystem};
use khora_telemetry::TelemetryService;
use std::sync::Arc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

pub mod prelude {
    pub use khora_core::renderer::{
        RenderObject,
        BufferDescriptor, BufferId, BufferUsage, ColorTargetStateDescriptor, ColorWrites,
        MultisampleStateDescriptor, PipelineLayoutDescriptor, RenderPipelineDescriptor,
        RenderPipelineId, SampleCount, ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData,
        ShaderStage, VertexAttributeDescriptor, VertexBufferLayoutDescriptor, VertexFormat,
        VertexStepMode, IndexFormat
    };
}

pub struct EngineContext<'a> {
    pub graphics_device: &'a dyn khora_core::renderer::GraphicsDevice,
}

pub trait Application: Sized + 'static {
    /// Called once at the beginning of the application to create the initial state.
    fn new(context: EngineContext) -> Self;

    /// Called every frame for game logic updates.
    fn update(&mut self);

    /// Called every frame to handle rendering.
    fn render(&mut self) -> Vec<RenderObject>;
}

/// The internal state of the running engine, managed by the winit event loop.
/// It now holds the user's application state (`app: A`).
struct EngineState<A: Application> {
    app: Option<A>, // The user's application logic and data.
    window: Option<WinitWindow>,
    renderer: Option<Box<dyn RenderSystem>>,
    telemetry: Option<TelemetryService>,
    render_settings: RenderSettings,
}

impl<A: Application> EngineState<A> {
    /// Logs a summary of all registered telemetry monitors to the console.
    fn log_telemetry_summary(&self) {
        if let Some(telemetry) = &self.telemetry {
            log::info!("--- Telemetry Summary ---");
            let monitors = telemetry.monitor_registry().get_all_monitors();

            if monitors.is_empty() {
                log::info!("  No monitors registered.");
            }

            for monitor in monitors {
                let report = monitor.get_usage_report();
                match monitor.resource_type() {
                    MonitoredResourceType::SystemRam => {
                        let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                        let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                        log::info!(
                            "  RAM Usage: {:.2} MB (Peak: {:.2} MB)",
                            current_mb,
                            peak_mb
                        );
                    }
                    MonitoredResourceType::Vram => {
                        let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                        let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                        log::info!(
                            "  VRAM Usage: {:.2} MB (Peak: {:.2} MB)",
                            current_mb,
                            peak_mb
                        );
                    }
                    MonitoredResourceType::Gpu => {
                        // Downcast to the concrete GpuMonitor type to access detailed reports.
                        if let Some(gpu_monitor) = monitor.as_any().downcast_ref::<GpuMonitor>() {
                            if let Some(gpu_report) = gpu_monitor.get_gpu_report() {
                                log::info!(
                                    "  GPU Time: {:.3} ms (Main Pass: {:.3} ms)",
                                    gpu_report.frame_total_duration_us().unwrap_or(0) as f32
                                        / 1000.0,
                                    gpu_report.main_pass_duration_us().unwrap_or(0) as f32 / 1000.0
                                );
                            }
                        }
                    }
                }
            }
            log::info!("-------------------------");
        }
    }
}

/// Implementing `Drop` is the idiomatic Rust way to handle cleanup.
/// When `EngineState` goes out of scope (after the event loop exits), this `drop`
/// function will be called automatically, ensuring a controlled shutdown.
impl<A: Application> Drop for EngineState<A> {
    fn drop(&mut self) {
        log::info!("EngineState is being dropped. Performing controlled shutdown...");

        if let Some(mut renderer) = self.renderer.take() {
            renderer.shutdown();
        }

        log::info!("Engine systems shutdown complete.");
    }
}

impl<A: Application> ApplicationHandler for EngineState<A> {
    /// Called when the event loop is ready to start processing events.
    /// This is the ideal place to initialize systems that require a window.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // Avoid re-initializing if the app is resumed multiple times.
        }

        log::info!("Application resumed. Initializing window and engine systems...");

        // 1. Create the window using the builder from khora-infra.
        let window = WinitWindowBuilder::new().build(event_loop).unwrap();

        // 2. Create the renderer and get its associated resource monitors.
        let mut renderer: Box<dyn RenderSystem> = Box::new(WgpuRenderSystem::new());
        let renderer_monitors = renderer.init(&window).unwrap();

        // 3. Create the telemetry service.
        let telemetry = TelemetryService::new(Duration::from_secs(1));

        // 4. Register all available default monitors with the telemetry service.
        log::info!("Registering default resource monitors...");

        // Register the monitors that were created and returned by the renderer.
        for monitor in renderer_monitors {
            telemetry.monitor_registry().register(monitor);
        }

        // Register other independent monitors.
        let memory_monitor = Arc::new(MemoryMonitor::new("System_RAM".to_string()));
        telemetry.monitor_registry().register(memory_monitor);

        // 5. Create the application instance.
        let context = EngineContext {
            graphics_device: renderer.graphics_device(),
        };
        self.app = Some(A::new(context));

        // 6. Store the initialized systems in our application state.
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.telemetry = Some(telemetry);
        self.render_settings = RenderSettings::default();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(app_window) = self.window.as_ref() {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let event_window_hash = hasher.finish();

            if app_window.id() == event_window_hash {
                match event {
                    WindowEvent::CloseRequested => {
                        log::info!("Shutdown requested, exiting event loop...");
                        event_loop.exit();
                    }
                    WindowEvent::Resized(size) => {
                        if let Some(renderer) = self.renderer.as_mut() {
                            log::info!("Window resized to: {}x{}", size.width, size.height);
                            renderer.resize(size.width, size.height);
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        if let (Some(renderer), Some(telemetry)) =
                            (self.renderer.as_mut(), self.telemetry.as_mut())
                        {
                            // Update "active" monitors like the memory monitor.
                            let should_log_summary = telemetry.tick();

                            // Call the user's application update and render methods.
                            let app = self.app.as_mut().unwrap();

                            app.update();
                            
                            let render_objects = app.render();

                            // The renderer will update its own internal monitors (like GpuMonitor) during this call.
                            match renderer.render(&render_objects, &Default::default(), &self.render_settings) {
                                Ok(stats) => {
                                    log::trace!("Frame {} rendered.", stats.frame_number);
                                }
                                Err(e) => log::error!("Rendering error: {}", e),
                            }

                            if should_log_summary {
                                self.log_telemetry_summary();
                            }
                        }
                    }
                    _ => {
                        // Translate winit events into our engine's event type for game logic to consume.
                        if let Some(input_event) = translate_winit_input(&event) {
                            log::debug!("Input event: {:?}", input_event);
                        }
                    }
                }
            }
        }
    }

    /// Called when the event loop has processed all pending events and is about to wait.
    /// This is the ideal place to request a redraw for continuous rendering (i.e., a game loop).
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// The public entry point for the Khora Engine.
pub struct Engine;

impl Engine {
    /// Creates a new engine instance and runs it.
    ///
    /// This is the primary function for a game developer to call. It will create a window,
    /// initialize the rendering and other core systems, and start the main event loop,
    /// blocking the current thread until the application is closed.
    pub fn run<A: Application>() -> Result<()> {
        log::info!("Khora Engine SDK: Starting...");
        let event_loop = EventLoop::new()?;

        // The initial state is empty; it will be populated in the `resumed` event.
        let mut app_state = EngineState::<A> {
            app: None,
            window: None,
            renderer: None,
            telemetry: None,
            render_settings: RenderSettings::default(),
        };

        event_loop.run_app(&mut app_state)?;

        Ok(())
    }
}
