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

//! Live behavior state on its way back into the behavior's component and its
//! state companion (`Guard`, `Guard#state`), kept current for a save.
//!
//! The lane writes back only the instances that did work this frame: one that
//! spent no fuel changed nothing. This is apart from [`CommandBuffer`], so
//! engine bookkeeping never enters the conflict report of the author's writes.
//!
//! [`CommandBuffer`]: super::CommandBuffer

use crate::ecs::entity::EntityId;

use super::ScriptSnapshot;

/// One instance's state, as the scene should record it.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorUpdate {
    /// Which entity.
    pub entity: EntityId,
    /// Which behavior on it — an entity may carry several.
    pub behavior: String,
    /// Everything it is: fields, state, countdowns, a suspended sequence.
    pub snapshot: ScriptSnapshot,
    /// The fields this turn changed: the only ones written back, so a field
    /// another script wrote the same frame survives.
    pub changed: Vec<String>,
    /// How many of the frame's [`CommandBuffer`] commands were queued when
    /// the turn began.
    ///
    /// A `Set` of its fields at a lower position was read by it, and is not a
    /// write it lost to.
    ///
    /// [`CommandBuffer`]: super::CommandBuffer
    pub read_until: usize,
    /// How many of the frame's commands were queued when the turn ended.
    ///
    /// A `Set` of a changed field at this position or later wins over the
    /// turn's write; one the turn queued itself, before, loses to it.
    pub emitted_after: usize,
}

/// Instance state waiting to be recorded in the scene.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BehaviorWriteback {
    updates: Vec<BehaviorUpdate>,
}

impl BehaviorWriteback {
    /// Queues one instance's state.
    pub fn push(&mut self, update: BehaviorUpdate) {
        self.updates.push(update);
    }

    /// Takes every update, leaving the allocation intact for the next frame.
    pub fn drain(&mut self) -> std::vec::Drain<'_, BehaviorUpdate> {
        self.updates.drain(..)
    }
}

impl Extend<BehaviorUpdate> for BehaviorWriteback {
    fn extend<T: IntoIterator<Item = BehaviorUpdate>>(&mut self, iter: T) {
        self.updates.extend(iter);
    }
}
