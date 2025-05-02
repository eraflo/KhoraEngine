use crate::event::{EventBus, EngineEvent};


/// Represents the main engine structure, responsible for orchestrating subsystems.
#[derive(Debug, Default)]
pub struct Engine {
    is_running: bool,
    event_bus: EventBus
}



impl Engine {
    /// Creates a new, uninitialized Engine instance.
    /// # Returns
    /// A new instance of the Engine struct.
    pub fn new() -> Self {
        Self {
            is_running: false,
            event_bus: EventBus::default()
        }
    }

    /// Sets up the engine subsystems before running the main loop.
    /// # Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn setup(&mut self) {
        env_logger::init();
        log::info!("Setting up engine systems...");

        // TODO: Initialize subsystems (ECS, graphics, audio, etc.)

    }

    /// Starts and executes the main engine loop.
    /// # Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn run(&mut self) {
        self.is_running = true;
        log::info!("Starting engine loop...");

        while self.is_running {
            let receiver = self.event_bus.receiver();
            while let Ok(event) = receiver.try_recv() {
                log::debug!("Received event: {:?}", event);
                match event {
                    EngineEvent::WindowResized { width, height } => {
                        log::info!("Window resized event: {}x{}", width, height);
                    }
                    EngineEvent::Input(input_event) => {
                        log::debug!("Input event received: {:?}", input_event);
                    }
                    EngineEvent::ShutdownRequested => {
                        log::info!("Shutdown requested event received.");
                        self.is_running = false;
                        break;
                    }
                    _ => {}
                }
            }
        }

        log::info!("Engine loop finished.");
        self.is_running = false;
    }

    /// Cleans up resources and shuts down subsystems.
    /// # Arguments
    /// * `&mut self` - A mutable reference to the Engine instance.
    pub fn shutdown(&mut self) {
        log::info!("Shutting down engine...");
        // TODO: Clean up subsystems (ECS, graphics, audio, etc.)
    }

    /// Returns a clone of the event sender channel via the EventBus.
    /// # Returns
    /// A clone of the event sender channel.
    pub fn event_sender(&self) -> flume::Sender<EngineEvent> {
        self.event_bus.sender()
    }
}