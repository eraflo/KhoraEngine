
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId
};
use crate::window::KhoraWindow;
use crate::subsystems::input as KhoraInputSubsystem;
use crate::memory::get_currently_allocated_bytes;
use crate::{core::timer::Stopwatch, event::{EngineEvent, EventBus}};


/// Represents the main engine structure, responsible for orchestrating subsystems.
#[derive(Debug, Default)]
pub struct Engine {
    is_running: bool,
    event_bus: EventBus,
    window: Option<KhoraWindow>,

    // Timers and counters
    frame_count: u64,
    last_stats_time: Stopwatch,
    frames_since_last_log: u32,
    log_interval_secs: f64,
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

            // Initialize stats counters
            frame_count: 0,
            last_stats_time: Stopwatch::new(),
            frames_since_last_log: 0,
            log_interval_secs: 1.0,
        }
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
        self.last_stats_time = Stopwatch::new(); // Reset the stats timer

        // Create the handler struct which holds the Engine state
        let mut app_handler = EngineAppHandler { engine: self };

        // Start the event loop using run_app
        // run_app takes ownership of the handler and the event loop.
        // It handles errors internally or via handler methods.
        if let Err(e) = event_loop.run_app(&mut app_handler) {
            log::error!("Event loop exited with error: {}", e);
        }

        // Code here runs *after* the event loop has fully exited
        log::info!("Event loop has finished.");
        app_handler.engine.shutdown();
    }

    /// Cleans up resources and subsystems before exiting the engine.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn shutdown(&mut self) {
        // Securely shutdown the engine and its subsystems
        if self.is_running {
            log::warn!("Shutdown called while engine was still marked as running.");
            self.is_running = false;
        }

        log::info!("Shutting down engine...");

        self.window = None;
        log::info!("Window wrapper dropped.");

        // TODO: Explicitly shutdown other subsystems

        log::info!("Engine shutdown complete. Final memory usage: {} KiB", get_currently_allocated_bytes() / 1024);
    }

    /// Returns the event bus sender for publishing events.
    /// ## Arguments
    /// * `&self` - A reference to the Engine instance.
    pub fn event_sender(&self) -> flume::Sender<EngineEvent> {
        self.event_bus.sender()
    }
}


//// Represents the application handler for the engine, implementing the ApplicationHandler trait.
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

        // Create the window if it doesn't exist
        // This is called when the application is resumed from a suspended state.
        // If the window is already created, we can skip this step.
        // This is useful for handling cases where the application was suspended and resumed,
        // but the window was not destroyed (e.g., on mobile platforms).
        if self.engine.window.is_none() {
            log::info!("Application resumed: Creating window...");
            match KhoraWindow::new(event_loop) {
                Ok(win_wrapper) => {
                    self.engine.window = Some(win_wrapper);
                }
                Err(e) => {
                    log::error!("FATAL: Failed to create window: {}", e);
                    event_loop.exit();
                }
            }
        } else {
            log::info!("Application resumed.");
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
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let engine = &mut self.engine;

        // Process event only if window exists and ID matches
        if let Some(khora_window) = &engine.window {
            if khora_window.id() == window_id {

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
                        log::info!("Window Resized event: {}x{}", physical_size.width, physical_size.height);
                        engine.event_bus.publish(EngineEvent::WindowResized {
                            width: physical_size.width,
                            height: physical_size.height,
                        });

                        // TODO: Potentially tell renderer directly to resize swapchain etc.
                    }

                    // Case where the window is redrawn (e.g., after resizing or when requested)
                    WindowEvent::RedrawRequested => {
                        log::trace!("Window Redraw Requested for id: {:?}", window_id);

                        // Increment frame counters BEFORE rendering
                        engine.frame_count += 1;
                        engine.frames_since_last_log += 1;

                        // --- Render Phase ---
                        engine.render();

                        // --- Stats Logging ---
                        let time_since_last_log = engine.last_stats_time.elapsed_secs_f64().unwrap_or(0.0);

                        if time_since_last_log >= engine.log_interval_secs {
                            let elapsed_secs = time_since_last_log;

                            // Calculate FPS based on frames since last log and elapsed time
                            let fps = if elapsed_secs > 0.0 { (engine.frames_since_last_log as f64 / elapsed_secs) as u32 } else { 0 };
                            
                            // Calculate memory usage in bytes
                            let memory_usage_bytes = get_currently_allocated_bytes();
                            let render_us: u64 = 0;
                            
                            log::info!(
                                "Stats | Frame: {}, FPS: {}, Mem: {} KiB | Render: {} us",
                                engine.frame_count, fps, memory_usage_bytes / 1024, render_us
                            );
                            engine.last_stats_time = Stopwatch::new();
                            engine.frames_since_last_log = 0;
                        }
                    }

                    // Case where the window is moved by the user
                    WindowEvent::Focused(focused) => {
                        log::debug!("Window Focus Changed: {}", focused);
                        // TODO: Publish focus change event if needed
                    }
                    _ => {}
                }
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

        // --- Process Internal Events ---
        let mut events_to_process = Vec::new();
        let receiver = engine.event_bus.receiver();

        // Collect events from the receiver into a vector to avoid mutable borrow issues
        while let Ok(event) = receiver.try_recv() {
            events_to_process.push(event);
        }

        log::trace!("Collected {} internal events to process.", events_to_process.len()); // Ne s'affiche pas

        // Now process collected events with mutable access
        for event in events_to_process {
            log::trace!("Processing internal event: {:?}", event); // Ne s'affiche pas
            engine.handle_internal_event(event);
        }

        // Check if shutdown was requested internally
        if !engine.is_running {
            log::info!("Shutdown requested via internal event, exiting loop.");
            event_loop.exit();
            return;
        }

        // --- Update Phase ---
        engine.update();

        // Request a redraw for the window to trigger the redraw_requested method
        if let Some(khora_window) = &engine.window {
            khora_window.request_redraw();
        }
    }

    /// Called just before the application exits
    /// This is where we can perform any final cleanup or resource deallocation.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the EngineAppHandler instance.
    /// * `event_loop` - A reference to the ActiveEventLoop instance.
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        log::info!("Application exiting event received.");
    }
}


impl Engine {
    /// Processes internal EngineEvents received from the EventBus
    /// This function collects events from the event bus and processes them in a batch.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    fn process_internal_events(&mut self) {
        
        let mut events_to_process = Vec::new();
        let receiver = self.event_bus.receiver();
         
        while let Ok(event) = receiver.try_recv() {
            events_to_process.push(event);
        }

        // Process collected events
        for event in events_to_process {
            log::trace!("Processing internal event: {:?}", event);
            self.handle_internal_event(event);
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
                log::info!("Internal Handling: Window resized: {}x{}", width, height);
                // TODO: Notify Renderer, update Camera aspect ratio, etc.
            }
            EngineEvent::Input(input_event) => {
                log::trace!("Internal Handling: Input event: {:?}", input_event);
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

    /// Engine's rendering logic
    /// This function is called every frame after the update logic.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    fn render(&mut self) {
        if let Some(khora_window) = &self.window {
            log::trace!("Render tick for window: {:?}", khora_window.id());
            // TODO: Implement rendering using khora_window
        } else {
            log::warn!("Render called but window is not initialized!");
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::input::{InputEvent, MouseButton};

    /// Test the Engine's setup method
    #[test]
    fn handle_internal_event_shutdown_sets_is_running_false() {
        let mut engine = Engine::new();
        engine.is_running = true;

        let event = EngineEvent::ShutdownRequested;
        engine.handle_internal_event(event);

        assert!(!engine.is_running, "Engine should not be running after ShutdownRequested");
    }

    /// Test the Engine's resizing method
    #[test]
    fn handle_internal_event_resize_does_not_panic() {
        let mut engine = Engine::new();
        engine.is_running = true;
        let event = EngineEvent::WindowResized { width: 800, height: 600 };

        engine.handle_internal_event(event);
    }

    /// Test the Engine's input handling method
    #[test]
    fn handle_internal_event_input_does_not_panic() {
        let mut engine = Engine::new();
        engine.is_running = true;
        let event = EngineEvent::Input(InputEvent::KeyPressed{ key_code: "KeyA".to_string() });

        engine.handle_internal_event(event);
    }
}