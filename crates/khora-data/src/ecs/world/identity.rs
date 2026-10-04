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

//! Each entity's persistent identity, and entity ids reserved ahead of a load.
//!
//! The world keeps, beside each live entity, the identity a save knows it
//! by. It is not a component: identity is not gameplay data, and a query
//! should never see it. The editor tags what an author creates; a load
//! restores what it saved; any other entity is given one of the created
//! namespace the first time something needs it — a save, a lookup — so a
//! spawn and a despawn nobody identifies cost no identity at all.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::World;

/// Which identity each live entity has, both ways.
#[derive(Debug, Clone, Default)]
pub(crate) struct PersistentIds {
    by_entity: HashMap<EntityId, PersistentId>,
    by_id: HashMap<PersistentId, EntityId>,
    /// The number the next created identity takes. Always past every created
    /// identity the world holds, so a spawn never repeats one a load restored.
    next_created: u64,
}

impl PersistentIds {
    /// Gives `entity` the next created identity no one holds.
    pub(crate) fn assign_created(&mut self, entity: EntityId) -> PersistentId {
        let id = loop {
            let candidate = PersistentId::created(self.next_created);
            self.next_created += 1;
            if !self.by_id.contains_key(&candidate) {
                break candidate;
            }
        };
        self.bind(entity, id);
        id
    }

    /// Forgets a despawned entity.
    pub(crate) fn forget(&mut self, entity: EntityId) {
        if let Some(id) = self.by_entity.remove(&entity) {
            self.by_id.remove(&id);
        }
    }

    fn bind(&mut self, entity: EntityId, id: PersistentId) {
        self.forget(entity);
        if let Some(previous) = self.by_id.insert(id, entity) {
            // The identity moves: whoever held it is left without one until
            // given another.
            self.by_entity.remove(&previous);
        }
        self.by_entity.insert(entity, id);
        if id.is_created() {
            let number = id.to_bits() & !PersistentId::created(0).to_bits();
            self.next_created = self.next_created.max(number.saturating_add(1));
        }
    }
}

/// The world's identities, behind the lock that lets a `&World` give an
/// entity its identity the first time it is asked.
#[derive(Debug, Default)]
pub(crate) struct IdentityCell(Mutex<PersistentIds>);

impl IdentityCell {
    /// The identities, through a shared world. A panic while the lock was
    /// held leaves the maps whole — every write is one insert or remove —
    /// so a poisoned lock is taken as it is.
    fn lock(&self) -> MutexGuard<'_, PersistentIds> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The identities, through an exclusive world: no lock to take.
    pub(crate) fn get_mut(&mut self) -> &mut PersistentIds {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

impl World {
    /// The identity a save knows `entity` by — given now, from the created
    /// namespace, if `entity` is alive and has none yet. `None` for an entity
    /// that is not alive.
    ///
    /// Numbering on first ask rather than at spawn keeps spawning free of
    /// identity work: most entities a game spawns are never saved or looked
    /// up by identity.
    pub fn persistent_id(&self, entity: EntityId) -> Option<PersistentId> {
        let mut ids = self.identity.lock();
        if let Some(id) = ids.by_entity.get(&entity) {
            return Some(*id);
        }
        self.contains(entity).then(|| ids.assign_created(entity))
    }

    /// The live entity known by `id` — among the entities given an identity.
    /// One never asked for its identity, saved or loaded has none, so no id
    /// finds it.
    pub fn entity_with_id(&self, id: PersistentId) -> Option<EntityId> {
        self.identity.lock().by_id.get(&id).copied()
    }

    /// Re-tags `entity` as authored with a random identity, and returns it.
    ///
    /// An entity already authored keeps its identity.
    pub fn mark_authored(&mut self, entity: EntityId) -> Option<PersistentId> {
        if !self.contains(entity) {
            return None;
        }
        // Read without asking: re-tagging needs no created number first.
        let ids = self.identity.get_mut();
        if let Some(id) = ids.by_entity.get(&entity) {
            if !id.is_created() {
                return Some(*id);
            }
        }
        let id = loop {
            let candidate = PersistentId::random_authored();
            if !ids.by_id.contains_key(&candidate) {
                break candidate;
            }
        };
        ids.bind(entity, id);
        Some(id)
    }

    /// Gives `entity` the identity `id`, as a load does.
    ///
    /// An entity that held `id` before loses it: an identity names one entity.
    pub fn set_persistent_id(&mut self, entity: EntityId, id: PersistentId) {
        if self.contains(entity) {
            self.identity.get_mut().bind(entity, id);
        }
    }

    /// An entity id held back for a load: not alive, not handed to anyone
    /// else, until it is spawned or released.
    pub fn reserve_entity(&mut self) -> EntityId {
        self.entities.reserve()
    }

    /// Brings a reserved id to life, empty — an entity like a spawned one,
    /// identified when first asked. Returns `false` when `entity` is not an
    /// id this world is holding back.
    pub fn spawn_reserved(&mut self, entity: EntityId) -> bool {
        // The entity a reference to nothing names stays nothing.
        if self.nowhere == Some(entity) {
            return false;
        }
        self.bring_reserved_to_life(entity, true)
    }

    /// Brings a reserved id to life with no identity yet — the load that
    /// reserved it gives it one next, once it knows which saved identities
    /// are free. `empty` places it as an entity with nothing on it, otherwise
    /// it waits for the page rows the load is about to give it.
    pub(crate) fn bring_reserved_to_life(&mut self, entity: EntityId, empty: bool) -> bool {
        if !self.entities.commit_reserved(entity) {
            return false;
        }
        if empty {
            self.place_empty(entity);
        }
        true
    }

    /// Gives `entity` the next created identity no one holds.
    pub(crate) fn assign_created_id(&mut self, entity: EntityId) {
        if self.contains(entity) {
            self.identity.get_mut().assign_created(entity);
        }
    }

    /// Forgets the identity of a despawned entity, if it had one.
    pub(crate) fn forget_identity(&mut self, entity: EntityId) {
        self.identity.get_mut().forget(entity);
    }

    /// The entity a reference to nothing names.
    ///
    /// A load that meets a reference it cannot bind — to an entity the save
    /// does not hold, or one written as outside a saved subtree — still has
    /// to give the component an id. It gives this one: reserved once per
    /// world, never alive, never handed to a spawn, so such a reference can
    /// never come to name a live entity, however many loads make one.
    /// The entity a reference to nothing names in this world, if a load has
    /// made one.
    pub fn nowhere_entity(&self) -> Option<EntityId> {
        self.nowhere
    }

    pub(crate) fn nowhere(&mut self) -> EntityId {
        if let Some(entity) = self.nowhere {
            return entity;
        }
        let entity = self.reserve_entity();
        self.nowhere = Some(entity);
        entity
    }

    /// Gives a reserved id back, never having lived. The entity a reference
    /// to nothing names is never given back: references hold it.
    pub fn release_reserved(&mut self, entity: EntityId) {
        if self.nowhere == Some(entity) {
            return;
        }
        self.entities.release_reserved(entity);
    }
}
