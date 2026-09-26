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

use khora_core::{
    asset::Material,
    ecs::entity::EntityId,
    renderer::api::scene::{GpuMaterial, GpuMesh, Mesh},
};

use crate::ecs::{
    components::HandleComponent, entity_store::EntityStore, page::PageIndex, planner::QueryPlanner,
    registry::ComponentRegistry, storage::StorageManager, ComponentBundle, LayoutPolicy,
    MaterialRef, MeshRef, SemanticDomain, TypeRegistry,
};

mod archetype_io;
mod compaction;
mod component_access;
mod hierarchy;
mod queries;

pub use archetype_io::DeserializeArchetypeError;
pub use component_access::AddComponentError;
pub use component_access::RemoveComponentError;

/// Simple statistics for a semantic domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct DomainStats {
    /// Total number of entities in this domain.
    pub entity_count: u32,
    /// Total number of pages allocated for this domain.
    pub page_count: u32,
}

/// The central container for the entire ECS, holding all entities, components, and metadata.
pub struct World {
    /// Manages entity IDs and metadata.
    pub(crate) entities: EntityStore,
    /// Manages component storage and pages.
    pub(crate) storage: StorageManager,
    /// Manages query planning and caching.
    pub(crate) planner: QueryPlanner,
    /// The type registry for serialization purposes.
    type_registry: TypeRegistry,
    /// Monotonic per-domain change counters ("epochs"), indexed by
    /// [`SemanticDomain::index`]. Every entry point that can change a
    /// domain's *semantic* content (spawn/despawn, component add/remove,
    /// mutable access) bumps the matching epoch in O(1). Flows compare
    /// epochs across frames to decide whether a cached View is still
    /// valid, so over-bumping is harmless while a missed bump would mean
    /// stale Views. Representation-only changes (AGDF layout) do NOT bump.
    domain_epochs: [u64; SemanticDomain::COUNT],
    /// Process-unique id of this `World` instance. Folded into Flow cache
    /// keys so a freshly created World (whose epochs restart at zero, e.g.
    /// a play-mode snapshot restore) can never alias a previous World's
    /// epoch values and serve a stale cached View.
    instance_id: u64,
}

/// Source of process-unique [`World::instance_id`] values.
static WORLD_INSTANCE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl World {
    /// (Internal) Allocates a new or recycled `EntityId` and reserves its metadata slot.
    fn create_entity(&mut self) -> EntityId {
        self.entities.create_entity()
    }

    /// (Internal) Finds a page suitable for the given `ComponentBundle`, or creates one if none exists.
    fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        self.storage.find_or_create_page_for_bundle::<B>()
    }

    /// (Internal) Removes a single physical row from a page via `swap_remove`,
    /// patching the moved entity's metadata so every domain that referenced the
    /// vacated slot now points at it.
    ///
    /// Takes a row, not an entity, and deliberately: **whose** row it was never
    /// mattered, and taking the entity invited comparing identities where rows
    /// were meant — which is exactly the bug this signature closes.
    ///
    /// A mixed-domain bundle is stored in one page but registered under several
    /// domain keys in [`EntityMetadata::locations`](crate::ecs::EntityMetadata::locations), all addressing the same
    /// `(page_id, row_index)`. The moved (last-row) entity may likewise reference
    /// this page under more than one domain, so every one of its locations that
    /// pointed at the page's old last row is repointed at `location` — patching a
    /// single domain would leave the others dangling. O(domains) per page, no scan.
    ///
    /// The moved row may be a migration orphan whose owner is dead (despawned, or
    /// its index recycled under a newer generation): there is no live metadata to
    /// patch, so it is left for compaction.
    fn remove_from_page(&mut self, location: PageIndex) {
        let page = &mut self.storage.pages[location.page_id as usize];
        if page.entities.is_empty() {
            return;
        }

        let old_last_row = (page.entities.len() - 1) as u32;
        let moved_entity = page.entities[old_last_row as usize];
        page.swap_remove_row(location.row_index);

        // Removing the last row moves nothing.
        if location.row_index == old_last_row {
            return;
        }

        // The last row moved into the vacated slot. Repoint every location of the
        // moved entity that addressed this page's old last row at the new slot.
        //
        // The test is **which row was removed**, not whose entity it was. Those
        // are not the same question, and reading one for the other was a real
        // bug: an entity that leaves a page and comes back — `add_component`
        // then `remove_component`, which is what a `Teleported` marker does
        // every frame — leaves an orphan row *and* holds a live row in the same
        // page, both under its own id. When compaction reclaimed the orphan and
        // the live row happened to be last, an id comparison concluded "nothing
        // moved" and skipped the patch. The entity kept a row index into a page
        // that had just got shorter, and the next migration read past the end of
        // a column.
        //
        // `get_metadata_mut` checks the generation, so a dead or recycled owner
        // yields `None` and its orphan row is left for compaction.
        if let Some(metadata) = self.entities.get_metadata_mut(moved_entity) {
            for loc in metadata.locations.values_mut() {
                if loc.page_id == location.page_id && loc.row_index == old_last_row {
                    *loc = location;
                }
            }
        }
    }

    /// Finds or creates a page for the given signature of component `TypeId`s.
    fn find_or_create_page_for_signature(&mut self, signature: &[TypeId]) -> u32 {
        self.storage.find_or_create_page_for_signature(signature)
    }

    /// Creates a new, empty `World` with pre-registered internal component types.
    pub fn new() -> Self {
        let mut world = Self {
            entities: EntityStore::new(),
            storage: StorageManager::new(ComponentRegistry::default()),
            planner: QueryPlanner::new(),
            type_registry: TypeRegistry::default(),
            domain_epochs: [0; SemanticDomain::COUNT],
            instance_id: WORLD_INSTANCE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        };
        // Generic and hand-implemented components can't self-register via the
        // derive (generics have no single `TypeId`; `MaterialRef` holds a trait
        // object and has a manual `Component` impl), so they stay explicit.
        // CollisionPairs is **not** an ECS component — it lives in `Resources`
        // as `Arc<Mutex<CollisionPairs>>`.
        world.register_component::<HandleComponent<Mesh>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<GpuMesh>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<GpuMaterial>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<Box<dyn Material>>>(SemanticDomain::Render);
        world.register_component::<MaterialRef>(SemanticDomain::Render);
        world.register_component::<MeshRef>(SemanticDomain::Render);

        // Auto-register every component that declares its domain via
        // `#[derive(Component)]` + `#[component(domain = ...)]`. Idempotent with the
        // explicit calls above (same TypeId → same vtable) during migration.
        for reg in inventory::iter::<crate::ecs::ComponentDomainRegistration> {
            (reg.register)(&mut world);
        }

        world
    }

    /// Returns the [`SemanticDomain`] a component type was registered with,
    /// or `None` if it isn't registered (yet) on this world. Used by the
    /// editor to categorise components in the "Add Component" menu and the
    /// inspector without hard-coding a per-type table.
    pub fn component_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.storage.registry.get_domain(type_id)
    }

    /// The [`LayoutPolicy`] a component type is currently stored with. Defaults
    /// to `Soa`; the layout-adaptation pass may change it.
    pub fn component_layout(&self, type_id: TypeId) -> Option<LayoutPolicy> {
        self.storage.registry.layout_of(type_id)
    }

    /// Online access stats `(query_count, rows_scanned)` for a component type —
    /// the DCC / telemetry read these to drive memory-layout adaptation. The DCC
    /// only *observes*; it never mutates the layout (Data self-optimizes).
    pub fn component_access_stats(&self, type_id: TypeId) -> Option<(u64, u64)> {
        self.storage.registry.access_stats(type_id)
    }

    /// The number of live entities — the coarse workload size `n` the DCC's
    /// cost model fits agent execution time against.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// A snapshot of every registered component's access pattern as
    /// `(type_name, size_bytes, query_count, rows_scanned)`. The hot path
    /// samples this at a low rate and publishes it through the observation
    /// tunnel; the DCC turns it into a read-only layout recommendation.
    pub fn component_access_snapshot(&self) -> Vec<(String, usize, u64, u64)> {
        self.storage
            .registry
            .access_snapshot()
            .into_iter()
            .map(|(tid, size, qc, rows)| {
                let name = self
                    .type_registry
                    .get_name_of(&tid)
                    .unwrap_or("<unknown>")
                    .to_string();
                (name, size, qc, rows)
            })
            .collect()
    }

    /// Current change epoch of `domain` — a monotonic counter bumped by
    /// every mutation entry point that can affect the domain's semantic
    /// content. Equal epochs across two reads guarantee the domain's data
    /// (and its query iteration order) is unchanged; a different value
    /// only means "possibly changed" (bumps are conservative). Flows use
    /// this to validate cached Views.
    pub fn domain_epoch(&self, domain: SemanticDomain) -> u64 {
        self.domain_epochs[domain.index()]
    }

    /// Process-unique identifier of this `World` instance. Cache keys
    /// derived from [`domain_epoch`](Self::domain_epoch) must include it so
    /// that epochs from two different World instances never compare equal.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// (Internal) Marks `domain` as semantically changed. O(1).
    pub(crate) fn bump_domain_epoch(&mut self, domain: SemanticDomain) {
        self.domain_epochs[domain.index()] = self.domain_epochs[domain.index()].wrapping_add(1);
    }

    /// (Internal) Marks every domain as semantically changed — used by bulk
    /// paths (deserialization, compaction) where per-domain attribution is
    /// not worth the bookkeeping. Over-invalidation is always safe.
    pub(crate) fn bump_all_domain_epochs(&mut self) {
        for epoch in &mut self.domain_epochs {
            *epoch = epoch.wrapping_add(1);
        }
    }

    /// Spawns a new entity with the given bundle of components.
    ///
    /// This is the primary method for creating entities. It orchestrates the entire process:
    /// 1. Allocates a new `EntityId`.
    /// 2. Finds or creates a suitable `ComponentPage` for the component bundle.
    /// 3. Pushes the component data into the page's columns.
    /// 4. Updates the entity's metadata to point to the new data's location.
    /// 5. Updates domain bitsets and stats for the newly created entity components.
    ///
    /// Returns the `EntityId` of the newly created entity.
    pub fn spawn<B: ComponentBundle>(&mut self, bundle: B) -> EntityId {
        // Step 1: Allocate a new EntityId.
        let entity_id = self.create_entity();

        // Step 2: Find or create a page for this bundle.
        let page_id = self.find_or_create_page_for_bundle::<B>();

        // --- Step 3: Push component data into the page. ---
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
            page.add_entity(entity_id);
        }

        // --- Step 4: Update the entity's metadata. ---
        let location = PageIndex { page_id, row_index };
        let metadata = self.entities.get_metadata_mut(entity_id).unwrap();
        B::update_metadata(metadata, location, &self.storage.registry);

        // --- Step 5: Update domain bitsets and stats for the newly created entity components. ---
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

            // A new entity appeared in this domain — invalidate cached
            // Views. Inline field access: `metadata` still borrows
            // `self.entities`, so `bump_domain_epoch(&mut self)` can't be
            // called here.
            self.domain_epochs[domain.index()] = self.domain_epochs[domain.index()].wrapping_add(1);
        }

        entity_id
    }

    /// Whether `entity_id` names a live entity.
    ///
    /// The generation half of the check is the point. An index alone is
    /// recycled, so a handle kept across frames — a script's, an editor
    /// selection's, an undo entry's — can end up naming a *different* entity
    /// than the one it was made for. Asking is how a caller avoids writing to a
    /// stranger; the alternative is finding out afterwards.
    pub fn contains(&self, entity_id: EntityId) -> bool {
        match self.entities.get(entity_id.index as usize) {
            Some((id_in_world, metadata_slot)) => {
                id_in_world.generation == entity_id.generation && metadata_slot.is_some()
            }
            None => false,
        }
    }

    /// Despawns an entity, removing all its components and freeing its ID for recycling.
    ///
    /// This method performs the following steps:
    /// 1. Verifies that the `EntityId` is valid by checking its index and generation.
    /// 2. Removes the entity's component data from all pages where it is stored.
    /// 3. Marks the entity's metadata slot as vacant and adds its index to the free list.
    ///
    /// Returns `true` if the entity was valid and despawned, `false` otherwise.
    pub fn despawn(&mut self, entity_id: EntityId) -> bool {
        // Step 1: Validate the EntityId.
        // First, check if the index is even valid for our entities Vec.
        if entity_id.index as usize >= self.entities.len() {
            return false;
        }

        // Get the data at the slot.
        let (id_in_world, metadata_slot) = self.entities.get(entity_id.index as usize).unwrap();

        // An ID is valid if its generation matches the one in the world,
        // AND if the metadata slot is currently occupied (`is_some`).
        if id_in_world.generation != entity_id.generation || metadata_slot.is_none() {
            return false;
        }

        // --- At this point, the ID is valid. ---

        // Step 2: Take the metadata out of the slot, leaving it `None`.
        // This is what officially "kills" the entity.
        let metadata = self
            .entities
            .get_mut(entity_id.index as usize)
            .unwrap()
            .1
            .take()
            .unwrap();
        self.entities.freed_entities.push(entity_id.index);

        // --- Step 3: Remove the entity's data and update per-domain bookkeeping. ---
        // A mixed-domain bundle lives in one page but is registered under several
        // domain keys that all address the same `(page_id, row_index)`. The
        // physical row must be `swap_remove`d exactly once — calling it per domain
        // key would remove the moved survivor's data on the second pass — while the
        // bitset/stats/epoch bookkeeping still runs for every domain the entity
        // belonged to.
        let mut removed_rows: HashSet<PageIndex> = HashSet::new();
        for (domain, location) in metadata.locations {
            if removed_rows.insert(location) {
                self.remove_from_page(location);
            }

            // Clear the entity's bit in the domain bitset and update stats.
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            if let Some(stats) = self.storage.domain_stats.get_mut(&domain) {
                stats.entity_count = stats.entity_count.saturating_sub(1);
            }

            // An entity left this domain (and `remove_from_page` may have
            // reordered the page) — invalidate cached Views.
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
