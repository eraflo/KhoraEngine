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
//! - **created**: entities spawned without an author's say — by the game while
//!   running, or by code — numbered by the world as it spawns them.

use serde::{Deserialize, Serialize};

/// The bit that marks the created namespace.
const CREATED: u64 = 1 << 63;

/// The namespace ids within prefab instances are derived in. Fixed for
/// ever: changing it would re-key every instance in every saved scene.
const WITHIN: uuid::Uuid = uuid::Uuid::from_u128(0x6b68_6f72_615f_7769_7468_696e_5f69_6430);

/// An entity's identity within the scene or save that records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersistentId(u64);

impl PersistentId {
    /// An authored identity from 63 random bits; the top bit is ignored.
    pub fn authored(bits: u64) -> Self {
        Self(bits & !CREATED)
    }

    /// A fresh authored identity, drawn at random.
    pub fn random_authored() -> Self {
        Self::authored(uuid::Uuid::new_v4().as_u64_pair().0)
    }

    /// The identity of the entity a prefab knows as `inner`, inside the
    /// instance whose root is `instance`: authored, the same on every load,
    /// and different for every instance and every inner entity. Nested
    /// instances compose: `within(within(instance, nested_root), inner)`.
    pub fn within(instance: PersistentId, inner: PersistentId) -> PersistentId {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&instance.0.to_le_bytes());
        bytes[8..].copy_from_slice(&inner.0.to_le_bytes());
        Self::authored(uuid::Uuid::new_v5(&WITHIN, &bytes).as_u64_pair().0)
    }

    /// The `n`-th identity of the created namespace.
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

    /// The editor gives every entity an author creates a random identity, so
    /// that two people adding entities on two branches do not collide. A
    /// random draw is always authored — whatever the top bit came out as — and
    /// draws do not repeat.
    #[test]
    fn a_random_authored_id_is_authored_and_does_not_repeat() {
        let mut seen = std::collections::HashSet::new();
        let mut top_bits_used = 0u64;
        for _ in 0..1000 {
            let id = PersistentId::random_authored();
            assert!(!id.is_created(), "{id:?} reads as created");
            assert_eq!(id.to_bits() & TOP_BIT, 0);
            assert!(seen.insert(id), "{id:?} was drawn twice");
            top_bits_used |= id.to_bits();
        }
        // Random, not a counter: the high bits of the authored space are used.
        assert_ne!(
            top_bits_used >> 32,
            0,
            "a thousand draws never reached the high half of the id space"
        );
    }

    /// An entity inside a prefab instance is known by the instance's root and
    /// its own id in the prefab. The derived id is the same however often it
    /// is asked for — that is what lets a reference into an instance survive
    /// a reload — and it is authored: a scene's author placed the instance.
    #[test]
    fn an_id_within_an_instance_is_stable_and_authored() {
        let instance = PersistentId::authored(0x1234_5678_9abc_def0);
        for inner in [
            PersistentId::authored(0),
            PersistentId::authored(7),
            PersistentId::authored(TOP_BIT - 1),
            PersistentId::created(3),
        ] {
            let derived = PersistentId::within(instance, inner);
            assert_eq!(derived, PersistentId::within(instance, inner));
            assert!(!derived.is_created(), "{derived:?} reads as created");
            assert_eq!(derived.to_bits() & TOP_BIT, 0);
            assert_ne!(derived, instance, "a member is not the instance root");
            assert_ne!(derived, inner, "a member is not the prefab's own entity");
        }
    }

    /// Two instances of one prefab never share an id, and two entities of
    /// one instance never do: neither the instance nor the inner id may be
    /// dropped from the derivation. Order matters too — the instance root and
    /// the inner id are not interchangeable.
    #[test]
    fn ids_within_instances_are_distinct_per_instance_and_per_entity() {
        let mut seen = std::collections::HashSet::new();
        let instances: Vec<PersistentId> = (1..=16)
            .map(|n| PersistentId::authored(n * 0x0101_0101_0101))
            .collect();
        let inners: Vec<PersistentId> = (1..=16)
            .map(|n| PersistentId::authored(n * 0x7777_0000_1111))
            .collect();
        for instance in &instances {
            for inner in &inners {
                let derived = PersistentId::within(*instance, *inner);
                assert!(
                    seen.insert(derived),
                    "{derived:?} is derived twice ({instance:?}, {inner:?})"
                );
            }
        }
        for id in instances.iter().chain(&inners) {
            assert!(!seen.contains(id), "a derived id collides with {id:?}");
        }

        let (a, b) = (PersistentId::authored(11), PersistentId::authored(22));
        assert_ne!(PersistentId::within(a, b), PersistentId::within(b, a));
    }

    /// A nested instance's members are derived twice: from the outer
    /// instance to the nested root, then from the nested root to the member.
    /// The composition is an id of its own — not the member's id in the
    /// outer instance, nor in the nested prefab alone.
    #[test]
    fn ids_within_nested_instances_compose() {
        let outer = PersistentId::authored(0xaaaa);
        let nested_root = PersistentId::authored(0xbbbb);
        let member = PersistentId::authored(0xcccc);

        let nested = PersistentId::within(PersistentId::within(outer, nested_root), member);
        assert!(!nested.is_created());
        assert_ne!(nested, PersistentId::within(outer, member));
        assert_ne!(nested, PersistentId::within(nested_root, member));
        assert_eq!(
            nested,
            PersistentId::within(PersistentId::within(outer, nested_root), member)
        );
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
