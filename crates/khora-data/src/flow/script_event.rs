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

//! The door the engine raises script events through.
//!
//! A script raising an event for another script never comes this way — those
//! stay inside the scripting agent, which is what keeps its isolation claim
//! true. This is for the *other* producers: a collision, an input binding, a
//! trigger volume. Whatever wants to tell a behavior something happened puts a
//! [`ScriptEvent`] here, and the next frame delivers it.
//!
//! The same shape as [`script_reload`](super::script_reload), and for the same
//! reason: a `DataSystem` or a service writes, a flow drains into the bus, and
//! the agent reads only the view. Nothing shared crosses into the lane.

use std::sync::{Arc, Mutex};

use khora_core::script::ScriptEvent;
use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;

/// Where an engine-raised event waits for the next frame.
#[derive(Debug, Clone, Default)]
pub struct PendingScriptEvents {
    queue: Arc<Mutex<Vec<ScriptEvent>>>,
}

impl PendingScriptEvents {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an event for the next frame.
    ///
    /// Appended, never merged: two collisions in one frame are two events, and
    /// a producer that meant to send one should send one.
    pub fn push(&self, event: ScriptEvent) {
        match self.queue.lock() {
            Ok(mut queue) => queue.push(event),
            Err(_) => log::error!("script event queue is poisoned; dropping an event"),
        }
    }

    /// Takes everything queued.
    pub fn drain(&self) -> Vec<ScriptEvent> {
        match self.queue.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => Vec::new(),
        }
    }

    /// How many are waiting.
    pub fn len(&self) -> usize {
        self.queue.lock().map(|queue| queue.len()).unwrap_or(0)
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What the engine raised for scripts this frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScriptEventView {
    /// The events, in the order they were raised.
    pub events: Vec<ScriptEvent>,
}

impl ScriptEventView {
    /// Whether nothing was raised.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Drains the pending engine events into the bus.
#[derive(Default)]
pub struct ScriptEventFlow;

impl Flow for ScriptEventFlow {
    type View = ScriptEventView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "script_event";

    fn project(&self, _world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let Some(pending) = runtime.resources.get::<PendingScriptEvents>() else {
            // Nothing has ever raised one, so the resource was never inserted.
            // Ordinary: an engine with no producer wired yet still runs scripts
            // that talk to each other.
            return ScriptEventView::default();
        };

        // Drained rather than read: an event is delivered once. Leaving it
        // queued would deliver it again every frame, which for `Damaged` means
        // an entity that never stops taking the same hit.
        ScriptEventView {
            events: pending.drain(),
        }
    }
}

register_flow!(ScriptEventFlow);

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;
    use khora_core::script::ScriptValue;

    fn entity(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 1,
        }
    }

    fn hit() -> ScriptEvent {
        ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(10))
    }

    #[test]
    fn a_queued_event_comes_back_once() {
        let pending = PendingScriptEvents::new();
        pending.push(hit());

        assert_eq!(pending.drain(), vec![hit()]);
        assert!(pending.drain().is_empty(), "draining takes it away");
    }

    /// **Not deduplicated.** Two hits in one frame are two hits — merging them
    /// would silently halve the damage.
    #[test]
    fn two_of_the_same_event_are_two_events() {
        let pending = PendingScriptEvents::new();
        pending.push(hit());
        pending.push(hit());

        assert_eq!(pending.len(), 2);
    }

    /// A producer that shares the handle reaches the same queue, which is the
    /// point of it being shared at all.
    #[test]
    fn a_clone_writes_to_the_same_queue() {
        let pending = PendingScriptEvents::new();
        let elsewhere = pending.clone();
        elsewhere.push(hit());

        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn a_view_with_no_resource_is_empty_rather_than_absent() {
        let world = World::new();
        let runtime = Runtime::default();
        let view = ScriptEventFlow.project(&world, &Selection::default(), &runtime);

        assert!(view.is_empty());
    }
}
