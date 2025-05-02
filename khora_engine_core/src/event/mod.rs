use flume::{Receiver, SendError, Sender, TryRecvError};
use log;
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

/// Manages the underlying event channel (sender and receiver).
#[derive(Debug)]
pub struct EventBus {
    sender: flume::Sender<EngineEvent>,
    receiver: flume::Receiver<EngineEvent>,
}

impl EventBus {
    /// Creates a new EventBus with an unbounded channel.
    /// # Returns
    /// A new instance of the EventBus struct.
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        log::info!("EventBus initialized.");
        Self { sender, receiver }
    }

    /// Attempts to send an event, logging an error if the receiver is disconnected.
    /// # Arguments
    /// * `event` - The event to be sent over the channel.
    /// # Returns
    /// None if the event was sent successfully, or an error if the receiver is disconnected.
    pub fn publish(&self, event: EngineEvent) {
        
        if let Err(e) = self.sender.send(event) {
            log::error!("Failed to send event: {}. Receiver likely disconnected.", e);
        }
    }

    /// Returns a clone of the sender end of the channel.
    /// Use this to allow other parts of the system to send events.
    /// # Returns
    /// A clone of the sender end of the channel.
    pub fn sender(&self) -> flume::Sender<EngineEvent> {
        self.sender.clone()
    }

    /// Returns a reference to the receiver end of the channel.
    /// Use this to allow other parts of the system to receive events.
    /// # Returns
    /// A reference to the receiver end of the channel.
    pub(crate) fn receiver(&self) -> &flume::Receiver<EngineEvent> {
        &self.receiver
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::input::InputEvent;
    use std::{thread, time::Duration};

    fn dummy_input_event() -> EngineEvent {
        EngineEvent::Input(InputEvent::KeyPressed { key_code: "Test".to_string() })
    }

    #[test]
    fn event_bus_creation() {
        let bus = EventBus::new();
        let _sender = bus.sender();
    }

    /// Test to ensure that the sender and receiver can be created and are not null
    #[test]
    fn send_receive_single_event() {
        let bus = EventBus::new();
        let sender = bus.sender();
        let receiver = bus.receiver();
        let event_to_send = dummy_input_event();

        sender.send(event_to_send.clone()).expect("Send should succeed");

        // Use recv_timeout to wait a short duration, preventing infinite hang if test fails
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(received_event) => assert_eq!(received_event, event_to_send),
            Err(e) => panic!("Failed to receive event: {:?}", e),
        }
    }

    /// Test to ensure that the receiver can handle empty state correctly
    /// and that it doesn't block indefinitely when no events are sent.
    #[test]
    fn try_receive_empty() {
        let bus = EventBus::new();
        let receiver = bus.receiver();

        match receiver.try_recv() {
            Err(TryRecvError::Empty) => { /* This is expected */ }
            Ok(event) => panic!("Received unexpected event: {:?}", event),
            Err(e) => panic!("Received unexpected error: {:?}", e),
        }
    }

    /// Test to ensure that multiple events can be sent and received correctly.
    /// This test sends three events and checks if they are received in the same order.
    #[test]
    fn send_receive_multiple_events() {
        let bus = EventBus::new();
        let sender = bus.sender();
        let receiver = bus.receiver();

        let event1 = EngineEvent::WindowResized { width: 1, height: 1 };
        let event2 = dummy_input_event();
        let event3 = EngineEvent::ShutdownRequested;

        sender.send(event1.clone()).expect("Send 1 should succeed");
        sender.send(event2.clone()).expect("Send 2 should succeed");
        sender.send(event3.clone()).expect("Send 3 should succeed");

        let mut received_events = Vec::new();
        // Drain the channel using try_recv in a loop (could also use recv_timeout 3 times)
        for _ in 0..3 {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => received_events.push(event),
                Err(e) => panic!("Failed to receive event within timeout: {:?}", e),
            }
        }


        assert_eq!(received_events.len(), 3);
        assert_eq!(received_events[0], event1);
        assert_eq!(received_events[1], event2);
        assert_eq!(received_events[2], event3);

        // Verify the channel is now empty
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }

    /// Test to ensure that multiple senders can send events to the same receiver.
    /// This test creates two senders and one receiver, sends events from both, and checks if they are received correctly.
    /// The order of events may not be guaranteed, so we check for both possibilities.
    #[test]
    fn multiple_senders() {
        let bus = EventBus::new();
        let sender1 = bus.sender();
        let sender2 = bus.sender();
        let receiver = bus.receiver();

        let event1 = EngineEvent::WindowResized { width: 1, height: 1 };
        let event2 = dummy_input_event();

        sender1.send(event1.clone()).expect("Send 1 should succeed");
        sender2.send(event2.clone()).expect("Send 2 should succeed");

        // Receive the two events (order might not be guaranteed, but likely FIFO here)
        let rec1 = receiver.recv_timeout(Duration::from_millis(50)).expect("Receive 1 failed");
        let rec2 = receiver.recv_timeout(Duration::from_millis(50)).expect("Receive 2 failed");

        // Check if both events were received, regardless of order
        assert!( (rec1 == event1 && rec2 == event2) || (rec1 == event2 && rec2 == event1) );
    }

    /// Test to ensure that sending from a separate thread works correctly.
    /// This test spawns a thread that sends an event after a short delay and checks if the main thread receives it correctly.
    /// The test uses a timeout to prevent hanging indefinitely if the event is not received.
    #[test]
    fn send_from_thread() {
        let bus = EventBus::new();
        let sender_clone = bus.sender(); // Clone sender for the thread
        let receiver = bus.receiver(); // Keep receiver in main thread
        let event_to_send = dummy_input_event();
        let event_clone = event_to_send.clone(); // Clone event data for the thread

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            sender_clone.send(event_clone).expect("Send from thread failed");
            log::trace!("Event sent from spawned thread.");
        });

        log::trace!("Main thread waiting for event...");
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(received_event) => {
                log::trace!("Main thread received event.");
                assert_eq!(received_event, event_to_send);
            }
            Err(e) => panic!("Failed to receive event from thread: {:?}", e),
        }

        handle.join().expect("Thread join failed");
    }

    /// Test to ensure that sending an event after the receiver has been dropped fails gracefully.
    /// This test drops the receiver and then tries to send an event, expecting a SendError.
    #[test]
    fn send_error_on_receiver_drop() {
        let bus = EventBus::new();
        let sender = bus.sender(); // Get sender BEFORE dropping the bus
        let event_to_send = dummy_input_event();

        // Drop the bus, which drops the receiver
        drop(bus);
        log::trace!("EventBus (and receiver) dropped.");

        // Now, try sending. This should fail.
        match sender.send(event_to_send) {
            Err(SendError(_)) => { /* This is the expected outcome */ }
            Ok(()) => panic!("Send unexpectedly succeeded after receiver drop"),
        }
        log::trace!("Send correctly failed after receiver drop.");
    }
}