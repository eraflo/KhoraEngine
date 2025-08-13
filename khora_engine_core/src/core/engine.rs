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

use crate::core::metrics::engine::{EngineMetrics, FrameStats};
use crate::core::metrics::scheduler::MetricsScheduler;
use crate::core::timer::Stopwatch;
use crate::event::{EngineEvent, EventBus};
use crate::memory::get_currently_allocated_bytes;
use crate::subsystems::input as KhoraInputSubsystem;
use crate::subsystems::renderer::{
    RenderError, RenderObject, RenderSettings, RenderSystem, ViewInfo, WgpuRenderSystem,
};
use crate::window::KhoraWindow;

use flume::Receiver;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

/// Represents the main engine structure, responsible for orchestrating subsystems.
#[derive(Debug)]
pub struct Engine {
    is_running: bool,
    event_bus: EventBus,
    window: Option<KhoraWindow>,
    render_system: Option<Box<dyn RenderSystem>>,

    // Metrics system
    engine_metrics: EngineMetrics,
    metrics_scheduler: MetricsScheduler,

    // Timers and counters
    frame_count: u64,
    last_stats_time: Stopwatch,
    frames_since_last_log: u32,
    log_interval_secs: f64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Creates a new instance of the Engine.
    /// ## Returns
    /// A new instance of the Engine struct.
    pub fn new() -> Self {
        Self {
            is_running: false,
            event_bus: EventBus::default(),
            window: None,
            render_system: Some(Box::new(WgpuRenderSystem::new())),

            // Initialize metrics system with default configuration
            engine_metrics: EngineMetrics::with_default_config(),
            metrics_scheduler: MetricsScheduler::with_default_interval(),

            // Initialize stats counters
            frame_count: 0,
            last_stats_time: Stopwatch::new(),
            frames_since_last_log: 0,
            log_interval_secs: 1.0,
        }
    }

    /// Initializes the metrics system and registers core engine metrics.
    /// Note: With the new configuration-based approach, metrics are automatically
    /// initialized when creating EngineMetrics::with_default_config().
    pub fn initialize_metrics(&mut self) {
        // Metrics are already initialized with default config in new()
        if !self.engine_metrics.is_initialized() {
            log::warn!("Engine metrics were not initialized properly");
        } else {
            log::info!("Engine metrics initialized successfully");
        }
    }

    /// Updates engine metrics with current performance data.
    pub fn update_metrics(&mut self, frame_time_ms: f64, cpu_time_ms: f64, gpu_time_ms: f64) {
        self.engine_metrics
            .update_basic(frame_time_ms, cpu_time_ms, gpu_time_ms);
    }

    /// Updates all engine metrics with comprehensive frame statistics.
    pub fn update_all_metrics(&mut self, stats: &FrameStats) {
        self.engine_metrics.update_all(stats);
    }

    /// Gets a snapshot of all engine metrics for monitoring and debugging.
    pub fn get_metrics_snapshot(&self) -> Vec<String> {
        self.engine_metrics.get_metrics_snapshot()
    }

    /// Logs a comprehensive metrics summary from the metrics system.
    pub fn log_metrics_summary(&self) {
        self.engine_metrics.log_metrics_summary()
    }

    /// Sets up the engine, initializing subsystems and preparing for the main loop.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn setup(&mut self) {
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .parse_default_env()
            .init();

        log::info!("Engine setup commencing...");
        log::info!("Memory tracking allocator active.");

        // Initialize metrics system
        self.initialize_metrics();
        log::info!("Metrics system initialized.");

        // TODO: Initialize other subsystems (ECS, audio, etc.)

        log::info!("Engine setup complete.");
    }

    /// Starts the main event loop for the engine.
    /// ## Arguments
    /// * `mut self` - Takes ownership of the Engine instance to move into the handler.
    pub fn run(mut self) {
        log::info!("Initializing windowing system and starting engine loop...");
        let event_loop = EventLoop::new().expect("Failed to create event loop");

        // Prepare the initial state for the ApplicationHandler
        self.is_running = true;
        self.last_stats_time = Stopwatch::new();
        self.metrics_scheduler.reset();

        let mut app_handler = EngineAppHandler { engine: self };

        if let Err(e) = event_loop.run_app(&mut app_handler) {
            log::error!("Event loop exited with error: {e}");
        }
    }

    /// Cleans up resources and subsystems before exiting the engine.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn shutdown(&mut self) {
        if !self.is_running && self.window.is_none() && self.render_system.is_none() {
            log::debug!("Shutdown called, but engine appears already stopped/cleaned up.");
            return;
        }
        log::info!("Shutting down engine...");

        if let Some(rs) = self.render_system.as_mut() {
            rs.shutdown();
        }

        self.render_system = None;
        log::info!("Render system shut down and dropped.");

        self.window = None;
        log::info!("Window wrapper dropped.");

        // TODO: Shutdown other subsystems

        log::info!(
            "Engine shutdown complete. Final memory usage: {} KiB",
            get_currently_allocated_bytes() / 1024
        );
    }

    /// Returns the event bus sender for publishing events.
    /// ## Arguments
    /// * `&self` - A reference to the Engine instance.
    pub fn event_sender(&self) -> flume::Sender<EngineEvent> {
        self.event_bus.sender()
    }
}

/// Represents the application handler for the engine, implementing the ApplicationHandler trait.
#[derive(Debug)]
struct EngineAppHandler {
    engine: Engine,
}

impl ApplicationHandler<()> for EngineAppHandler {
    /// Called when the application is first launched.
    /// This is where the main window is created and initialized.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the EngineAppHandler instance.
    /// * `event_loop` - A reference to the ActiveEventLoop instance.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.window.is_none() {
            log::info!(
                "EngineAppHandler::resumed: Creating window and initializing RenderSystem..."
            );

            match KhoraWindow::new(event_loop) {
                Ok(win_wrapper) => {
                    self.engine.window = Some(win_wrapper);

                    // Initialize the RenderSystem now that a window is available.
                    if let (Some(rs), Some(window_ref)) =
                        (self.engine.render_system.as_mut(), &self.engine.window)
                    {
                        log::info!("Initializing RenderSystem with the window...");

                        if let Err(e) = rs.init(window_ref) {
                            log::error!("FATAL: Failed to initialize RenderSystem: {e:?}");
                            event_loop.exit();
                            return;
                        }

                        log::info!("RenderSystem initialized successfully.");

                        if let Some(adapter_info) = rs.get_adapter_info() {
                            log::info!(
                                "Engine: Using Adapter: '{}', Backend: {:?}, Type: {:?}",
                                adapter_info.name,
                                adapter_info.backend_type,
                                adapter_info.device_type
                            );
                        }
                    } else {
                        log::error!(
                            "FATAL: RenderSystem or Window is None during resume. Cannot initialize rendering."
                        );
                        event_loop.exit();
                    }
                }
                Err(e) => {
                    log::error!("FATAL: Failed to create KhoraWindow: {e}");
                    event_loop.exit();
                }
            }
        } else {
            log::info!(
                "EngineAppHandler::resumed: Window and RenderSystem likely already exist/initialized."
            );

            if self.engine.render_system.is_none() {
                log::error!(
                    "Inconsistent state: Window exists but RenderSystem is None on resume!"
                );
                event_loop.exit();
            }
        }
    }

    /// Handles events specific to a window.
    /// This is where we handle window events such as resizing, closing, etc.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the EngineAppHandler instance.
    /// * `event_loop` - A reference to the ActiveEventLoop instance.
    /// * `window_id` - The ID of the window that received the event.
    /// * `event` - The window event that occurred.
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let engine = &mut self.engine;

        // Process event only if window exists and ID matches
        if let Some(khora_window) = &engine.window
            && khora_window.id() == window_id
        {
            // --- Translate Input Events ---
            if let Some(input_event) = KhoraInputSubsystem::translate_winit_input(&event) {
                engine.event_bus.publish(EngineEvent::Input(input_event));
                return;
            }

            // --- Handle Non-Input Window Events ---
            match event {
                // Case where the window is closed by the user
                WindowEvent::CloseRequested => {
                    log::info!("Window Close Requested event received.");
                    engine.event_bus.publish(EngineEvent::ShutdownRequested);
                }

                // Case where the window is resized by the user
                WindowEvent::Resized(physical_size) => {
                    engine.event_bus.publish(EngineEvent::WindowResized {
                        width: physical_size.width,
                        height: physical_size.height,
                    });
                }

                // Case where the window is redrawn (e.g., after resizing or when requested)
                WindowEvent::RedrawRequested => {
                    log::trace!("Window Redraw Requested for id: {window_id:?}");

                    // Increment frame counters BEFORE rendering
                    engine.frame_count += 1;
                    engine.frames_since_last_log += 1;

                    // Increment frame counter in metrics (every frame)
                    engine.engine_metrics.increment_counter("frame_counter", 1);

                    // --- Render Phase ---
                    let render_time = Stopwatch::new();
                    engine.perform_render_frame();

                    let render_duration_us = render_time.elapsed_us().unwrap_or(0);

                    // --- Stats Logging ---
                    let time_since_last_log =
                        engine.last_stats_time.elapsed_secs_f64().unwrap_or(0.0);

                    if time_since_last_log >= engine.log_interval_secs {
                        let fps = if time_since_last_log > 0.0 {
                            (engine.frames_since_last_log as f64 / time_since_last_log) as u32
                        } else {
                            0 // Avoid division by zero
                        };

                        // Calculate memory usage in bytes
                        let memory_usage_kib = get_currently_allocated_bytes() / 1024;

                        let (gpu_main_pass_ms, gpu_frame_total_ms, draw_calls, triangles) = engine
                            .render_system
                            .as_ref()
                            .map_or((0.0f32, 0.0f32, 0u32, 0u32), |rs| {
                                let stats = rs.get_last_frame_stats();
                                (
                                    stats.gpu_main_pass_time_ms,
                                    stats.gpu_frame_total_time_ms,
                                    stats.draw_calls,
                                    stats.triangles_rendered,
                                )
                            });

                        // Update metrics with current frame data
                        let frame_stats = FrameStats {
                            fps,
                            memory_usage_kib: memory_usage_kib as u64,
                            render_duration_us,
                            gpu_main_pass_ms,
                            gpu_frame_total_ms,
                            draw_calls,
                            triangles,
                        };
                        engine.update_all_metrics(&frame_stats);

                        log::info!(
                            "Stats | Frame:{} FPS:{} Mem:{} KiB | CPU:{} us | GPU Main:{:.2} ms | GPU Frame:{:.2} ms | {} draws, {} tris",
                            engine.frame_count,
                            fps,
                            memory_usage_kib,
                            render_duration_us,
                            gpu_main_pass_ms,
                            gpu_frame_total_ms,
                            draw_calls,
                            triangles
                        );

                        // Log comprehensive metrics summary every 10 seconds based on actual time
                        if engine.metrics_scheduler.should_log_summary() {
                            engine.log_metrics_summary();
                            engine.metrics_scheduler.mark_summary_logged();
                        }

                        engine.last_stats_time = Stopwatch::new();
                        engine.frames_since_last_log = 0;
                    }
                }

                // Case where the window is moved by the user
                WindowEvent::Focused(focused) => {
                    log::debug!("Window Focus Changed: {focused}");
                    // TODO: Publish focus change event if needed
                }
                _ => {}
            }
        }
    }

    /// Called every iteration after all OS events have been processed, before waiting.
    /// This is where we can process internal events and update the engine state.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the EngineAppHandler instance.
    /// * `event_loop` - A reference to the ActiveEventLoop instance.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let engine = &mut self.engine;

        engine.process_internal_events();

        // Check if shutdown was requested internally
        if !engine.is_running {
            log::info!("Shutdown requested via internal event, exiting loop.");
            event_loop.exit();
            return;
        }

        // --- Update Phase ---
        engine.update();

        // Request a redraw
        if let Some(khora_window) = &engine.window {
            khora_window.request_redraw();
        }
    }

    /// Called just before the application exits
    /// This is where we can perform any final cleanup or resource deallocation.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the EngineAppHandler instance.
    /// * `event_loop` - A reference to the ActiveEventLoop instance.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!("Application exiting event received.");

        self.engine.shutdown();
    }
}

impl Engine {
    /// Processes internal EngineEvents received from the EventBus
    /// This function collects events from the event bus and processes them in a batch.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    fn process_internal_events(&mut self) {
        let mut events_to_process: Vec<EngineEvent> = Vec::new();
        let receiver: &Receiver<EngineEvent> = self.event_bus.receiver();

        while let Ok(event) = receiver.try_recv() {
            events_to_process.push(event);
        }

        // Process collected events
        for event in events_to_process {
            log::trace!("Processing internal event: {event:?}");
            self.handle_internal_event(event);
        }
    }

    /// Performs the rendering of a single frame.
    fn perform_render_frame(&mut self) {
        if let Some(rs) = self.render_system.as_mut() {
            let view_info = ViewInfo::default();
            let render_objects: [RenderObject; 0] = [];
            let render_settings: RenderSettings = RenderSettings::default();

            match rs.render(&render_objects, &view_info, &render_settings) {
                Ok(_render_stats) => {
                    log::trace!("Engine: Frame rendered by RenderSystem. Stats: {_render_stats:?}");
                }
                Err(RenderError::SurfaceAcquisitionFailed(msg)) => {
                    log::warn!(
                        "RenderSystem reported surface acquisition failure: {msg}. Attempting resize."
                    );

                    if msg.contains("Lost") || msg.contains("Outdated") {
                        log::warn!(
                            "RenderSystem surface acquisition lost/outdated. Attempting to resize."
                        );

                        if let Some(win) = &self.window {
                            rs.resize(win.inner_size().0, win.inner_size().1);
                        }
                    } else if msg.contains("OutOfMemory") {
                        log::error!(
                            "RenderSystem ran out of memory for surface! Requesting shutdown: {msg}"
                        );

                        self.event_bus.publish(EngineEvent::ShutdownRequested);
                    }
                }
                Err(e) => {
                    log::error!("Engine: Error during RenderSystem::render_to_window: {e:?}");
                    self.event_bus.publish(EngineEvent::ShutdownRequested);
                }
            }
        } else if self.window.is_some() {
            log::warn!("Engine::perform_render_frame called but RenderSystem is not available!");
        }
    }

    /// Handles a single internal EngineEvent.
    /// This function is responsible for processing specific types of events that are internal to the engine.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    /// * `event` - The EngineEvent to handle.
    fn handle_internal_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::WindowResized { width, height } => {
                log::info!("Internal Handling: Window resized: {width}x{height}");

                if width > 0 && height > 0 {
                    // Resize the render system
                    if let Some(rs) = self.render_system.as_mut() {
                        rs.resize(width, height);
                    }
                } else {
                    log::warn!(
                        "Internal Handling: Window resized to zero size ({width}x{height}), not resizing render surface yet."
                    );
                }

                // TODO: Notify update Camera aspect ratio, ui, etc.
            }
            EngineEvent::Input(input_event) => {
                log::trace!("Internal Handling: Input event: {input_event:?}");
                // TODO: Update input state manager, dispatch to UI / game logic listeners
            }
            EngineEvent::ShutdownRequested => {
                log::info!("Internal Handling: Shutdown requested.");
                self.is_running = false;
            }
        }
    }

    /// Engine's update logic
    /// This function is called every frame before rendering.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    fn update(&mut self) {
        // TODO: Implement actual update logic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::input::InputEvent;

    /// Test the Engine's creation and initial state
    #[test]
    fn engine_creation_initial_state() {
        let engine = Engine::new();
        assert!(!engine.is_running, "Engine should not be running initially");
        assert!(
            engine.render_system.is_some(),
            "RenderSystem should be Some (boxed WgpuRenderer) initially."
        );
        assert!(engine.window.is_none(), "Window should be None initially");
        assert_eq!(engine.frame_count, 0, "Frame count should be 0 initially");
    }

    /// Test the Engine's setup method
    #[test]
    fn handle_internal_event_shutdown_sets_is_running_false() {
        let mut engine = Engine::new();
        engine.is_running = true;

        let event = EngineEvent::ShutdownRequested;
        engine.handle_internal_event(event);

        assert!(
            !engine.is_running,
            "Engine should not be running after ShutdownRequested"
        );
    }

    /// Test the Engine's resizing method
    #[test]
    fn handle_internal_event_resize_does_not_panic() {
        let mut engine = Engine::new();
        engine.is_running = true;
        let event = EngineEvent::WindowResized {
            width: 800,
            height: 600,
        };

        engine.handle_internal_event(event);
    }

    /// Test the Engine's input handling method
    #[test]
    fn handle_internal_event_input_does_not_panic() {
        let mut engine = Engine::new();
        engine.is_running = true;
        let event = EngineEvent::Input(InputEvent::KeyPressed {
            key_code: "KeyA".to_string(),
        });

        engine.handle_internal_event(event);
    }
}
