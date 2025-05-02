use crate::subsystems::input::InputEvent;



/// Represents engine-wide events that can be sent over the message bus.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The application window was resized.
    WindowResized { width: u32, height: u32 },

    /// An input event occurred (wraps the specific InputEvent).
    Input(InputEvent),

    /// A signal to initiate engine shutdown.
    ShutdownRequested,
}