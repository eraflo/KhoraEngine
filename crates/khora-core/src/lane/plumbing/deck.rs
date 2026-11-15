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

//! `OutputDeck` — typed mutable bus for lane outputs, scoped to one tick.
//!
//! Symmetric counterpart of [`LaneBus`](crate::lane::LaneBus): lanes write typed
//! outputs into the deck during the CLAD descent; the engine drains specific
//! types at the I/O boundary (e.g. recorded GPU command buffers, draw lists).
//!
//! # Visibility contract
//!
//! - [`OutputDeck::slot`] returns a `&mut T` for lanes to push into.
//! - [`OutputDeck::take`] is public so the engine's I/O boundary (the SDK's
//!   pass submission, the Maintenance DataSystems) can drain typed outputs;
//!   lanes only ever push.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Mutable typed deck collecting lane outputs for one tick.
///
/// Each typed slot is created on first access, holding `T::default()`. The
/// engine drains slots at the I/O boundary through [`OutputDeck::take`].
pub struct OutputDeck {
    slots: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// The slot types written through [`slot`](Self::slot) since the last
    /// [`drain_written`](Self::drain_written).
    ///
    /// The scheduler drops what a panicking agent wrote from it, and checks the
    /// writes against the agent's declaration in debug builds.
    written: Vec<TypeId>,
}

impl OutputDeck {
    /// Creates an empty deck.
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
            written: Vec::new(),
        }
    }

    /// Returns the slot for type `T`, created with `T::default()` on first access.
    ///
    /// Records `T` as written.
    pub fn slot<T: Default + Any + Send + Sync>(&mut self) -> &mut T {
        let type_id = TypeId::of::<T>();
        if !self.written.contains(&type_id) {
            self.written.push(type_id);
        }
        self.slots
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut::<T>()
            .expect("slot type mismatch — TypeId collision is impossible in safe Rust")
    }

    /// Removes and returns the slot for type `T`, so the next tick starts clean.
    ///
    /// Returns `T::default()` if no lane wrote into it this tick.
    pub fn take<T: Default + Any + Send + Sync>(&mut self) -> T {
        self.slots
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok().map(|b| *b))
            .unwrap_or_default()
    }

    /// Reads the slot for type `T`, if one was written this tick.
    ///
    /// Unlike [`slot`](Self::slot), it records no write, so a reader is not
    /// counted as a writer.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.slots
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }

    /// Returns, and forgets, the slot types written through
    /// [`slot`](Self::slot) since the previous call.
    pub fn drain_written(&mut self) -> Vec<TypeId> {
        std::mem::take(&mut self.written)
    }

    /// Removes the slots of the given types — what a panicking agent wrote,
    /// so a half-written value never reaches the I/O boundary.
    pub fn discard(&mut self, types: &[TypeId]) {
        for type_id in types {
            self.slots.remove(type_id);
        }
    }

    /// Reports whether a slot of the given type has been written this tick.
    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.slots.contains_key(&TypeId::of::<T>())
    }

    /// Number of slots currently holding data.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no slots have been written.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Clears all slots, for a deck reused between ticks.
    pub fn clear(&mut self) {
        self.slots.clear();
    }

    /// Moves the slots of `types` out, as they are, into a deck of their own.
    ///
    /// What a concurrent wave hands each member, so a slot grown sub-step
    /// after sub-step keeps growing. Nothing is recorded as written.
    pub fn take_slots(&mut self, types: &[TypeId]) -> OutputDeck {
        let mut taken = OutputDeck::new();
        for type_id in types {
            if let Some(value) = self.slots.remove(type_id) {
                taken.slots.insert(*type_id, value);
            }
        }
        taken
    }

    /// Moves every slot from `other` into `self`, replacing and recording each as written.
    ///
    /// How an agent commits what one run of its lane wrote into a deck of its own.
    pub fn commit_from(&mut self, other: OutputDeck) {
        for (type_id, value) in other.slots {
            if !self.written.contains(&type_id) {
                self.written.push(type_id);
            }
            self.slots.insert(type_id, value);
        }
    }

    /// Moves every slot from `other` into `self`.
    ///
    /// Folds a worker's shard back after a concurrent wave. Shards write
    /// disjoint slots (a wave's [`Contention`](crate::agent::Contention)s are
    /// disjoint), so a collision is a scheduling bug: the existing slot is
    /// kept and the collision logged.
    pub fn merge_from(&mut self, other: OutputDeck) {
        for (type_id, value) in other.slots {
            if self.slots.contains_key(&type_id) {
                log::error!(
                    "OutputDeck::merge_from: colliding slot {type_id:?} across parallel deck \
                     shards — keeping the existing entry (parallel-eligibility bug)"
                );
                continue;
            }
            self.slots.insert(type_id, value);
        }
    }
}

impl Default for OutputDeck {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OutputDeck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputDeck")
            .field("slots", &self.slots.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_lazily_inserts_default() {
        let mut deck = OutputDeck::new();
        assert!(!deck.contains::<Vec<u32>>());
        let v = deck.slot::<Vec<u32>>();
        v.push(1);
        v.push(2);
        assert_eq!(deck.slot::<Vec<u32>>(), &vec![1, 2]);
    }

    #[test]
    fn take_removes_and_returns_value() {
        let mut deck = OutputDeck::new();
        deck.slot::<Vec<u32>>().extend([1, 2, 3]);
        let taken: Vec<u32> = deck.take();
        assert_eq!(taken, vec![1, 2, 3]);
        assert!(!deck.contains::<Vec<u32>>());
    }

    #[test]
    fn take_missing_returns_default() {
        let mut deck = OutputDeck::new();
        let taken: Vec<u32> = deck.take();
        assert!(taken.is_empty());
    }

    #[test]
    fn distinct_types_dont_alias() {
        let mut deck = OutputDeck::new();
        deck.slot::<Vec<u32>>().push(1);
        deck.slot::<Vec<u8>>().push(2);
        assert_eq!(deck.take::<Vec<u32>>(), vec![1]);
        assert_eq!(deck.take::<Vec<u8>>(), vec![2]);
    }

    #[test]
    fn merge_from_folds_disjoint_shards() {
        let mut main = OutputDeck::new();
        main.slot::<Vec<u32>>().push(1);

        let mut shard = OutputDeck::new();
        shard.slot::<Vec<u8>>().extend([7, 8]);

        main.merge_from(shard);
        assert_eq!(main.take::<Vec<u32>>(), vec![1]);
        assert_eq!(main.take::<Vec<u8>>(), vec![7, 8]);
    }

    #[test]
    fn merge_from_keeps_existing_on_collision() {
        let mut main = OutputDeck::new();
        main.slot::<Vec<u32>>().push(1);

        let mut shard = OutputDeck::new();
        shard.slot::<Vec<u32>>().push(99);

        // Colliding slot type: the existing entry is kept (defensive).
        main.merge_from(shard);
        assert_eq!(main.take::<Vec<u32>>(), vec![1]);
    }
}
