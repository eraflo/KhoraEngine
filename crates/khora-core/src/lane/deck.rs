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
//! Symmetric counterpart of [`LaneBus`](super::LaneBus): lanes write typed
//! outputs into the deck during the CLAD descent; the engine drains specific
//! types at the I/O boundary (e.g. recorded GPU command buffers, draw lists).
//!
//! # Visibility contract
//!
//! - [`OutputDeck::slot`] returns a `&mut T` for lanes to push into.
//! - [`OutputDeck::take`] is `pub(crate)` (used by the scheduler to expose
//!   typed outputs to the engine I/O layer in a controlled way).

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Mutable typed deck collecting lane outputs for one tick.
///
/// Each typed slot is created on first access, holding `T::default()` until
/// a lane writes into it. The engine drains specific types at the I/O
/// boundary (submit, present) via [`OutputDeck::take`].
pub struct OutputDeck {
    slots: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl OutputDeck {
    /// Creates an empty deck.
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }

    /// Returns a mutable reference to the slot for type `T`, creating it
    /// with `T::default()` on first access.
    ///
    /// Lanes call this to accumulate outputs (e.g. push command buffers
    /// onto a `Vec`).
    pub fn slot<T: Default + Any + Send + Sync>(&mut self) -> &mut T {
        self.slots
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut::<T>()
            .expect("slot type mismatch — TypeId collision is impossible in safe Rust")
    }

    /// Removes and returns the slot for type `T`, replacing it with the
    /// default value (so subsequent ticks start clean).
    ///
    /// Returns `T::default()` if no lane wrote into this slot during the
    /// tick. Used by the engine at the I/O boundary.
    pub fn take<T: Default + Any + Send + Sync>(&mut self) -> T {
        self.slots
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok().map(|b| *b))
            .unwrap_or_default()
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

    /// Clears all slots. Called by the scheduler between ticks if a deck
    /// instance is reused rather than reallocated.
    pub fn clear(&mut self) {
        self.slots.clear();
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
}
