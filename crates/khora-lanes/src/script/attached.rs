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

//! What the lane keeps across frames beside its instances: what it last saw
//! of each component-attached instance's row, the events an instance of an
//! entity running several behaviors has yet to hear.
//!
//! A behavior attached by its component is stored in the `World`, and the
//! lane is handed its row every frame. Comparing that row with what the lane
//! last saw or wrote is how it tells a field written outside it — an edit
//! during play, another script's write — from its own, and takes the first
//! without discarding the rest of what the instance holds.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue};

/// An instance, by its entity and its behavior.
type Key = (EntityId, String);

/// The lane's memory of the instances a component attaches.
#[derive(Debug, Default)]
pub struct Attached {
    /// Each instance's row as the lane last saw or wrote it.
    seen: HashMap<Key, Vec<(String, ScriptValue)>>,
    /// Events an instance has not heard yet, held for its next turn.
    held: HashMap<Key, Vec<ScriptEvent>>,
}

impl Attached {
    /// Forgets every instance whose entity is gone.
    pub(super) fn retain(&mut self, alive: impl Fn(EntityId, &str) -> bool) {
        self.seen
            .retain(|(entity, behavior), _| alive(*entity, behavior));
        self.held
            .retain(|(entity, behavior), _| alive(*entity, behavior));
    }

    /// The fields of `row` that differ from what the lane last saw of the
    /// instance — nothing for one it has not seen.
    pub(super) fn changed_outside(
        &self,
        entity: EntityId,
        behavior: &str,
        row: &[(String, ScriptValue)],
    ) -> Vec<(String, ScriptValue)> {
        let Some(seen) = self.seen.get(&(entity, behavior.to_owned())) else {
            return Vec::new();
        };
        row.iter()
            .filter(|(name, value)| {
                !seen
                    .iter()
                    .any(|(known, was)| known == name && was == value)
            })
            .cloned()
            .collect()
    }

    /// Records the instance's row as the lane has it now.
    pub(super) fn saw(
        &mut self,
        entity: EntityId,
        behavior: &str,
        row: Vec<(String, ScriptValue)>,
    ) {
        self.seen.insert((entity, behavior.to_owned()), row);
    }

    /// The events the instance has yet to hear, taken.
    pub(super) fn take_held(&mut self, entity: EntityId, behavior: &str) -> Vec<ScriptEvent> {
        self.held
            .remove(&(entity, behavior.to_owned()))
            .unwrap_or_default()
    }

    /// Holds `events` for the instance's next turn.
    pub(super) fn hold(&mut self, entity: EntityId, behavior: &str, events: Vec<ScriptEvent>) {
        if !events.is_empty() {
            self.held.insert((entity, behavior.to_owned()), events);
        }
    }
}
