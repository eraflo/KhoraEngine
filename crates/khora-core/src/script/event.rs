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

//! What a script is told about.
//!
//! The mirror of [`WorldCommand`]: a command is what a script asks the engine
//! for, an event is what the engine — or another script — tells it. Both are
//! queued and drained at the frame boundary, for the same reason.
//!
//! # Nothing subscribes
//!
//! `on Damaged(int amount) { … }` is the whole subscription. There is no
//! register call, so there is none to forget, and no unregister call, so there
//! is none to forget either. An event is delivered by asking whether the target
//! entity's behavior declares a handler for it — which means a despawned entity
//! stops receiving events because it is no longer there to be asked, not
//! because something remembered to detach it.
//!
//! That is the difference from a subscription list, and it is not a
//! simplification: a list holding a dead entity is a leak *and* a dangling
//! call, and every engine that keeps one grows a lifetime discipline to manage
//! it. Deriving the answer instead has no state to go stale.
//!
//! [`WorldCommand`]: super::WorldCommand

use crate::ecs::entity::EntityId;

use super::ScriptValue;

/// Something that happened, addressed to one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptEvent {
    /// The entity whose behavior should hear about it.
    ///
    /// Always named: a broadcast would have to be delivered by scanning every
    /// behavior, and an event with no subject is a game-wide fact, which is
    /// what a global behavior's own state is for.
    pub target: EntityId,
    /// The event's name, as a handler spells it — `Damaged`.
    pub name: String,
    /// What it carries, in declaration order.
    pub args: Vec<ScriptValue>,
}

impl ScriptEvent {
    /// An event carrying nothing.
    pub fn new(target: EntityId, name: impl Into<String>) -> Self {
        Self {
            target,
            name: name.into(),
            args: Vec::new(),
        }
    }

    /// Adds an argument.
    pub fn with(mut self, value: ScriptValue) -> Self {
        self.args.push(value);
        self
    }
}

/// Events waiting to be delivered.
///
/// Queued rather than delivered where they arise, for the same reason commands
/// are: a collision is detected inside a lane, and running a script there would
/// put gameplay in the middle of the physics step — where the `World` is not
/// writable and the frame's ordering is not the scene's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventQueue {
    events: Vec<ScriptEvent>,
}

impl EventQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an event.
    pub fn push(&mut self, event: ScriptEvent) {
        self.events.push(event);
    }

    /// How many are waiting.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The queued events, in the order they were raised.
    pub fn as_slice(&self) -> &[ScriptEvent] {
        &self.events
    }

    /// Takes every event, leaving the queue empty and its allocation intact.
    pub fn drain(&mut self) -> std::vec::Drain<'_, ScriptEvent> {
        self.events.drain(..)
    }

    /// Discards everything queued.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Everything addressed to one entity, in order.
    pub fn for_entity(&self, entity: EntityId) -> impl Iterator<Item = &ScriptEvent> + '_ {
        self.events.iter().filter(move |e| e.target == entity)
    }
}

impl<'a> IntoIterator for &'a EventQueue {
    type Item = &'a ScriptEvent;
    type IntoIter = std::slice::Iter<'a, ScriptEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}
