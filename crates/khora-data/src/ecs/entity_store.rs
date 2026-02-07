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
}

impl EntityStore {
    /// Creates a new, empty `EntityStore`.
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            freed_entities: Vec::new(),
        }
    }

    /// Allocates a new or recycled `EntityId`.
    ///
    /// If there are indices in the `freed_entities` list, one is popped and its
    /// generation is incremented. Otherwise, a new slot is appended to the `entities` vector.
    /// In both cases, a default `EntityMetadata` is initialized in the slot.
    pub fn create_entity(&mut self) -> EntityId {
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

    /// Returns the total number of entity slots (both alive and dead).
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
