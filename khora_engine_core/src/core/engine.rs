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
        self.is_running = true;

        log::info!("Starting engine loop...");

        // Main loop timer to measure the total time taken for the engine loop
        let total_timer = Stopwatch::new();

        
        // Frame counter to keep track of the number of frames processed
        let mut frame_count: u64 = 0;

        while self.is_running {
            frame_count += 1;
            
            // Watchdog for the entire engine loop
            // This timer is used to measure the time taken for each frame in the engine loop.
            let frame_timer  = Stopwatch::new();

            // --- Event processing ---
            let event_timer = Stopwatch::new();
            let receiver = self.event_bus.receiver();
            let mut received_count = 0;

            while let Ok(event) = receiver.try_recv() {
                received_count += 1;
                log::trace!("Event received: {:?}", event);

                match event {
                    EngineEvent::WindowResized { width, height } => {
                        log::info!("Window resized event: {}x{}", width, height);
                    }
                    EngineEvent::Input(input_event) => {
                        log::trace!("Input event received: {:?}", input_event);
                    }
                    EngineEvent::ShutdownRequested => {
                        log::info!("Shutdown requested event received.");
                        self.is_running = false;
                        break;
                    }
                    _ => {}
                }
            }
            
            // --- Log event processing time ---
            if let Some(duration_us) = event_timer.elapsed_us() {
                if received_count > 0 || duration_us > 10 {
                    log::trace!("Event processing took: {} us ({} events)", duration_us, received_count);
                }
            }

            // TODO: Implement Update and Render phases

            // --- Log frame processing time ---
            if let Some(duration) = frame_timer.elapsed() {
                let duration_ms = duration.as_millis() as u64;
                if frame_count % 10 == 0 {
                    log::trace!("Frame processing took: {} ms ({} frames)", duration_ms, frame_count);
                }
            }
        }

        // --- Log total engine loop time ---
        if let Some(duration) = total_timer.elapsed() {
            let duration_ms = duration.as_millis() as u64;
            log::info!("Engine loop completed in: {} ms ({} frames)", duration_ms, frame_count);
        }

        // --- Loop exit ---
        log::info!("Exiting engine loop...");
        self.is_running = false;
    }

    /// Cleans up resources and shuts down subsystems.
    /// ## Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn shutdown(&mut self) {
        log::info!("Shutting down engine...");
        // TODO: Clean up subsystems (ECS, graphics, audio, etc.)
    }

    /// Returns a clone of the event sender channel via the EventBus.
    /// ## Returns
    /// A clone of the event sender channel.
    pub fn event_sender(&self) -> flume::Sender<EngineEvent> {
        self.event_bus.sender()
    }
}