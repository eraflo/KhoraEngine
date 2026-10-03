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

//! Internal entity storage and ID management.

use std::collections::HashSet;

use crate::ecs::entity::EntityMetadata;
use khora_core::ecs::entity::EntityId;

/// Internal manager for entity slots and metadata.
///
/// The `EntityStore` maintains a dense list of entity handles and their associated
/// metadata. It handles entity creation, recycling of indices via a free list,
/// and metadata access.
#[derive(Clone)]
pub(crate) struct EntityStore {
    /// A dense list of metadata for every entity slot that has ever been created.
    /// Each entry contains the current `EntityId` (including generation) and an
    /// `Option<EntityMetadata>` which is `Some` only if the entity is currently alive.
    pub(crate) entities: Vec<(EntityId, Option<EntityMetadata>)>,
    /// A list of entity indices available for reuse, enabling $O(1)$ allocation
    /// for previously despawned entities.
    pub(crate) freed_entities: Vec<u32>,
    /// Indices held back for a load: not alive, not on the free list, so
    /// neither a spawn nor a recycle can hand them out.
    reserved: HashSet<u32>,
    /// How many slots hold a live entity. Kept rather than counted: the DCC
    /// reads it every frame as the size of the world's workload.
    alive: usize,
}

impl EntityStore {
    /// Creates a new, empty `EntityStore`.
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            freed_entities: Vec::new(),
            reserved: HashSet::new(),
            alive: 0,
        }
    }

    /// Allocates a new or recycled `EntityId`.
    ///
    /// If there are indices in the `freed_entities` list, one is popped and its
    /// generation is incremented. Otherwise, a new slot is appended to the `entities` vector.
    /// In both cases, a default `EntityMetadata` is initialized in the slot.
    pub fn create_entity(&mut self) -> EntityId {
        self.alive += 1;
        if let Some(index) = self.freed_entities.pop() {
            let index = index as usize;
            let (id_slot, metadata_slot) = &mut self.entities[index];
            id_slot.generation += 1;
            *metadata_slot = Some(EntityMetadata::default());
            *id_slot
        } else {
            let index = self.entities.len() as u32;
            let new_id = EntityId {
                index,
                generation: 0,
            };
            self.entities
                .push((new_id, Some(EntityMetadata::default())));
            new_id
        }
    }

    /// Holds back an id: allocated like a spawn's, but not alive.
    pub fn reserve(&mut self) -> EntityId {
        let id = if let Some(index) = self.freed_entities.pop() {
            let (id_slot, _) = &mut self.entities[index as usize];
            id_slot.generation += 1;
            *id_slot
        } else {
            let id = EntityId {
                index: self.entities.len() as u32,
                generation: 0,
            };
            self.entities.push((id, None));
            id
        };
        self.reserved.insert(id.index);
        id
    }

    /// Brings a reserved id to life. `false` when `id` is not one this store
    /// is holding back.
    pub fn commit_reserved(&mut self, id: EntityId) -> bool {
        let held = self.reserved.contains(&id.index)
            && self
                .entities
                .get(id.index as usize)
                .is_some_and(|(slot, meta)| *slot == id && meta.is_none());
        if !held {
            return false;
        }
        self.reserved.remove(&id.index);
        self.entities[id.index as usize].1 = Some(EntityMetadata::default());
        self.alive += 1;
        true
    }

    /// Gives a reserved id back to the free list, never having lived.
    pub fn release_reserved(&mut self, id: EntityId) {
        let held = self
            .entities
            .get(id.index as usize)
            .is_some_and(|(slot, meta)| *slot == id && meta.is_none());
        if held && self.reserved.remove(&id.index) {
            self.freed_entities.push(id.index);
        }
    }

    /// Returns a mutable reference to an entity's metadata if the entity is alive.
    ///
    /// The generation of the provided `EntityId` must match the current generation in the store.
    pub fn get_metadata_mut(&mut self, id: EntityId) -> Option<&mut EntityMetadata> {
        self.entities
            .get_mut(id.index as usize)
            .and_then(|(slot_id, meta)| {
                if slot_id.generation == id.generation {
                    meta.as_mut()
                } else {
                    None
                }
            })
    }

    /// The number of live entities.
    pub fn alive(&self) -> usize {
        self.alive
    }

    /// Kills the live entity `id` names: takes its metadata, frees its index
    /// for recycling and returns the metadata. `None`, changing nothing, when
    /// `id` names no live entity (dead, stale generation, reserved, unknown).
    pub fn despawn(&mut self, id: EntityId) -> Option<EntityMetadata> {
        let (slot, metadata) = self.entities.get_mut(id.index as usize)?;
        if *slot != id {
            return None;
        }
        let metadata = metadata.take()?;
        self.freed_entities.push(id.index);
        self.alive -= 1;
        Some(metadata)
    }

    /// Returns the total number of entity slots (alive, dead and held back).
    /// The live count is [`alive`](Self::alive); this one is for tests that
    /// check slots are recycled rather than added.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Returns an iterator over all entity slots in the store.
    pub fn iter(&self) -> std::slice::Iter<'_, (EntityId, Option<EntityMetadata>)> {
        self.entities.iter()
    }

    /// Returns a reference to a specific entity slot by its raw index.
    pub fn get(&self, index: usize) -> Option<&(EntityId, Option<EntityMetadata>)> {
        self.entities.get(index)
    }

    /// Returns a mutable reference to a specific entity slot by its raw index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut (EntityId, Option<EntityMetadata>)> {
        self.entities.get_mut(index)
    }
}

#[cfg(test)]
mod tests {
    //! The live count: what is alive, not what was ever allocated.

    use super::*;

    /// Whether `id` names a live entity of `store`.
    fn is_alive(store: &EntityStore, id: EntityId) -> bool {
        store
            .get(id.index as usize)
            .is_some_and(|(slot, meta)| *slot == id && meta.is_some())
    }

    #[test]
    fn an_empty_store_has_nothing_alive() {
        assert_eq!(EntityStore::new().alive(), 0);
    }

    #[test]
    fn every_created_entity_is_alive() {
        let mut store = EntityStore::new();
        for expected in 1..=5 {
            store.create_entity();
            assert_eq!(store.alive(), expected);
        }
    }

    #[test]
    fn despawning_a_live_entity_returns_its_metadata_and_counts_one_less() {
        let mut store = EntityStore::new();
        let a = store.create_entity();
        let b = store.create_entity();
        let c = store.create_entity();

        assert!(store.despawn(b).is_some(), "a live entity is despawned");
        assert_eq!(store.alive(), 2);
        assert!(!is_alive(&store, b), "a despawned entity is dead");
        assert!(
            is_alive(&store, a) && is_alive(&store, c),
            "the others live on"
        );
    }

    #[test]
    fn despawning_twice_counts_once() {
        let mut store = EntityStore::new();
        let a = store.create_entity();
        store.create_entity();

        assert!(store.despawn(a).is_some());
        assert!(store.despawn(a).is_none(), "a dead entity cannot die again");
        assert_eq!(store.alive(), 1);
    }

    #[test]
    fn despawning_twice_frees_the_index_once() {
        let mut store = EntityStore::new();
        let a = store.create_entity();
        assert!(store.despawn(a).is_some());
        assert!(store.despawn(a).is_none());

        // One free index, so the second creation allocates a new slot rather
        // than handing the same index out twice.
        let first = store.create_entity();
        let second = store.create_entity();
        assert_ne!(first.index, second.index, "an index was freed twice");
        assert_eq!(store.alive(), 2);
    }

    #[test]
    fn despawning_a_stale_id_changes_nothing() {
        let mut store = EntityStore::new();
        let old = store.create_entity();
        assert!(store.despawn(old).is_some());
        let recycled = store.create_entity();
        assert_eq!(recycled.index, old.index, "the freed index is recycled");
        assert_ne!(recycled.generation, old.generation);

        assert!(
            store.despawn(old).is_none(),
            "an id of an earlier generation names no live entity"
        );
        assert_eq!(store.alive(), 1);
        assert!(
            is_alive(&store, recycled),
            "the recycled entity is untouched"
        );
    }

    #[test]
    fn despawning_an_unknown_index_changes_nothing() {
        let mut store = EntityStore::new();
        store.create_entity();
        let unknown = EntityId {
            index: 42,
            generation: 0,
        };
        assert!(store.despawn(unknown).is_none());
        assert_eq!(store.alive(), 1);
        assert_eq!(store.len(), 1, "no slot is allocated for an unknown id");
    }

    #[test]
    fn a_despawned_index_is_recycled_with_a_new_generation() {
        let mut store = EntityStore::new();
        let a = store.create_entity();
        store.create_entity();
        assert!(store.despawn(a).is_some());

        let recycled = store.create_entity();
        assert_eq!(recycled.index, a.index);
        assert_eq!(recycled.generation, a.generation + 1);
        assert_eq!(store.alive(), 2);
        assert_eq!(store.len(), 2, "recycling allocates no new slot");
    }

    #[test]
    fn recycling_many_times_keeps_the_count() {
        let mut store = EntityStore::new();
        let keep = store.create_entity();
        for _ in 0..10 {
            let churn = store.create_entity();
            assert_eq!(store.alive(), 2);
            assert!(store.despawn(churn).is_some());
            assert_eq!(store.alive(), 1);
        }
        assert!(is_alive(&store, keep));
        assert_eq!(store.len(), 2, "every churned entity reused one slot");
    }

    #[test]
    fn a_reserved_id_is_not_alive() {
        let mut store = EntityStore::new();
        store.create_entity();
        store.reserve();
        store.reserve();
        assert_eq!(store.alive(), 1);
    }

    #[test]
    fn a_reserved_id_cannot_be_despawned() {
        let mut store = EntityStore::new();
        store.create_entity();
        let held = store.reserve();

        assert!(store.despawn(held).is_none(), "a reserved id is not alive");
        assert_eq!(store.alive(), 1);
        assert!(
            store.commit_reserved(held),
            "a refused despawn leaves the reservation in place"
        );
    }

    #[test]
    fn committing_a_reserved_id_counts_it() {
        let mut store = EntityStore::new();
        store.create_entity();
        let held = store.reserve();

        assert!(store.commit_reserved(held));
        assert_eq!(store.alive(), 2);
        assert!(
            !store.commit_reserved(held),
            "a committed id is no longer held"
        );
        assert_eq!(store.alive(), 2, "a refused commit does not count");
    }

    #[test]
    fn releasing_a_reserved_id_changes_nothing_alive() {
        let mut store = EntityStore::new();
        store.create_entity();
        let held = store.reserve();
        store.release_reserved(held);
        assert_eq!(store.alive(), 1);

        // Reserved on a recycled index, then released.
        let dead = store.create_entity();
        assert!(store.despawn(dead).is_some());
        let held = store.reserve();
        assert_eq!(held.index, dead.index, "the reservation recycles the index");
        store.release_reserved(held);
        assert_eq!(store.alive(), 1);
    }

    #[test]
    fn a_committed_reservation_can_be_despawned() {
        let mut store = EntityStore::new();
        let held = store.reserve();
        assert!(store.commit_reserved(held));
        assert!(store.despawn(held).is_some());
        assert_eq!(store.alive(), 0);
    }

    #[test]
    fn the_count_matches_the_live_slots_after_a_mixed_sequence() {
        let mut store = EntityStore::new();
        let a = store.create_entity();
        let b = store.create_entity();
        let held = store.reserve();
        let c = store.create_entity();
        assert!(store.despawn(b).is_some());
        let released = store.reserve();
        assert!(store.commit_reserved(held));
        store.release_reserved(released);
        store.create_entity();
        assert!(store.despawn(a).is_some());
        assert!(store.despawn(a).is_none());
        assert!(store.despawn(c).is_some());
        store.create_entity();

        let live = store.iter().filter(|(_, meta)| meta.is_some()).count();
        assert_eq!(store.alive(), live);
        assert_eq!(live, 3);
    }
}
