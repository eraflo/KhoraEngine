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

use log;

/// Manages a generic, multi-producer, single-consumer (MPSC), thread-safe event channel.
///
/// This `EventBus` is generic over the event type `T` it transports. It serves as a
/// foundational communication primitive within Khora, allowing different parts of the
/// engine to communicate in a decoupled manner.
///
/// The design is intentional: there are many senders but only one receiver, ensuring
/// that a single, authoritative system is responsible for processing all events of a
/// given type. Senders can be cloned freely and passed to different threads.
///
/// # Examples
///
/// ```
/// # use khora_core::event::EventBus;
/// #[derive(Clone, Debug, PartialEq)]
/// enum GameEvent {
///     PlayerJumped,
///     ScoreChanged(u32),
/// }
///
/// // Create a new bus for our specific event type.
/// let event_bus = EventBus::<GameEvent>::new();
///
/// // Clone the sender to give to a game system.
/// let sender = event_bus.sender();
///
/// // A system publishes an event.
/// sender.send(GameEvent::PlayerJumped);
///
/// // The main event loop (the owner of the bus) processes the event.
/// if let Ok(event) = event_bus.receiver().try_recv() {
///     assert_eq!(event, GameEvent::PlayerJumped);
/// }
/// ```
#[derive(Debug)]
pub struct EventBus<T: Clone + Send + Sync + 'static> {
    sender: flume::Sender<T>,
    receiver: flume::Receiver<T>,
}

impl<T: Clone + Send + Sync + 'static> EventBus<T> {
    /// Creates a new `EventBus` with an unbounded channel.
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        log::info!(
            "Generic EventBus initialized for type {}.",
            std::any::type_name::<T>()
        );
        Self { sender, receiver }
    }

    /// Publishes an event to all receivers.
    ///
    /// This method is a convenience wrapper around the channel's `send` operation.
    /// It logs an error if the send fails, which typically means the receiver
    /// (and thus the `EventBus` instance) has been dropped.
    ///
    /// # Arguments
    ///
    /// * `event`: The event of type `T` to be sent over the channel.
    pub fn publish(&self, event: T) {
        log::trace!(
            "Publishing an event of type {}.",
            std::any::type_name::<T>()
        );

        if let Err(e) = self.sender.send(event) {
            log::error!("Failed to send event: {e}. Receiver likely disconnected.");
        }
    }

    /// Returns a clone of the sender part of the channel.
    ///
    /// This is the primary way to allow other parts of the system to send events
    /// without giving them ownership of the entire bus. Senders can be cloned
    /// multiple times and sent across threads.
    pub fn sender(&self) -> flume::Sender<T> {
        self.sender.clone()
    }

    /// Returns a reference to the receiver part of the channel.
    ///
    /// This is intended for the owner of the bus (e.g., the main event loop) to
    /// process incoming events. It returns a reference to prevent the receiver
    /// from being moved out of the `EventBus`.
    pub fn receiver(&self) -> &flume::Receiver<T> {
        &self.receiver
    }
}

impl<T: Clone + Send + Sync + 'static> Default for EventBus<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flume::{SendError, TryRecvError};
    use std::{thread, time::Duration};

    /// A local, self-contained event enum for testing purposes.
    /// This mimics the old `EngineEvent` without creating external dependencies.
    #[derive(Debug, Clone, PartialEq)]
    enum TestEvent {
        WindowResized { width: u32, height: u32 },
        KeyPressed { key_code: String },
        ShutdownRequested,
    }

    fn dummy_key_event() -> TestEvent {
        TestEvent::KeyPressed {
            key_code: "Test".to_string(),
        }
    }

    #[test]
    fn event_bus_creation() {
        let bus = EventBus::<TestEvent>::new();
        let _sender = bus.sender();
        // The receiver is private, which is good.
        assert!(bus.receiver().is_empty());
    }

    #[test]
    fn send_receive_single_event() {
        let bus = EventBus::<TestEvent>::new();
        let sender = bus.sender();
        let receiver = bus.receiver();
        let event_to_send = dummy_key_event();

        sender
            .send(event_to_send.clone())
            .expect("Send should succeed");

        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(received_event) => assert_eq!(received_event, event_to_send),
            Err(e) => panic!("Failed to receive event: {e:?}"),
        }
    }

    #[test]
    fn try_receive_empty() {
        let bus = EventBus::<TestEvent>::new();
        let receiver = bus.receiver();

        match receiver.try_recv() {
            Err(TryRecvError::Empty) => { /* This is the expected outcome */ }
            Ok(event) => panic!("Received unexpected event: {event:?}"),
            Err(e) => panic!("Received unexpected error: {e:?}"),
        }
    }

    #[test]
    fn send_receive_multiple_events() {
        let bus = EventBus::<TestEvent>::new();
        let sender = bus.sender();
        let receiver = bus.receiver();

        let event1 = TestEvent::WindowResized {
            width: 1,
            height: 1,
        };
        let event2 = dummy_key_event();
        let event3 = TestEvent::ShutdownRequested;

        sender.send(event1.clone()).expect("Send 1 should succeed");
        sender.send(event2.clone()).expect("Send 2 should succeed");
        sender.send(event3.clone()).expect("Send 3 should succeed");

        let mut received_events = Vec::new();
        for _ in 0..3 {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => received_events.push(event),
                Err(e) => panic!("Failed to receive event within timeout: {e:?}"),
            }
        }

        assert_eq!(received_events.len(), 3);
        assert_eq!(received_events[0], event1);
        assert_eq!(received_events[1], event2);
        assert_eq!(received_events[2], event3);

        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn multiple_senders() {
        let bus = EventBus::<TestEvent>::new();
        let sender1 = bus.sender();
        let sender2 = bus.sender();
        let receiver = bus.receiver();

        let event1 = TestEvent::WindowResized {
            width: 1,
            height: 1,
        };
        let event2 = dummy_key_event();

        sender1.send(event1.clone()).expect("Send 1 should succeed");
        sender2.send(event2.clone()).expect("Send 2 should succeed");

        let rec1 = receiver
            .recv_timeout(Duration::from_millis(50))
            .expect("Receive 1 failed");
        let rec2 = receiver
            .recv_timeout(Duration::from_millis(50))
            .expect("Receive 2 failed");

        assert!((rec1 == event1 && rec2 == event2) || (rec1 == event2 && rec2 == event1));
    }

    #[test]
    fn send_from_thread() {
        let bus = EventBus::<TestEvent>::new();
        let sender_clone = bus.sender();
        let receiver = bus.receiver();
        let event_to_send = dummy_key_event();
        let event_clone = event_to_send.clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            sender_clone
                .send(event_clone)
                .expect("Send from thread failed");
            log::trace!("Event sent from spawned thread.");
        });

        log::trace!("Main thread waiting for event...");
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(received_event) => {
                log::trace!("Main thread received event.");
                assert_eq!(received_event, event_to_send);
            }
            Err(e) => panic!("Failed to receive event from thread: {e:?}"),
        }

        handle.join().expect("Thread join failed");
    }

    #[test]
    fn send_error_on_receiver_drop() {
        let bus = EventBus::<TestEvent>::new();
        let sender = bus.sender();
        let event_to_send = dummy_key_event();

        drop(bus);
        log::trace!("EventBus (and receiver) dropped.");

        match sender.send(event_to_send) {
            Err(SendError(_)) => { /* This is the expected outcome */ }
            Ok(()) => panic!("Send unexpectedly succeeded after receiver drop"),
        }
        log::trace!("Send correctly failed after receiver drop.");
    }
}
