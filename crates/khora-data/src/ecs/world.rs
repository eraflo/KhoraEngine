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

use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
};

use bincode::config;
use khora_core::{
    asset::Material,
    ecs::entity::EntityId,
    renderer::api::scene::{GpuMaterial, GpuMesh, Mesh},
};

use crate::ecs::{
    components::HandleComponent,
    entity_store::EntityStore,
    page::{ComponentPage, PageIndex},
    planner::QueryPlanner,
    query::{Query, WorldQuery},
    registry::ComponentRegistry,
    serialization::SceneMemoryLayout,
    storage::StorageManager,
    Component, ComponentBundle, DomainBitset, LayoutPolicy, MaterialRef, MeshRef, QueryMut,
    QueryPlan, SemanticDomain, SerializedPage, TypeRegistry,
};

/// Errors that can occur when adding a component to an entity.
#[derive(Debug, PartialEq, Eq)]
pub enum AddComponentError {
    /// The specified entity does not exist or is not alive.
    EntityNotFound,
    /// The specified component type is not registered in the ECS.
    ComponentNotRegistered,
    /// The entity already has a component of the specified type.
    ComponentAlreadyExists,
}

/// Errors that can occur when removing a single component from an entity.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveComponentError {
    /// The specified entity does not exist or is not alive.
    EntityNotFound,
    /// The specified component type is not registered in the ECS.
    ComponentNotRegistered,
    /// The entity does not currently have a component of the specified type.
    ComponentNotPresent,
}

/// Errors that can occur while reconstructing a `World` from a raw archetype
/// memory snapshot in [`World::deserialize_archetype`].
#[derive(Debug)]
pub enum DeserializeArchetypeError {
    /// The outer bincode payload could not be decoded.
    Decode(bincode::error::DecodeError),
    /// A serialized component type name is not present in the type registry of
    /// this `World`, so its column cannot be reconstructed.
    UnknownComponent(String),
    /// A column's raw bytes failed validation (misaligned or oversized length).
    InvalidColumn(super::page::SetFromBytesError),
}

impl std::fmt::Display for DeserializeArchetypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeserializeArchetypeError::Decode(e) => write!(f, "archetype decode failed: {e}"),
            DeserializeArchetypeError::UnknownComponent(name) => {
                write!(f, "unknown serialized component type: {name}")
            }
            DeserializeArchetypeError::InvalidColumn(e) => {
                write!(f, "invalid component column: {e}")
            }
        }
    }
}

impl std::error::Error for DeserializeArchetypeError {}

impl From<bincode::error::DecodeError> for DeserializeArchetypeError {
    fn from(e: bincode::error::DecodeError) -> Self {
        DeserializeArchetypeError::Decode(e)
    }
}

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
    /// A mixed-domain bundle is stored in one page but registered under several
    /// domain keys in [`EntityMetadata::locations`], all addressing the same
    /// `(page_id, row_index)`. The moved (last-row) entity may likewise reference
    /// this page under more than one domain, so every one of its locations that
    /// pointed at the page's old last row is repointed at `location` — patching a
    /// single domain would leave the others dangling. O(domains) per page, no scan.
    fn remove_from_page(&mut self, entity_to_despawn: EntityId, location: PageIndex) {
        let page = &mut self.storage.pages[location.page_id as usize];
        if page.entities.is_empty() {
            return;
        }

        let old_last_row = (page.entities.len() - 1) as u32;
        let last_entity_in_page = page.entities[old_last_row as usize];
        page.swap_remove_row(location.row_index);

        // The last row moved into the vacated slot. Repoint every location of the
        // moved entity that addressed this page's old last row at the new slot.
        // (When the despawned entity *is* the last row, nothing moved.)
        if last_entity_in_page != entity_to_despawn {
            let metadata = self.entities.get_metadata_mut(last_entity_in_page).unwrap();
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
            instance_id: WORLD_INSTANCE_COUNTER
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
            self.domain_epochs[domain.index()] =
                self.domain_epochs[domain.index()].wrapping_add(1);
        }

        entity_id
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
                self.remove_from_page(entity_id, location);
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

    /// Creates an iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This is the primary method for reading and writing data in the ECS. The query `Q`
    /// is specified as a tuple via turbofish syntax. It can include component references
    /// (e.g., `&Position`, `&mut Velocity`) and filters (e.g., `Without<Parent>`).
    ///
    /// This method is very cheap to call. It performs an efficient search to identify all
    /// `ComponentPage`s that satisfy the query's criteria. The returned iterator then
    /// efficiently iterates over the data in only those pages.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Find all entities with a `Transform` and `GlobalTransform`.
    /// for (transform, global) in world.query::<(&Transform, &GlobalTransform)>() {
    ///     // ...
    /// }
    ///
    /// // Find all root entities (those with a `Transform` but without a `Parent`).
    /// for (transform,) in world.query::<(&Transform, Without<Parent>)>() {
    ///     // ...
    /// }
    /// ```
    pub fn query<'a, Q: WorldQuery>(&'a self) -> Query<'a, Q> {
        let type_ids = Q::type_ids();

        // 1. Try to fetch the strategy plan from the cache.
        // We cache the execution logic (Native vs Transversal), not the page indices.
        let plan = {
            let cache = self.planner.query_cache.read().unwrap_or_else(|e| e.into_inner());
            if let Some(plan) = cache.get(&type_ids) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self.planner.query_cache.write().unwrap_or_else(|e| e.into_inner());
                cache.insert(type_ids.clone(), new_plan.clone());
                new_plan
            }
        };

        // 2. Dynamically find matching pages for this call.
        // This ensures the query is correct even if new archetypes were created
        // in a different domain since the last call.
        let matching_page_indices =
            self.find_matching_pages(&plan.driver_signature, &Q::without_type_ids());

        // Record one access observation per queried component (coarse, off the
        // per-element path): count the query and the rows it scans. The DCC reads
        // these to drive adaptive memory layout — observation only, never control.
        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage.registry.record_access(&type_ids, rows_scanned);

        // 3. Return the query with the plan and the current matching pages.
        Query::new(self, plan, matching_page_indices)
    }

    /// Creates a mutable iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This method is similar to `query`, but it allows mutable access to the components.
    /// It uses the same dynamic plan re-finding to ensure thread-safe consistency.
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
        let type_ids = Q::type_ids();

        // 1. Get strategy from cache
        let plan = {
            let cache = self.planner.query_cache.read().unwrap_or_else(|e| e.into_inner());
            if let Some(plan) = cache.get(&type_ids) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self.planner.query_cache.write().unwrap_or_else(|e| e.into_inner());
                cache.insert(type_ids.clone(), new_plan.clone());
                new_plan
            }
        };

        // 2. Dynamically find pages
        let matching_page_indices =
            self.find_matching_pages(&plan.driver_signature, &Q::without_type_ids());

        // Record one access observation per queried component (see `query`).
        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage.registry.record_access(&type_ids, rows_scanned);

        // The caller may write through every `&mut` term of the query:
        // mark those domains changed ONCE here, at construction — never
        // per row, which would put the bump on the iteration hot path.
        for type_id in Q::mutable_type_ids() {
            if let Some(domain) = self.storage.registry.get_domain(type_id) {
                self.bump_domain_epoch(domain);
            }
        }

        // 3. Construct the iterator
        QueryMut::new(self, plan, matching_page_indices)
    }

    /// Registers a component type with a specific semantic domain.
    ///
    /// This is a crucial setup step. Before a component of type `T` can be used
    /// in a bundle, it must be registered with the world to define which semantic
    /// page group its data will be stored in.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        self.storage.registry.register::<T>(domain);
        self.type_registry.register::<T>();
    }

    /// Analyzes a query's component signature to create an optimized execution plan.
    ///
    /// This method identifies if a query is transversal (spanning multiple domains)
    /// and selects the most efficient "Driver Domain" based on entity density.
    pub(crate) fn analyze_query(&self, type_ids: &[TypeId]) -> QueryPlan {
        let mut domains = HashSet::new();
        for type_id in type_ids {
            if let Some(domain) = self.storage.registry.get_domain(*type_id) {
                domains.insert(domain);
            }
        }

        if domains.len() <= 1 {
            // NATIVE MODE: All components belong to the same semantic domain (or none).
            // This is the fastest execution path as it avoids any cross-domain joins.
            let first_domain = domains.into_iter().next();
            let plan = QueryPlan::new(false, first_domain, HashSet::new(), type_ids.to_vec());
            return plan;
        }

        // TRANSVERSAL MODE
        // Select the domain with the highest density of entities as the driver domain.
        // Highest density = most entities per page => page_A_count * entity_B_count < page_B_count * entity_A_count
        let driver_domain = domains
            .iter()
            .min_by(|&&a, &&b| {
                let stats_a = self
                    .storage
                    .domain_stats
                    .get(&a)
                    .copied()
                    .unwrap_or_default();
                let stats_b = self
                    .storage
                    .domain_stats
                    .get(&b)
                    .copied()
                    .unwrap_or_default();
                let score_a = (stats_a.page_count as u64) * (stats_b.entity_count as u64);
                let score_b = (stats_b.page_count as u64) * (stats_a.entity_count as u64);
                score_a.cmp(&score_b)
            })
            .copied()
            .unwrap(); // Should always be Some if domains.len() > 1

        let mut peer_domains = domains;
        peer_domains.remove(&driver_domain);

        // Calculate driver signature (subset of type_ids in the driver domain)
        let mut driver_signature = Vec::new();
        for type_id in type_ids {
            if self.storage.registry.get_domain(*type_id) == Some(driver_domain) {
                driver_signature.push(*type_id);
            }
        }
        driver_signature.sort();

        // Initialize the final plan.
        // In transversal mode, the driver signature is the subset of components
        // that belong to the driver domain.
        QueryPlan::new(true, Some(driver_domain), peer_domains, driver_signature)
    }

    /// Internal helper to find pages matching a signature and filter.
    fn find_matching_pages(&self, type_ids: &[TypeId], without_type_ids: &[TypeId]) -> Vec<u32> {
        let mut matching_page_indices = Vec::new();
        'page_loop: for (page_id, page) in self.storage.pages.iter().enumerate() {
            for required_type in type_ids {
                if page.type_ids.binary_search(required_type).is_err() {
                    continue 'page_loop;
                }
            }
            for excluded_type in without_type_ids {
                if page.type_ids.binary_search(excluded_type).is_ok() {
                    continue 'page_loop;
                }
            }
            matching_page_indices.push(page_id as u32);
        }
        matching_page_indices
    }

    /// (Internal) Computes a bitset that represents the intersection of all domains
    /// involved in a transversal query. This is used to speed up joins by skipping
    /// metadata lookups for entities that are guaranteed to not satisfy the query.
    pub(crate) fn compute_query_bitset(&self, plan: &QueryPlan) -> Option<DomainBitset> {
        if plan.mode == crate::ecs::QueryMode::Native {
            return None;
        }

        let driver_domain = plan.driver_domain?;

        // Start with the driver domain's bitset.
        let mut bitset = self.storage.domain_bitsets.get(&driver_domain)?.clone();

        // Intersect with all peer domains.
        for peer_domain in &plan.peer_domains {
            if let Some(peer_bitset) = self.storage.domain_bitsets.get(peer_domain) {
                bitset.intersect(peer_bitset);
            } else {
                // If a required peer domain has NO components at all, the intersection is empty.
                return Some(DomainBitset::new());
            }
        }

        Some(bitset)
    }

    /// This operation is designed to be fast. It performs the necessary data
    /// migration to move the entity's components for the given `SemanticDomain`
    /// to a new `ComponentPage` that matches the new layout.
    ///
    /// Crucially, it does NOT clean up the "hole" left in the old page. Instead,
    /// it returns the location of the orphaned data, delegating the cleanup task
    /// to an asynchronous garbage collection system.
    ///
    /// # Returns
    ///
    /// - `Ok(Option<PageIndex>)`: On success. The `Option` contains the location of
    ///   orphaned data if a migration occurred, which should be sent to a garbage collector.
    ///   It is `None` if no migration was needed (e.g., adding to a new domain).
    /// - `Err(AddComponentError)`: If the operation failed (e.g., entity not alive,
    ///   component not registered, or component already present).
    pub fn add_component<C: Component>(
        &mut self,
        entity_id: EntityId,
        component: C,
    ) -> Result<Option<PageIndex>, AddComponentError> {
        // 1. Validate EntityId and get metadata
        let Some((id_in_world, Some(_))) = self.entities.get(entity_id.index as usize) else {
            return Err(AddComponentError::EntityNotFound);
        };

        if id_in_world.generation != entity_id.generation {
            return Err(AddComponentError::EntityNotFound);
        }

        let Some(domain) = self.storage.registry.get_domain(TypeId::of::<C>()) else {
            return Err(AddComponentError::ComponentNotRegistered);
        };

        let mut metadata = self
            .entities
            .get_mut(entity_id.index as usize)
            .unwrap()
            .1
            .take()
            .unwrap();
        let old_location_opt = metadata.locations.get(&domain).copied();

        // 2. Determine old and new page signatures
        let old_type_ids = old_location_opt.map_or(Vec::new(), |loc| {
            self.storage.pages[loc.page_id as usize].type_ids.clone()
        });
        let mut new_type_ids = old_type_ids.clone();
        new_type_ids.push(TypeId::of::<C>());
        new_type_ids.sort();
        new_type_ids.dedup();

        if new_type_ids == old_type_ids {
            self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata); // Put it back
            return Err(AddComponentError::ComponentAlreadyExists);
        }

        // 3. Find or create the destination page
        let dest_page_id = self.find_or_create_page_for_signature(&new_type_ids);

        // 4. Perform the migration
        let dest_row_index;
        unsafe {
            // SAFETY: when an old location exists it lives on a different page
            // than `dest_page_id` (the equal-page case is unreachable — a
            // differing signature guarantees a different page), so `src_page`
            // and `dest_page` are borrowed disjointly through raw pointers into
            // `storage.pages`.
            let (src_page_opt, dest_page) = if let Some(loc) = old_location_opt {
                if loc.page_id == dest_page_id {
                    unreachable!(
                        "same-page migration must be caught by the earlier signature check"
                    );
                } else {
                    let all_pages_ptr = self.storage.pages.as_mut_ptr();
                    let dest_page = &mut *all_pages_ptr.add(dest_page_id as usize);
                    let src_page = &*all_pages_ptr.add(loc.page_id as usize);
                    (Some(src_page), dest_page)
                }
            } else {
                (None, &mut self.storage.pages[dest_page_id as usize])
            };

            dest_row_index = dest_page.entities.len() as u32;

            if let Some(src_page) = src_page_opt {
                let src_row = old_location_opt.unwrap().row_index as usize;
                for type_id in &old_type_ids {
                    let copier = self.storage.registry.get_row_copier(type_id).unwrap();
                    let src_col = src_page.columns.get(type_id).unwrap();
                    let dest_col = dest_page.columns.get_mut(type_id).unwrap();
                    copier(src_col.as_ref(), src_row, dest_col.as_mut());
                }
            }

            dest_page
                .columns
                .get_mut(&TypeId::of::<C>())
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Vec<C>>()
                .unwrap()
                .push(component);

            dest_page.add_entity(entity_id);
        }

        // 5. Update metadata and put it back
        metadata.locations.insert(
            domain,
            PageIndex {
                page_id: dest_page_id,
                row_index: dest_row_index,
            },
        );

        // Update the domain bitset for the entity.
        self.storage
            .domain_bitsets
            .entry(domain)
            .or_default()
            .set(entity_id.index);

        self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata);

        // The entity gained a component in this domain — invalidate cached Views.
        self.bump_domain_epoch(domain);

        // 6. Record the abandoned source page so maintenance compacts its
        //    now-orphaned row later (see `StorageManager::dirty_pages`). The
        //    `None` case adds the entity to a domain for the first time, leaving
        //    no orphan behind.
        if let Some(old) = old_location_opt {
            self.storage.dirty_pages.insert(old.page_id);
        }

        // 7. Return the old location for cleanup, without performing swap_remove
        Ok(old_location_opt)
    }

    /// Removes a **single** component `C` from `entity`, preserving every
    /// other component the entity carries (in any domain).
    ///
    /// Mirrors [`add_component`](Self::add_component) in reverse: rebuilds
    /// the entity's domain page signature without `C`, finds-or-creates a
    /// page matching the new signature, and copies the surviving
    /// same-domain components there. The old slot is orphaned and reported
    /// for the GC, exactly like an add migration.
    ///
    /// Use this — not [`remove_component_domain`](Self::remove_component_domain) —
    /// for surgical component removal (editor "delete component" button,
    /// `set_parent` unparenting, AGDF demotion). `remove_component_domain`
    /// is a low-level primitive that drops the entire domain bucket and
    /// should be reserved for entity teardown / GC paths.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(PageIndex))` — old location to send to the GC.
    /// - `Ok(None)` — should not happen in practice (kept for symmetry with
    ///   `add_component`).
    /// - `Err(RemoveComponentError::EntityNotFound)` — entity dead or stale.
    /// - `Err(RemoveComponentError::ComponentNotRegistered)` — type unknown.
    /// - `Err(RemoveComponentError::ComponentNotPresent)` — entity didn't
    ///   carry this component to begin with.
    pub fn remove_component<C: Component>(
        &mut self,
        entity_id: EntityId,
    ) -> Result<Option<PageIndex>, RemoveComponentError> {
        // 1. Validate the entity.
        let Some((id_in_world, Some(_))) = self.entities.get(entity_id.index as usize) else {
            return Err(RemoveComponentError::EntityNotFound);
        };
        if id_in_world.generation != entity_id.generation {
            return Err(RemoveComponentError::EntityNotFound);
        }

        // 2. Resolve the component's domain.
        let Some(domain) = self.storage.registry.get_domain(TypeId::of::<C>()) else {
            return Err(RemoveComponentError::ComponentNotRegistered);
        };

        // Take metadata out so we can mutate `self.storage` freely.
        let mut metadata = self
            .entities
            .get_mut(entity_id.index as usize)
            .unwrap()
            .1
            .take()
            .unwrap();

        let Some(loc) = metadata.locations.get(&domain).copied() else {
            // Entity isn't in this domain at all.
            self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata);
            return Err(RemoveComponentError::ComponentNotPresent);
        };

        let target_type = TypeId::of::<C>();
        let old_type_ids = self.storage.pages[loc.page_id as usize].type_ids.clone();
        if !old_type_ids.contains(&target_type) {
            // Entity is in this domain but doesn't have C specifically.
            self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata);
            return Err(RemoveComponentError::ComponentNotPresent);
        }

        // 3. Build the new domain signature (sans C).
        let new_type_ids: Vec<TypeId> = old_type_ids
            .iter()
            .copied()
            .filter(|t| *t != target_type)
            .collect();

        // 4. If C was the last component in this domain, just drop the
        //    domain location and bitset bit. No page migration needed.
        if new_type_ids.is_empty() {
            metadata.locations.remove(&domain);
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata);
            // The entity left this domain entirely — invalidate cached Views.
            self.bump_domain_epoch(domain);
            // The row is orphaned (metadata no longer references it) — schedule
            // its page for compaction.
            self.storage.dirty_pages.insert(loc.page_id);
            return Ok(Some(loc));
        }

        // 5. Find or create the destination page for the reduced signature.
        let dest_page_id = self.find_or_create_page_for_signature(&new_type_ids);

        // 6. Migrate every surviving same-domain component from src → dest.
        let dest_row_index;
        unsafe {
            // SAFETY: src and dest are different pages (signatures differ),
            // so we can borrow them disjointly through raw pointers.
            let all_pages_ptr = self.storage.pages.as_mut_ptr();
            let dest_page = &mut *all_pages_ptr.add(dest_page_id as usize);
            let src_page = &*all_pages_ptr.add(loc.page_id as usize);
            assert_ne!(
                loc.page_id, dest_page_id,
                "remove_component: src/dest aliased"
            );

            dest_row_index = dest_page.entities.len() as u32;
            let src_row = loc.row_index as usize;

            for type_id in &new_type_ids {
                let copier = self.storage.registry.get_row_copier(type_id).unwrap();
                let src_col = src_page.columns.get(type_id).unwrap();
                let dest_col = dest_page.columns.get_mut(type_id).unwrap();
                copier(src_col.as_ref(), src_row, dest_col.as_mut());
            }

            dest_page.add_entity(entity_id);
        }

        // 7. Update entity metadata to point at the new (page, row).
        metadata.locations.insert(
            domain,
            PageIndex {
                page_id: dest_page_id,
                row_index: dest_row_index,
            },
        );
        // The bitset stays set — other components remain in this domain.
        self.entities.get_mut(entity_id.index as usize).unwrap().1 = Some(metadata);

        // The entity lost a component in this domain — invalidate cached Views.
        self.bump_domain_epoch(domain);

        // 8. Record the abandoned source page so maintenance compacts its
        //    now-orphaned row later.
        self.storage.dirty_pages.insert(loc.page_id);

        // 9. Hand the old location off to the GC.
        Ok(Some(loc))
    }

    /// Logically removes all components belonging to a specific `SemanticDomain` from an entity.
    ///
    /// This is an extremely fast, O(1) operation that only modifies the entity's
    /// metadata. It does not immediately deallocate or move any component data.
    /// The component data is "orphaned" and will be cleaned up later by a
    /// garbage collection process.
    ///
    /// This method is generic over a component `C` to determine which domain to remove.
    ///
    /// # Returns
    ///
    /// - `Some(PageIndex)`: Contains the location of the orphaned data if the
    ///   components were successfully removed. This can be sent to a garbage collector.
    /// - `None`: If the entity is not alive or did not have any components in the
    ///   specified `SemanticDomain`.
    pub fn remove_component_domain<C: Component>(
        &mut self,
        entity_id: EntityId,
    ) -> Option<PageIndex> {
        // 1. Validate the entity ID to ensure we're acting on a live entity.
        let (id_in_world, metadata_slot) = self.entities.get_mut(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_slot.is_none() {
            return None;
        }

        // 2. Use the registry to find the component's domain.
        let domain = self.storage.registry.get_domain(TypeId::of::<C>())?;

        // 3. Remove the location entry from the entity's metadata.
        //    `HashMap::remove` returns the value that was at that key, which is exactly what we need.
        let metadata = metadata_slot.as_mut().unwrap();
        let location = metadata.locations.remove(&domain);

        // Clear the domain bitset if a component was removed.
        if let Some(loc) = location {
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            // The entity left this domain — invalidate cached Views.
            self.bump_domain_epoch(domain);
            // The row is orphaned (metadata no longer references it) — schedule
            // its page for compaction.
            self.storage.dirty_pages.insert(loc.page_id);
        }

        location
    }

    /// Gets a mutable reference to a single component `T` for a given entity.
    ///
    /// This provides direct, "random" access to a component, which can be less
    /// performant than querying but is useful for targeted modifications.
    ///
    /// # Returns
    ///
    /// `None` if the entity is not alive or does not have the requested component.
    pub fn get_mut<T: Component>(&mut self, entity_id: EntityId) -> Option<&mut T> {
        // 1. Validate the entity ID. An out-of-range index means "not alive"
        // (e.g. a stale or malformed EntityId), so return None rather than panic.
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        // 2. Use the registry to find the component's domain and its location.
        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = metadata.locations.get(&domain)?;

        // Handing out `&mut T` means T may change — invalidate cached
        // Views. Inline field access: `metadata` still borrows
        // `self.entities`, so `bump_domain_epoch(&mut self)` can't be
        // called here.
        self.domain_epochs[domain.index()] = self.domain_epochs[domain.index()].wrapping_add(1);

        // 3. Get the component data from the page.
        let type_id = TypeId::of::<T>();
        let page = self.storage.pages.get_mut(location.page_id as usize)?;
        let column = page.columns.get_mut(&type_id)?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;

        vec.get_mut(location.row_index as usize)
    }

    /// Gets mutable references to components of type `T` for multiple entities simultaneously.
    ///
    /// This is safer than `get_mut` in a loop because it allows retrieving multiple
    /// disjoint mutable references to components of the same type.
    ///
    /// # Returns
    ///
    /// An array of `Option<&mut T>`. If any entity is not found, does not have the component,
    /// or if there are duplicate requests for the same component instance, that entry will be `None`.
    pub fn get_many_mut<T: Component, const N: usize>(
        &mut self,
        ids: [EntityId; N],
    ) -> [Option<&mut T>; N] {
        let mut results: [Option<&mut T>; N] = std::array::from_fn(|_| None);

        let type_id = TypeId::of::<T>();
        let domain = match self.storage.registry.get_domain(type_id) {
            Some(d) => d,
            None => return results,
        };

        // Handing out `&mut T` references means T may change — invalidate
        // cached Views (conservative: bumped even if no entity resolves).
        self.bump_domain_epoch(domain);

        // 1. Collect locations and check for duplicates
        let mut locations = [(0u32, 0u32); N];
        let mut found_mask = [false; N];

        for i in 0..N {
            if let Some((stored_id, Some(metadata))) = self.entities.get(ids[i].index as usize) {
                // Ensure the EntityId matches (including generation)
                if *stored_id == ids[i] {
                    if let Some(loc) = metadata.locations.get(&domain) {
                        locations[i] = (loc.page_id, loc.row_index);
                        found_mask[i] = true;
                    }
                }
            }
        }

        // 2. Check for collisions in (page, row) to prevent aliasing
        for i in 0..N {
            if !found_mask[i] {
                continue;
            }
            for j in (i + 1)..N {
                if found_mask[j] && locations[i] == locations[j] {
                    // Collision detected: return all None for safety
                    return std::array::from_fn(|_| None);
                }
            }
        }

        // 3. Retrieve references using unsafe to bypass split_at_mut complexity.
        // SAFETY: We have verified that all (page, row) pairs are unique,
        // so we are not creating multiple mutable references to the same data.
        for i in 0..N {
            if found_mask[i] {
                let (page_id, row_index) = locations[i];
                unsafe {
                    // We can't borrow self.pages multiple times mutably in the loop,
                    // but we know the indices are disjoint or the data is disjoint.
                    let world_ptr = self as *mut Self;
                    if let Some(page) = (&mut *world_ptr).storage.pages.get_mut(page_id as usize) {
                        if let Some(column) = page.columns.get_mut(&type_id) {
                            if let Some(vec) = column.as_any_mut().downcast_mut::<Vec<T>>() {
                                results[i] = Some(vec.get_unchecked_mut(row_index as usize));
                            }
                        }
                    }
                }
            }
        }

        results
    }

    /// Gets an immutable reference to a single component `T` for a given entity.
    ///
    /// This provides direct, "random" access to a component.
    ///
    /// # Returns
    ///
    /// `None` if the entity is not alive or does not have the requested component.
    pub fn get<T: Component>(&self, entity_id: EntityId) -> Option<&T> {
        // 1. Validate the entity ID. An out-of-range index means "not alive"
        // (e.g. a stale or malformed EntityId), so return None rather than panic.
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        // 2. Use the registry to find the component's domain and its location.
        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = metadata.locations.get(&domain)?;

        // 3. Get the component data from the page.
        let type_id = TypeId::of::<T>();
        let page = self.storage.pages.get(location.page_id as usize)?;

        // 4. Return the immutable reference.
        let vec = page
            .columns
            .get(&type_id)?
            .as_any()
            .downcast_ref::<Vec<T>>()?;
        vec.get(location.row_index as usize)
    }

    /// Reads a component by **value**, working for *any* physical layout (AoS or
    /// field-SoA). This is the layout-agnostic read path: a field-SoA component
    /// can't hand out `&T` (its bytes aren't a contiguous `T`), so callers that
    /// must work regardless of layout — the serialization recipe, the `Soa<T>`
    /// query — go through here. For AoS it simply clones the `&T`.
    ///
    /// `None` if the entity is not alive or lacks the component.
    pub fn clone_component<T: Component>(&self, entity_id: EntityId) -> Option<T> {
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation {
            return None;
        }
        let metadata = metadata_opt.as_ref()?;
        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = metadata.locations.get(&domain)?;
        let page = self.storage.pages.get(location.page_id as usize)?;
        let column = page.columns.get(&TypeId::of::<T>())?;
        Some(T::clone_from_column(
            column.as_ref(),
            location.row_index as usize,
        ))
    }

    /// Writes a component by **value**, working for any physical layout. The
    /// layout-agnostic write path (AoS assigns the slot; field-SoA scatters into
    /// its lanes). Returns `false` if the entity is not alive or lacks the
    /// component (nothing is written).
    pub fn set_component<T: Component>(&mut self, entity_id: EntityId, value: T) -> bool {
        let Some((id_in_world, metadata_opt)) = self.entities.get(entity_id.index as usize) else {
            return false;
        };
        if id_in_world.generation != entity_id.generation {
            return false;
        }
        let Some(metadata) = metadata_opt.as_ref() else {
            return false;
        };
        let Some(domain) = self.storage.registry.get_domain(TypeId::of::<T>()) else {
            return false;
        };
        let Some(location) = metadata.locations.get(&domain).copied() else {
            return false;
        };
        let Some(page) = self.storage.pages.get_mut(location.page_id as usize) else {
            return false;
        };
        let Some(column) = page.columns.get_mut(&TypeId::of::<T>()) else {
            return false;
        };
        value.set_in_column(column.as_mut(), location.row_index as usize);
        // The component's value changed — invalidate cached Views.
        self.bump_domain_epoch(domain);
        true
    }

    /// Runs `f` over every field-SoA column of component `T` in the world — the
    /// bulk SIMD entry point. Each call hands the kernel a [`FieldSoaColumn`]
    /// whose per-field `f32` lanes are contiguous, so it can tile them into
    /// `f32x8` without gather (the resident layout that reaches ~4×).
    ///
    /// Iterates per page so each lane slice is a single archetype's run. A
    /// no-op for any page whose `T` column is not field-SoA.
    pub fn for_each_soa_column_mut<T: crate::ecs::SoaLayout>(
        &mut self,
        mut f: impl FnMut(&mut crate::ecs::FieldSoaColumn<T>),
    ) {
        let type_id = TypeId::of::<T>();
        // Bulk mutable access to T's lanes — invalidate cached Views once,
        // up front (never inside the per-page/per-element loop).
        if let Some(domain) = self.storage.registry.get_domain(type_id) {
            self.bump_domain_epoch(domain);
        }
        for page in self.storage.pages.iter_mut() {
            if let Some(column) = page.columns.get_mut(&type_id) {
                if let Some(soa) = column
                    .as_any_mut()
                    .downcast_mut::<crate::ecs::FieldSoaColumn<T>>()
                {
                    f(soa);
                }
            }
        }
    }

    /// Returns an iterator over all currently living `EntityId`s in the world.
    pub fn iter_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities
            .iter()
            .filter_map(|(id, metadata_opt)| metadata_opt.as_ref().map(|_| *id))
    }

    /// Serializes the entire World state using a direct memory layout strategy.
    ///
    /// This method is highly unsafe as it reads raw component memory.
    pub fn serialize_archetype(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        let mut serialized_pages = Vec::with_capacity(self.storage.pages.len());
        for page in &self.storage.pages {
            let mut serialized_columns = HashMap::new();

            // Use the TypeRegistry to get the stable string name for each TypeId.
            let type_names: Vec<String> = page
                .type_ids
                .iter()
                .map(|id| self.type_registry.get_name_of(id).unwrap().to_string())
                .collect();

            for type_id in &page.type_ids {
                let type_name = self.type_registry.get_name_of(type_id).unwrap();
                let column = &page.columns[type_id];
                // The column owns its byte format (AoS raw bytes, or field-major
                // for a field-SoA column) — round-tripped by `set_from_bytes`.
                serialized_columns.insert(type_name.to_string(), column.to_bytes());
            }

            serialized_pages.push(SerializedPage {
                type_names,
                entities: page.entities.clone(),
                columns: serialized_columns,
            });
        }
        let layout = SceneMemoryLayout {
            entities: self.entities.entities.clone(),
            freed_entities: self.entities.freed_entities.clone(),
            pages: serialized_pages,
        };
        bincode::encode_to_vec(layout, config::standard())
    }

    /// Deserializes and completely replaces the World state from a memory layout.
    ///
    /// This method is highly unsafe as it writes raw bytes into component vectors.
    pub fn deserialize_archetype(
        &mut self,
        data: &[u8],
    ) -> Result<(), DeserializeArchetypeError> {
        let (layout, _): (SceneMemoryLayout, _) =
            bincode::decode_from_slice(data, config::standard())?;

        self.entities.entities = layout.entities;
        self.entities.freed_entities = layout.freed_entities;
        self.storage.pages.clear();

        for serialized_page in layout.pages {
            // Use the TypeRegistry to convert string names back to TypeIds. A
            // name absent from the registry comes from an untrusted/foreign
            // scene, so fail gracefully instead of panicking.
            let type_ids: Vec<TypeId> = serialized_page
                .type_names
                .iter()
                .map(|name| {
                    self.type_registry
                        .get_id_of(name)
                        .ok_or_else(|| DeserializeArchetypeError::UnknownComponent(name.clone()))
                })
                .collect::<Result<_, _>>()?;

            let mut new_page = ComponentPage {
                type_ids,
                entities: serialized_page.entities,
                columns: HashMap::new(),
            };

            for (type_name, bytes) in &serialized_page.columns {
                let type_id = self
                    .type_registry
                    .get_id_of(type_name)
                    .ok_or_else(|| DeserializeArchetypeError::UnknownComponent(type_name.clone()))?;
                let constructor = self
                    .storage
                    .registry
                    .get_column_constructor(&type_id)
                    .ok_or_else(|| {
                        DeserializeArchetypeError::UnknownComponent(type_name.clone())
                    })?;
                let mut column = constructor();
                // SAFETY: `constructor` is the registered column factory for
                // `type_id`, so it produces a column whose element type matches
                // the bytes serialized for that same type. `set_from_bytes`
                // additionally validates the byte length before any allocation,
                // returning an error (propagated here) on a hostile or
                // mismatched length rather than aborting.
                unsafe {
                    column
                        .set_from_bytes(bytes)
                        .map_err(DeserializeArchetypeError::InvalidColumn)?;
                }
                new_page.columns.insert(type_id, column);
            }
            self.storage.pages.push(new_page);
        }

        // The entire World content was replaced by raw storage writes —
        // every domain may have changed, so invalidate all cached Views.
        self.bump_all_domain_epochs();

        Ok(())
    }
}

impl World {
    /// Returns `true` if any of `entity`'s live metadata locations references
    /// `(page_id, row)`. Domain-**agnostic** on purpose: in a multi-domain page a
    /// physical row is dead only when *no* domain still points at it, so a
    /// per-domain check (like the query layer's `is_live_row`) would wrongly
    /// classify a row still live for another domain as an orphan and destroy it.
    ///
    /// A dead/recycled entity (generation mismatch or vacated metadata) counts as
    /// not referencing the row, so its leftover row is reclaimable.
    fn entity_references_row(&self, entity: EntityId, page_id: u32, row: usize) -> bool {
        let Some((slot_id, metadata_opt)) = self.entities.get(entity.index as usize) else {
            return false;
        };
        if slot_id.generation != entity.generation {
            return false;
        }
        let Some(metadata) = metadata_opt.as_ref() else {
            return false;
        };
        metadata
            .locations
            .values()
            .any(|loc| loc.page_id == page_id && loc.row_index as usize == row)
    }

    /// Compacts a single page: physically drops every row no live entity
    /// references (a migration orphan), preserving order for the surviving rows.
    ///
    /// Reuses [`remove_from_page`](Self::remove_from_page) as the removal
    /// primitive, so the survivor moved into each hole has its metadata repaired
    /// across **all** its domains — the validation the former `cleanup_orphan_at`
    /// lacked. Representation-only, but reordering rows changes query iteration
    /// order, so the page's domain epochs are bumped when at least one row is
    /// removed (order-sensitive Views — index-aligned light/audio lists — depend
    /// on the bump). No bump when nothing was removed.
    pub(crate) fn compact_page(&mut self, page_id: u32) {
        match self.storage.pages.get(page_id as usize) {
            Some(page) if !page.entities.is_empty() => {}
            _ => return,
        }

        let mut removed_any = false;
        let mut row = 0usize;
        loop {
            let len = self.storage.pages[page_id as usize].entities.len();
            if row >= len {
                break;
            }
            let entity = self.storage.pages[page_id as usize].entities[row];
            if self.entity_references_row(entity, page_id, row) {
                // Live for some domain — keep it and advance.
                row += 1;
            } else {
                // Orphan: `remove_from_page` swap-removes it and repoints the
                // survivor moved into the slot (across all its domains). The
                // swapped-in row now sits at `row`, so re-check the same index.
                self.remove_from_page(
                    entity,
                    PageIndex {
                        page_id,
                        row_index: row as u32,
                    },
                );
                removed_any = true;
            }
        }

        if removed_any {
            // Iteration order for every domain this page participates in changed.
            let mut domains: Vec<SemanticDomain> = {
                let page = &self.storage.pages[page_id as usize];
                page.type_ids
                    .iter()
                    .filter_map(|t| self.storage.registry.get_domain(*t))
                    .collect()
            };
            domains.sort_by_key(|d| d.index());
            domains.dedup();
            for domain in domains {
                self.bump_domain_epoch(domain);
            }
        }
    }

    /// Drains up to `budget` dirty pages and compacts each, returning the number
    /// of pages processed. Called once per frame by
    /// [`EcsMaintenance`](crate::ecs::EcsMaintenance) in `TickPhase::Maintenance`.
    /// Pages beyond the budget stay queued for the next frame — harmless, since
    /// the query layer already skips orphan rows via `is_live_row`.
    pub(crate) fn run_compaction(&mut self, budget: usize) -> usize {
        if budget == 0 || self.storage.dirty_pages.is_empty() {
            return 0;
        }
        let take: Vec<u32> = self
            .storage
            .dirty_pages
            .iter()
            .copied()
            .take(budget)
            .collect();
        for &page_id in &take {
            self.storage.dirty_pages.remove(&page_id);
        }
        for &page_id in &take {
            self.compact_page(page_id);
        }
        take.len()
    }
}

impl Default for World {
    /// Creates a new, empty `World` via `World::new()`.
    fn default() -> Self {
        Self::new()
    }
}
