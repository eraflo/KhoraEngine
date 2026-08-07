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

//! Live behavior state on its way back into the scene.
//!
//! A behavior's fields live in the script lane while the game runs, and in the
//! `Script` component when the scene is saved. Something has to carry them from
//! the first to the second, and a save can happen at any moment — so the
//! component has to be current at every moment, not only when someone asks.
//!
//! # Only what did something
//!
//! Writing every instance every frame would clone a name and a value per field
//! per entity in a game that may never save. Writing none until a save is
//! requested needs a signal that crosses the same boundary the lane exists to
//! keep, and lands a frame late.
//!
//! Neither is necessary, because a behavior that spent no fuel this frame
//! changed nothing: it handled no event and ran no code. So the lane writes back
//! exactly the instances that **did work**, which is free in a quiet frame and
//! costs only where the state genuinely moved.
//!
//! It is a separate slot from [`CommandBuffer`] rather than a `SetComponent`
//! command because the two are different kinds of thing: a command is what a
//! *script* asked for, and this is the engine reconciling its own bookkeeping.
//! Folding them together would put engine writes into the conflict report meant
//! for the author's.
//!
//! [`CommandBuffer`]: super::CommandBuffer

use crate::ecs::entity::EntityId;

use super::ScriptSnapshot;

/// One instance's state, as the scene should record it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptStateUpdate {
    /// Which entity.
    pub entity: EntityId,
    /// Which behavior on it — an entity may carry several.
    pub behavior: String,
    /// Everything it is: fields, state, countdowns, a suspended sequence.
    pub snapshot: ScriptSnapshot,
}

/// Instance state waiting to be recorded in the scene.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptStateWriteback {
    updates: Vec<ScriptStateUpdate>,
}

impl ScriptStateWriteback {
    /// Queues one instance's state.
    pub fn push(&mut self, update: ScriptStateUpdate) {
        self.updates.push(update);
    }

    /// Takes every update, leaving the allocation intact for the next frame.
    pub fn drain(&mut self) -> std::vec::Drain<'_, ScriptStateUpdate> {
        self.updates.drain(..)
    }
}

impl Extend<ScriptStateUpdate> for ScriptStateWriteback {
    fn extend<T: IntoIterator<Item = ScriptStateUpdate>>(&mut self, iter: T) {
        self.updates.extend(iter);
    }
}
