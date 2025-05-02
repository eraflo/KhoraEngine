
use crate::memory::get_currently_allocated_bytes;
use crate::{core::timer::Stopwatch, event::{EngineEvent, EventBus}};


/// Represents the main engine structure, responsible for orchestrating subsystems.
#[derive(Debug, Default)]
pub struct Engine {
    is_running: bool,
    event_bus: EventBus
}



impl Engine {
    /// Creates a new, uninitialized Engine instance.
    /// ## Returns
    /// A new instance of the Engine struct.
    pub fn new() -> Self {
        Self {
            is_running: false,
            event_bus: EventBus::default()
        }
    }

    /// Sets up the engine subsystems before running the main loop.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn setup(&mut self) {
        env_logger::init();
        log::info!("Setting up engine systems...");

        // TODO: Initialize subsystems (ECS, graphics, audio, etc.)

    }

    /// Starts and executes the main engine loop.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn run(&mut self) {

        // Check if the engine is already running
        if self.is_running {
            log::warn!("Engine::run called while already running.");
            return;
        }
        self.is_running = true;

        log::info!("Starting engine loop...");
        
        let mut frame_count: u64 = 0;

        // Timer for total duration
        let total_timer = Stopwatch::new();
        

        // Timer for FPS calculation (measure time between frames or over N frames)
        let mut last_fps_time = Stopwatch::new();
        let mut frames_since_last_log: u32 = 0;
        let log_interval_secs = 1.0; // Log stats every second

        while self.is_running {
            frame_count += 1;
            frames_since_last_log += 1;

            // Timer for the current frame duration
            let frame_timer = Stopwatch::new();

            // --- Event processing ---
            let event_processing_timer = Stopwatch::new();
            let receiver = self.event_bus.receiver();

            let mut received_count = 0;

            // Collect all events first to avoid borrow conflicts
            let mut events = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                received_count += 1;
                log::trace!("Event received: {:?}", event);
                events.push(event);
            }

            // Process all events
            for event in events {
                self.handle_event(event);

                if !self.is_running 
                { 
                    break; 
                }
            }


            // Log the time taken for event processing
            let event_processing_us = event_processing_timer.elapsed_us().unwrap_or(0);

            if received_count > 0 {
                log::trace!("Event processing took: {} us ({} events)", event_processing_us, received_count);
            }

            // Exit loop if shutdown was requested
            if !self.is_running { break; }


            // TODO: Update Phase


            // TODO: Render Phase


            // --- Frame End & Stats ---
            let frame_time_us = frame_timer.elapsed_us().unwrap_or(0);

            // Calculate and log stats periodically
            let time_since_last_log = last_fps_time.elapsed().map_or(0.0, |d| d.as_secs_f64());

            if time_since_last_log >= log_interval_secs {

                let elapsed_secs = time_since_last_log;

                let fps = if elapsed_secs > 0.0 { // Avoid division by zero
                    (frames_since_last_log as f64 / elapsed_secs) as u32
                } else {
                    0 
                };


                // --- Get current memory usage ---
                let memory_usage_bytes = get_currently_allocated_bytes();
                // --------------------------------

                log::info!(
                    "Stats | Frame: {}, FPS: {}, FrameTime: {:.2} ms, Mem: {} KiB | Events: {} us",
                    frame_count,
                    fps,
                    frame_time_us as f64 / 1000.0, // Frame time in ms
                    memory_usage_bytes / 1024, // Memory in KiB
                    event_processing_us
                );

                // Reset counters for next interval
                last_fps_time = Stopwatch::new(); // Restart timer
                frames_since_last_log = 0;
            }
        }

        // --- Loop exit ---
        let total_duration_secs = total_timer.elapsed().unwrap_or(std::time::Duration::ZERO).as_secs_f64();
        log::info!(
            "Engine loop finished after {:.2} seconds ({} frames). Final memory usage: {} KiB",
            total_duration_secs,
            frame_count,
            get_currently_allocated_bytes() / 1024
        );
        self.is_running = false;
    }

    
    /// Handles a single EngineEvent.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    /// * `event` - The EngineEvent to handle.
    fn handle_event(&mut self, event: EngineEvent) {
         match event {
            EngineEvent::WindowResized { width, height } => {
                log::info!("Window resized event: {}x{}", width, height);
                // TODO: Notify relevant subsystems (e.g., renderer)
            }
            EngineEvent::Input(input_event) => {
                log::trace!("Input event received: {:?}", input_event);
                // TODO: Pass input to the input subsystem or relevant game logic
            }
            EngineEvent::ShutdownRequested => {
                log::info!("Shutdown requested event received.");
                self.is_running = false;
            }
            _ => {
                log::warn!("Unhandled engine event type: {:?}", event);
            }
        }
    }


    /// Cleans up resources and shuts down subsystems.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn shutdown(&mut self) {

        if self.is_running {
            log::warn!("Shutdown called while engine loop was still marked as running.");
        }

        log::info!("Shutting down engine...");

        // TODO: Explicitly clean up subsystems (ECS, graphics, audio, physics, networking etc.)

        log::info!("Engine shutdown complete.");
    }

    /// Returns a clone of the event sender channel via the EventBus.
    /// ## Returns
    /// A clone of the event sender channel.
    pub fn event_sender(&self) -> flume::Sender<EngineEvent> {
        self.event_bus.sender()
    }
}