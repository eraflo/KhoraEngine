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

//! The identity an entity keeps across saves.
//!
//! An [`EntityId`](super::entity::EntityId) is where an entity sits in one
//! running world: its index is recycled and its generation moves, so it means
//! nothing once the world is gone. A saved scene needs an identity that does
//! not depend on where the entity happens to sit, nor on anything its author
//! edits — a name changes, a parent changes — and that two people adding
//! entities on two branches will not both hand out.
//!
//! Two namespaces share the 64 bits, told apart by the top bit:
//!
//! - **authored**: entities a scene was built with, given a random 63-bit id
//!   when they are created in the editor — random so that concurrent edits do
//!   not collide the way a counter would;
//! - **created**: entities the game spawned while running, numbered by the save
//!   that first records them.

use serde::{Deserialize, Serialize};

/// The bit that marks the created namespace.
const CREATED: u64 = 1 << 63;

/// An entity's identity within the scene or save that records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersistentId(u64);

impl PersistentId {
    /// An authored identity from 63 random bits; the top bit is ignored.
    pub fn authored(bits: u64) -> Self {
        Self(bits & !CREATED)
    }

    /// The `n`-th identity a save hands to an entity the game created.
    pub fn created(n: u64) -> Self {
        Self(n | CREATED)
    }

    /// Whether the game created this entity rather than an author.
    pub fn is_created(self) -> bool {
        self.0 & CREATED != 0
    }

    /// The raw 64 bits, as a file stores them.
    pub fn to_bits(self) -> u64 {
        self.0
    }

    /// The identity a file stored as `bits`.
    pub fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP_BIT: u64 = 1 << 63;

    /// An authored id and a created id can never be the same identity, even
    /// from the same number: a scene's authored entities and the entities a
    /// save numbered while the game ran must not collide on load. The top bit
    /// is what tells them apart, so an authored id drops it whatever it is
    /// given — random bits include it half of the time.
    #[test]
    fn authored_and_created_ids_live_in_separate_namespaces() {
        for n in [0, 1, 7, 0x1234_5678_9abc_def0, TOP_BIT - 1] {
            let authored = PersistentId::authored(n);
            let created = PersistentId::created(n);
            assert!(!authored.is_created(), "authored({n:#x}) reads as created");
            assert!(created.is_created(), "created({n:#x}) reads as authored");
            assert_ne!(authored, created, "authored and created collide on {n:#x}");
            assert_eq!(authored.to_bits() & TOP_BIT, 0);
            assert_eq!(created.to_bits() & TOP_BIT, TOP_BIT);
        }

        // Random bits with the top bit set still land in the authored space,
        // and name the same identity as the same bits without it.
        for bits in [u64::MAX, TOP_BIT, TOP_BIT | 42] {
            let authored = PersistentId::authored(bits);
            assert!(
                !authored.is_created(),
                "authored({bits:#x}) reads as created"
            );
            assert_eq!(authored, PersistentId::authored(bits & !TOP_BIT));
            assert_eq!(authored.to_bits(), bits & !TOP_BIT);
        }

        // Created ids are numbered: distinct numbers, distinct identities, and
        // the number is what the low bits keep.
        assert_ne!(PersistentId::created(1), PersistentId::created(2));
        assert_eq!(PersistentId::created(5).to_bits() & !TOP_BIT, 5);
        assert_eq!(PersistentId::authored(5).to_bits(), 5);
    }

    /// A file stores the raw 64 bits; reading them back must give the very
    /// identity that was written, in either namespace, for every bit pattern
    /// a file can hold.
    #[test]
    fn persistent_id_bits_round_trip() {
        for bits in [0, 1, 42, TOP_BIT - 1, TOP_BIT, TOP_BIT | 9, u64::MAX] {
            assert_eq!(PersistentId::from_bits(bits).to_bits(), bits);
        }
        for id in [
            PersistentId::authored(0),
            PersistentId::authored(0x0bad_cafe),
            PersistentId::authored(u64::MAX),
            PersistentId::created(0),
            PersistentId::created(3),
            PersistentId::created(TOP_BIT - 1),
        ] {
            let back = PersistentId::from_bits(id.to_bits());
            assert_eq!(back, id);
            assert_eq!(back.is_created(), id.is_created());
        }
        assert!(PersistentId::from_bits(TOP_BIT | 3).is_created());
        assert!(!PersistentId::from_bits(3).is_created());
    }
}
