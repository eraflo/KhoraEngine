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

//! The heart of the CRPECS: the `World` struct.

use std::{any::TypeId, collections::HashSet};

use khora_core::asset::font::Font;
use khora_core::asset::Material;
use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::gpu_scene::Mesh;

use crate::ecs::component_registry::ComponentRegistry;
use crate::ecs::components::HandleComponent;
use crate::ecs::entity_store::EntityStore;
use crate::ecs::page::PageIndex;
use crate::ecs::planner::QueryPlanner;
use crate::ecs::storage::StorageManager;
use crate::ecs::{
    ComponentBundle, ComponentKey, LayoutPolicy, MaterialRef, MeshRef, SemanticDomain,
};

mod clock;
mod column_write;
mod compaction;
mod component_access;
mod domain_stats;
mod hierarchy;
mod identity;
mod lost_value;
mod migration;
mod projection;
mod queries;
mod relayout_report;
mod reservations;
mod rows;
mod runtime;
mod spawn_error;

pub use column_write::ColumnWriteReport;
pub use component_access::AddComponentError;
pub use component_access::RemoveComponentError;
pub use domain_stats::DomainStats;
pub use hierarchy::HierarchyWrite;
pub(crate) use hierarchy::LoadedHierarchy;
pub use lost_value::{LossCause, LostValue};
pub use projection::BehaviorComponent;
pub use relayout_report::RelayoutReport;
pub use rows::{RowMut, RowRef};
pub use runtime::{RegisterError, RowError, RuntimeComponentDecl};
pub use spawn_error::SpawnError;

/// Source of process-unique [`World::instance_id`] values.
static WORLD_INSTANCE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The central container for the entire ECS, holding all entities, components, and metadata.
pub struct World {
    /// Manages entity IDs and metadata.
    pub(crate) entities: EntityStore,
    /// Manages component storage and pages.
    pub(crate) storage: StorageManager,
    /// Manages query planning and caching.
    pub(crate) planner: QueryPlanner,
    /// Per-domain change epochs, indexed by [`SemanticDomain::index`].
    ///
    /// See the book: concepts/change-detection-formal.md, "Domain epochs".
    domain_epochs: [u64; SemanticDomain::COUNT],
    /// The persistent identity of each live entity that has been given one.
    identity: identity::IdentityCell,
    /// The entity a reference to nothing names, once one is needed — see
    /// [`World::nowhere`].
    nowhere: Option<EntityId>,
    /// Ids a script reserves in its turn, for this world to commit or take
    /// back at the frame's end.
    reserver: std::sync::Arc<khora_core::ecs::EntityReserver>,
    /// Process-unique id of this `World` instance; see [`World::instance_id`].
    instance_id: u64,
    /// The world's clock, the keys observers watch, and each observer's
    /// last-seen tick.
    clock: clock::WorldClock,
    /// The load catalogue last built, rebuilt only once the components
    /// changed.
    load_catalog: std::sync::Mutex<Option<std::sync::Arc<crate::scene::LoadCatalog>>>,
}

impl World {
    /// Allocates a new or recycled `EntityId` and reserves its metadata slot.
    fn create_entity(&mut self) -> EntityId {
        // What a script reserved first, so a spawn here never takes an index
        // a reservation already holds.
        self.settle_reservations();
        let entity = if self.entities.has_freed() {
            self.entities.create_entity()
        } else if let Some(ready) = self.reserver.take_ready() {
            // A slot held back for the reserver — left ready by a load or a
            // frame — filled before the store grows. A fresh index a hold
            // gave back from another thread since the settle above has no
            // slot yet: settled now.
            self.settle_reservations();
            self.entities.commit_reserved(ready);
            ready
        } else {
            // A slot past the end of the store: its index claimed from the
            // reserver first, so a reservation made from another thread
            // meanwhile — a load prepared off the frame — never gets it too.
            let entity = EntityId {
                index: self.reserver.claim_fresh(),
                generation: 0,
            };
            self.settle_reservations();
            self.entities.commit_reserved(entity);
            entity
        };
        self.after_allocation();
        entity
    }

    fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        self.storage.find_or_create_page_for_bundle::<B>()
    }

    /// Removes the row at `location` by swap, repointing the moved entity's locations at it.
    ///
    /// Takes a row, not an entity: whose row it was never matters. Every
    /// location of the moved entity that addressed the old last row is
    /// repointed, one per domain of a mixed-domain bundle. A moved migration
    /// orphan whose owner is dead is left for compaction.
    fn remove_from_page(&mut self, location: PageIndex) {
        let page = &mut self.storage.pages[location.page_id as usize];
        if page.entities.is_empty() {
            return;
        }

        let old_last_row = (page.entities.len() - 1) as u32;
        let moved_entity = page.entities[old_last_row as usize];
        page.swap_remove_row(location.row_index);
        self.note_page_changed(location.page_id);

        // Removing the last row moves nothing.
        if location.row_index == old_last_row {
            return;
        }

        // Compared by row, never by entity id: an entity that left this page
        // and came back holds an orphan row and a live row here, both under
        // its id. A dead or recycled owner fails the generation check and its
        // orphan row is left for compaction.
        if let Some(metadata) = self.entities.get_metadata_mut(moved_entity) {
            for loc in metadata.locations.values_mut() {
                if loc.page_id == location.page_id && loc.row_index == old_last_row {
                    *loc = location;
                }
            }
        }
    }

    /// Finds or creates the page of `signature`, sorted component keys.
    fn find_or_create_page_for_signature(&mut self, signature: &[ComponentKey]) -> u32 {
        self.storage.find_or_create_page_for_signature(signature)
    }

    /// Creates a new, empty `World` with pre-registered internal component types.
    pub fn new() -> Self {
        let mut world = Self {
            entities: EntityStore::new(),
            storage: StorageManager::new(ComponentRegistry::default()),
            planner: QueryPlanner::new(),
            domain_epochs: [0; SemanticDomain::COUNT],
            identity: identity::IdentityCell::default(),
            nowhere: None,
            reserver: std::sync::Arc::new(khora_core::ecs::EntityReserver::new()),
            instance_id: WORLD_INSTANCE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            clock: clock::WorldClock::default(),
            load_catalog: std::sync::Mutex::new(None),
        };
        // The entity a reference to nothing names exists from the start: a
        // load prepared off the frame names it without touching the world.
        world.nowhere();
        // Every key a registration watches is observed from the first frame.
        for reg in inventory::iter::<crate::ecs::DataSystemRegistration> {
            for key in reg.observes {
                world.observe(key());
            }
        }
        for reg in inventory::iter::<crate::flow::FlowRegistration> {
            for key in reg.observes {
                world.observe(key());
            }
        }
        // Generic and hand-implemented components can't self-register via the
        // derive: generics have no single `TypeId`, and `MaterialRef` has a
        // manual `Component` impl.
        world.register_component::<HandleComponent<Mesh>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<Box<dyn Material>>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<Font>>(SemanticDomain::Ui);
        world.register_component::<MaterialRef>(SemanticDomain::Render);
        world.register_component::<MeshRef>(SemanticDomain::Render);

        // Idempotent with the explicit calls above: same type, same domain.
        for reg in inventory::iter::<crate::ecs::ComponentDomainRegistration> {
            (reg.register)(&mut world);
        }

        world
    }

    /// Returns the [`SemanticDomain`] a component type was registered with, if any.
    pub fn component_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.storage.registry.get_domain(type_id)
    }

    /// Returns the [`LayoutPolicy`] a component type is currently stored with.
    ///
    /// `Soa` by default; the layout-adaptation pass may change it.
    pub fn component_layout(&self, type_id: TypeId) -> Option<LayoutPolicy> {
        self.storage.registry.layout_of(type_id)
    }

    /// Returns a component type's access stats as `(query_count, rows_scanned)`.
    ///
    /// Read to drive layout adaptation. See the book: concepts/agdf.md,
    /// "Layer 1 — observe and advise".
    pub fn component_access_stats(&self, type_id: TypeId) -> Option<(u64, u64)> {
        self.storage.registry.access_stats(type_id)
    }

    /// Returns the number of live entities, in O(1).
    ///
    /// The coarse workload size `n` the DCC's cost model fits execution time against.
    pub fn entity_count(&self) -> usize {
        self.entities.alive()
    }

    /// Returns every registered component's `(name, size_bytes, query_count, rows_scanned)`.
    ///
    /// Keyed by the component's short name. See the book: concepts/agdf.md,
    /// "Layer 1 — observe and advise".
    pub fn component_access_snapshot(&self) -> Vec<(String, usize, u64, u64)> {
        self.storage
            .registry
            .access_snapshot()
            .into_iter()
            .map(|(name, size, qc, rows)| (name.to_string(), size, qc, rows))
            .collect()
    }

    /// Returns `domain`'s change epoch: monotone, read in O(1).
    ///
    /// Equal epochs across two reads guarantee the domain's data and its
    /// iteration order are unchanged; a different value means only "possibly
    /// changed". See the book: concepts/change-detection-formal.md, "Domain epochs".
    pub fn domain_epoch(&self, domain: SemanticDomain) -> u64 {
        self.domain_epochs[domain.index()]
    }

    /// Returns the process-unique identifier of this `World` instance.
    ///
    /// A cache key built from [`domain_epoch`](Self::domain_epoch) includes it,
    /// so epochs of two worlds (a fresh one restarts at zero) never compare equal.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Raises `domain`'s epoch to the clock's tick.
    pub(crate) fn bump_domain_epoch(&mut self, domain: SemanticDomain) {
        let epoch = &mut self.domain_epochs[domain.index()];
        *epoch = (*epoch).max(self.clock.tick.0);
    }

    /// Spawns a new entity holding `bundle` and returns its id.
    ///
    /// # Panics
    ///
    /// If a component type of `bundle` is not registered, or `bundle` names one
    /// type twice; [`try_spawn`](Self::try_spawn) returns the error instead.
    /// See the book: concepts/ecs.md, "Registration".
    pub fn spawn<B: ComponentBundle>(&mut self, bundle: B) -> EntityId {
        match self.try_spawn(bundle) {
            Ok(entity) => entity,
            Err(SpawnError::ComponentNotRegistered { component }) => panic!(
                "`{component}` is not registered: derive `Component` with a domain, or register it in `World::new`"
            ),
            Err(SpawnError::ComponentNamedTwice { component }) => {
                panic!("a bundle names `{component}` twice: an entity holds one of each")
            }
        }
    }

    /// Spawns a new entity holding `bundle`, or refuses it before anything is allocated.
    ///
    /// # Errors
    ///
    /// [`SpawnError`] if a component type of `bundle` is not registered, or
    /// `bundle` names one type twice.
    pub fn try_spawn<B: ComponentBundle>(&mut self, bundle: B) -> Result<EntityId, SpawnError> {
        if let Some(component) = B::unregistered(&self.storage.registry) {
            return Err(SpawnError::ComponentNotRegistered { component });
        }
        if let Some(component) = B::repeated() {
            return Err(SpawnError::ComponentNamedTwice { component });
        }
        // Its persistent identity waits until something asks for it.
        let entity_id = self.create_entity();
        self.place(entity_id, bundle);
        Ok(entity_id)
    }

    /// Places a freshly allocated entity, with nothing on it.
    pub(crate) fn place_empty(&mut self, entity_id: EntityId) {
        self.place(entity_id, ());
    }

    /// Stores `bundle` as the row of an entity whose id is already allocated.
    fn place<B: ComponentBundle>(&mut self, entity_id: EntityId, bundle: B) {
        let page_id = self.find_or_create_page_for_bundle::<B>();

        let row_index;
        {
            let page = &mut self.storage.pages[page_id as usize];
            row_index = page.entities.len() as u32;
            // SAFETY: `page` was just resolved by `find_or_create_page_for_bundle::<B>()`
            // (line above), which guarantees its column layout matches `B::component_types()`.
            // `bundle.add_to_page` requires that every column type in the bundle has a
            // matching column on the page; that invariant is upheld by the page-discovery
            // step. Exclusive access is held via `&mut page`.
            unsafe {
                bundle.add_to_page(page);
            }
            let now = self.clock.tick;
            page.add_entity(entity_id, &self.clock.observed, |_| now);
        }
        self.note_page_changed(page_id);

        let location = PageIndex { page_id, row_index };
        let metadata = self
            .entities
            .get_metadata_mut(entity_id)
            .expect("the caller allocated `entity_id`");
        B::update_metadata(metadata, location, &self.storage.registry);

        for domain in metadata.locations.keys() {
            self.storage
                .domain_bitsets
                .entry(*domain)
                .or_default()
                .set(entity_id.index);

            self.storage
                .domain_stats
                .entry(*domain)
                .or_default()
                .entity_count += 1;

            // `bump_domain_epoch` inlined: `metadata` still borrows `self.entities`.
            let epoch = &mut self.domain_epochs[domain.index()];
            *epoch = (*epoch).max(self.clock.tick.0);
        }
    }

    /// Returns the page column holding `entity`'s component `type_id`, and its row there.
    pub(crate) fn component_cell(
        &self,
        entity: EntityId,
        type_id: TypeId,
    ) -> Option<(&dyn crate::ecs::AnyVec, usize)> {
        let (id, metadata) = self.entities.get(entity.index as usize)?;
        if *id != entity {
            return None;
        }
        let domain = self.storage.registry.get_domain(type_id)?;
        let location = metadata.as_ref()?.locations.get(&domain)?;
        let column = self
            .storage
            .pages
            .get(location.page_id as usize)?
            .columns
            .get(&ComponentKey::Rust(type_id))?;
        Some((column.as_ref(), location.row_index as usize))
    }

    /// Places a row for `entity` in the page of a run-time `signature`, as a spawn does.
    ///
    /// `signature` holds sorted, deduped keys of registered components; `fill`
    /// pushes exactly one value into each column. Returns `false`, touching
    /// nothing, when `entity` is not alive, a key is not registered, or the
    /// entity already holds a component of one of the signature's domains.
    pub(crate) fn place_row(
        &mut self,
        entity: EntityId,
        signature: &[ComponentKey],
        fill: impl FnOnce(&mut crate::ecs::page::ComponentPage),
    ) -> bool {
        let mut domains = Vec::with_capacity(signature.len());
        for key in signature {
            let Some(domain) = self.storage.registry.domain_of(*key) else {
                return false;
            };
            if !domains.contains(&domain) {
                domains.push(domain);
            }
        }
        match self.entities.get(entity.index as usize) {
            Some((id, Some(metadata)))
                if *id == entity && domains.iter().all(|d| !metadata.locations.contains_key(d)) => {
            }
            _ => return false,
        }

        let page_id = self.find_or_create_page_for_signature(signature);
        let row_index = {
            let page = &mut self.storage.pages[page_id as usize];
            let row_index = page.entities.len() as u32;
            fill(page);
            let now = self.clock.tick;
            page.add_entity(entity, &self.clock.observed, |_| now);
            row_index
        };
        self.note_page_changed(page_id);

        let location = PageIndex { page_id, row_index };
        if let Some(metadata) = self.entities.get_metadata_mut(entity) {
            for domain in &domains {
                metadata.locations.insert(*domain, location);
            }
        }
        for domain in domains {
            self.storage
                .domain_bitsets
                .entry(domain)
                .or_default()
                .set(entity.index);
            self.storage
                .domain_stats
                .entry(domain)
                .or_default()
                .entity_count += 1;
            self.bump_domain_epoch(domain);
        }
        true
    }

    /// Returns whether row `row` of page `page_id` is `entity`'s live row, not one it left behind.
    pub(crate) fn is_live_row_of(&self, page_id: u32, row: usize, entity: EntityId) -> bool {
        match self.entities.get(entity.index as usize) {
            Some((id, Some(metadata))) if *id == entity => metadata
                .locations
                .values()
                .any(|loc| loc.page_id == page_id && loc.row_index as usize == row),
            _ => false,
        }
    }

    /// Returns whether `entity_id` names a live entity, generation included.
    ///
    /// An index is recycled, so a handle kept across frames may name another
    /// entity than the one it was made for; this tells the two apart.
    pub fn contains(&self, entity_id: EntityId) -> bool {
        match self.entities.get(entity_id.index as usize) {
            Some((id_in_world, metadata_slot)) => {
                id_in_world.generation == entity_id.generation && metadata_slot.is_some()
            }
            None => false,
        }
    }

    /// Despawns an entity, removing all its components and freeing its index for reuse.
    ///
    /// Returns `false`, touching nothing, if `entity_id` is not alive at its generation.
    pub fn despawn(&mut self, entity_id: EntityId) -> bool {
        let Some(metadata) = self.entities.despawn(entity_id) else {
            return false;
        };
        self.forget_identity(entity_id);

        // A mixed-domain row sits under several domain keys: removed once, or
        // the second pass would remove the moved survivor's row.
        let mut removed_rows: HashSet<PageIndex> = HashSet::new();
        for (domain, location) in metadata.locations {
            if removed_rows.insert(location) {
                self.remove_from_page(location);
            }

            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            if let Some(stats) = self.storage.domain_stats.get_mut(&domain) {
                stats.entity_count = stats.entity_count.saturating_sub(1);
            }

            self.bump_domain_epoch(domain);
        }
        true
    }

    /// Returns an iterator over all currently living `EntityId`s in the world.
    pub fn iter_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities
            .iter()
            .filter_map(|(id, metadata_opt)| metadata_opt.as_ref().map(|_| *id))
    }
}

impl Default for World {
    /// Creates a new, empty `World` via `World::new()`.
    fn default() -> Self {
        Self::new()
    }
}
