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

//! What survives the frame.
//!
//! A behavior's fields, its current state and any suspended continuation live
//! here rather than in the frame arena, because all three outlast the frame
//! that created them. This is the other half of the no-collector design: the
//! arena can be freed blindly precisely because everything with a longer life
//! was put somewhere else on purpose.
//!
//! Scalars are stored inline. Arrays and strings are **owned copies**
//! ([`Owned`]), not arena references — a reference would be stale by the next
//! frame, and the whole point of this store is to hold what the arena cannot.
//! Storing one is therefore a copy, which is also what value semantics asks of
//! assigning one.

use serde::{Deserialize, Serialize};

use super::Owned;
use crate::vm::Value;

/// A value that outlives the frame it was made in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Persisted {
    /// A scalar, stored inline.
    Scalar(Value),
    /// An owned array or string.
    ///
    /// Owned rather than referenced: an arena handle would be stale the moment
    /// the frame ended, so keeping one here would store a guaranteed dangle.
    Owned(Owned),
}

/// The fields and state of one behavior instance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistentStore {
    slots: Vec<Persisted>,
}

impl PersistentStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// A store with `count` slots, each holding nothing.
    ///
    /// Sized up front from the behavior's field count, so a slot is never
    /// missing and a read never has to invent a default mid-frame.
    pub fn with_slots(count: usize) -> Self {
        Self {
            slots: vec![Persisted::Scalar(Value::Unit); count],
        }
    }

    /// How many slots it holds.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Reads a slot.
    pub fn get(&self, slot: usize) -> Option<&Persisted> {
        self.slots.get(slot)
    }

    /// Writes a slot, growing the store if it is short.
    ///
    /// Growing rather than failing matters for hot-reload: a script that gained
    /// a field should keep the values of the ones it already had, instead of
    /// being reset because its shape changed by one.
    pub fn set(&mut self, slot: usize, value: Persisted) {
        if slot >= self.slots.len() {
            self.slots.resize(slot + 1, Persisted::Scalar(Value::Unit));
        }
        self.slots[slot] = value;
    }

    /// Keeps an owned value in the store, so it survives the frame.
    pub fn store_owned(&mut self, slot: usize, owned: Owned) {
        self.set(slot, Persisted::Owned(owned));
    }

    /// Drops slots past `count`, keeping the rest.
    ///
    /// What hot-reload needs when a script *lost* a field: the surviving
    /// fields keep their values instead of the instance starting over.
    pub fn truncate(&mut self, count: usize) {
        self.slots.truncate(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_start_empty_and_hold_what_they_are_given() {
        let mut store = PersistentStore::with_slots(2);
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Unit)));

        store.set(0, Persisted::Scalar(Value::Int(7)));
        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Int(7))));
    }

    /// An array put here is a copy. Keeping an arena handle would store a
    /// reference that is guaranteed stale by the next frame.
    #[test]
    fn an_object_is_stored_by_value() {
        let mut store = PersistentStore::with_slots(1);
        let array = Owned::Array(vec![
            Owned::Scalar(Value::Int(1)),
            Owned::Scalar(Value::Int(2)),
        ]);
        store.store_owned(0, array.clone());

        assert_eq!(store.get(0), Some(&Persisted::Owned(array)));
    }

    /// Writing past the end grows the store: a script that gained a field
    /// should keep the values of the ones it already had.
    #[test]
    fn writing_past_the_end_grows_rather_than_failing() {
        let mut store = PersistentStore::with_slots(1);
        store.set(0, Persisted::Scalar(Value::Int(1)));
        store.set(3, Persisted::Scalar(Value::Int(4)));

        assert_eq!(store.len(), 4);
        assert_eq!(
            store.get(0),
            Some(&Persisted::Scalar(Value::Int(1))),
            "the existing field kept its value"
        );
        assert_eq!(store.get(3), Some(&Persisted::Scalar(Value::Int(4))));
        assert_eq!(
            store.get(1),
            Some(&Persisted::Scalar(Value::Unit)),
            "the gap is defined, not garbage"
        );
    }

    /// And losing a field keeps the others, rather than resetting the instance.
    #[test]
    fn truncating_keeps_the_surviving_fields() {
        let mut store = PersistentStore::with_slots(3);
        store.set(0, Persisted::Scalar(Value::Int(1)));
        store.set(2, Persisted::Scalar(Value::Int(3)));

        store.truncate(2);
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Int(1))));
        assert_eq!(store.get(2), None);
    }

    #[test]
    fn reading_past_the_end_is_none_rather_than_a_panic() {
        assert_eq!(PersistentStore::new().get(5), None);
    }
}
