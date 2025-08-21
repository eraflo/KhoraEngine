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

use std::any::TypeId;

use crate::ecs::{
    entity::EntityMetadata,
    page::{ComponentPage, PageIndex},
    query::{Query, WorldQuery},
    registry::ComponentRegistry,
    Children, Component, ComponentBundle, EntityId, GlobalTransform, Parent, SemanticDomain,
    Transform,
};

/// The central container for the entire ECS, holding all entities, components, and metadata.
///
/// The `World` orchestrates the CRPECS architecture. It owns the data and provides the main
/// API for interacting with the ECS state.
pub struct World {
    /// A dense list of metadata for every entity that has ever been created.
    /// The index into this vector is used as the `index` part of an `EntityId`.
    /// The `Option<EntityMetadata>` is `None` if the entity slot is currently free.
    pub(crate) entities: Vec<(EntityId, Option<EntityMetadata>)>,

    /// A list of all allocated `ComponentPage`s.
    /// A `page_id` in a `PageIndex` corresponds to an index in this vector.
    pub(crate) pages: Vec<ComponentPage>,

    /// A list of entity indices that have been freed by `despawn` and are available
    /// for reuse by `spawn`. This recycling mechanism keeps the `entities` vector dense.
    pub(crate) freed_entities: Vec<u32>,

    /// The registry that maps component types to their storage domains.
    registry: ComponentRegistry,
    // We will add more fields here later, such as a resource manager
    // or an entity ID allocator to manage recycled generations.
}

impl World {
    /// Allocates a new or recycled `EntityId` and reserves its metadata slot.
    ///
    /// This is the first step in the spawning process. It prioritizes recycling
    /// freed entity indices to keep the entity list dense. If no indices are free,
    /// it creates a new entry. It also handles incrementing the generation count
    /// for recycled entities to prevent the ABA problem.
    fn create_entity(&mut self) -> EntityId {
        if let Some(index) = self.freed_entities.pop() {
            // --- Recycle an existing slot ---
            let index = index as usize;

            // Get the EntityId slot, which is guaranteed to exist.
            let (id_slot, metadata_slot) = &mut self.entities[index];

            // Increment the generation.
            id_slot.generation += 1;

            // The slot is now occupied with new, default metadata.
            *metadata_slot = Some(EntityMetadata::default());

            // Return the new, updated ID.
            *id_slot
        } else {
            // --- Allocate a new slot ---
            let index = self.entities.len() as u32;
            let new_id = EntityId {
                index,
                generation: 0,
            };

            // Create a new slot with the new ID and occupied metadata.
            self.entities
                .push((new_id, Some(EntityMetadata::default())));
            new_id
        }
    }

    /// Finds a page suitable for the given `ComponentBundle`, or creates one if none exists.
    ///
    /// A page is considered suitable if it stores the exact same set of component types
    /// as the bundle. This method iterates through existing pages to find a match based
    /// on their canonical type signatures. If no match is found, it allocates a new `ComponentPage`.
    ///
    /// Returns the `page_id` of the suitable page.
    fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        // 1. Get the canonical signature for the bundle we want to insert.
        let bundle_type_ids = B::type_ids();

        // 2. --- Search for an existing page ---
        // Iterate through all currently allocated pages.
        for (page_id, page) in self.pages.iter().enumerate() {
            // Compare the page's signature with the bundle's signature.
            if page.type_ids == bundle_type_ids {
                // Found a perfect match. Return its ID.
                return page_id as u32;
            }
        }

        // 3. --- Create a new page if none was found ---
        // At this point, the loop has finished and found no match.
        // The ID for the new page will be the current number of pages.
        let new_page_id = self.pages.len() as u32;

        // Use the new `ComponentBundle` methods to construct the page.
        let new_page = ComponentPage {
            type_ids: bundle_type_ids,
            columns: B::create_columns(), // Create empty storage columns.
            entities: Vec::new(),         // The entity list starts empty.
        };

        // Add the new page to the world's collection.
        self.pages.push(new_page);

        new_page_id
    }

    /// A helper function to handle the `swap_remove` logic for a single component group.
    ///
    /// It removes component data from the specified page location. If another entity's
    /// data is moved during this process, it uses the `update_fn` closure to update
    /// the moved entity's metadata with its new location.
    fn remove_from_page(
        &mut self,
        entity_to_despawn: EntityId,
        location: PageIndex,
        domain: SemanticDomain,
    ) {
        let page = &mut self.pages[location.page_id as usize];
        if page.entities.is_empty() {
            return;
        }

        let last_entity_in_page = *page.entities.last().unwrap();
        page.swap_remove_row(location.row_index);

        if last_entity_in_page != entity_to_despawn {
            let (_id, metadata_opt) = &mut self.entities[last_entity_in_page.index as usize];
            let metadata = metadata_opt.as_mut().unwrap();

            // Update the metadata with the new location.
            metadata.locations.insert(domain, location);
        }
    }

    /// Creates a new, empty `World` with pre-registered internal component types.
    pub fn new() -> Self {
        let mut world = Self {
            entities: Vec::new(),
            pages: Vec::new(),
            freed_entities: Vec::new(),
            registry: ComponentRegistry::default(),
        };

        // --- Register all built-in scene components ---
        world.register_component::<Transform>(SemanticDomain::Spatial);
        world.register_component::<GlobalTransform>(SemanticDomain::Spatial);
        world.register_component::<Parent>(SemanticDomain::Spatial);
        world.register_component::<Children>(SemanticDomain::Spatial);

        world
    }

    /// Spawns a new entity with the given bundle of components.
    ///
    /// This is the primary method for creating entities. It orchestrates the entire process:
    /// 1. Allocates a new `EntityId`.
    /// 2. Finds or creates a suitable `ComponentPage` for the component bundle.
    /// 3. Pushes the component data into the page.
    /// 4. Updates the entity's metadata to point to the new data.
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
            // We create a smaller scope here to release the mutable borrow on `self.pages`
            // before we need to mutably borrow `self.entities` later.
            let page = &mut self.pages[page_id as usize];
            row_index = page.entities.len() as u32;

            unsafe {
                bundle.add_to_page(page);
            }
            page.add_entity(entity_id);
        }

        // --- Step 4: Update the entity's metadata. ---
        let location = PageIndex { page_id, row_index };

        // Get a mutable reference to the entity's metadata slot.
        let (_id, metadata_opt) = &mut self.entities[entity_id.index as usize];
        let metadata = metadata_opt.as_mut().unwrap();

        // Pass the registry to `update_metadata`.
        B::update_metadata(metadata, location, &self.registry);

        entity_id
    }

    /// Despawns an entity, removing all its components and freeing its ID for recycling.
    ///
    /// This method performs the following steps:
    /// 1. Verifies that the `EntityId` is valid by checking its index and generation.
    /// 2. Removes the entity's components from their respective pages.
    /// 3. Marks the entity's metadata slot as vacant and adds its ID to the free list.
    ///
    /// Returns `true` if the entity was valid and could be despawned, `false` otherwise.
    pub fn despawn(&mut self, entity_id: EntityId) -> bool {
        // Step 1: Validate the EntityId.
        // First, check if the index is even valid for our entities Vec.
        if entity_id.index as usize >= self.entities.len() {
            return false;
        }

        // Get the data at the slot.
        let (id_in_world, metadata_slot) = &self.entities[entity_id.index as usize];

        // An ID is valid if its generation matches the one in the world,
        // AND if the metadata slot is currently occupied (`is_some`).
        if id_in_world.generation != entity_id.generation || metadata_slot.is_none() {
            return false; // Stale ID or already despawned entity.
        }

        // --- At this point, the ID is valid. ---

        // Step 2: Take the metadata out of the slot, leaving it `None`.
        // This is what officially "kills" the entity.
        let metadata = self.entities[entity_id.index as usize].1.take().unwrap();

        // Add the now-freed index to our recycling list.
        self.freed_entities.push(entity_id.index);

        // --- Step 3: Iterate over the entity's component locations and remove them ---

        for (domain, location) in metadata.locations {
            self.remove_from_page(entity_id, location, domain);
        }

        true
    }

    /// Creates an iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This is the primary method for reading data from the ECS. The query `Q` is specified
    /// as a tuple via turbofish syntax. It can include component references (e.g., `&Position`,
    /// `&mut Velocity`) and filters (e.g., `Without<Parent>`).
    ///
    /// # Examples
    ///
    /// ```
    /// // Find all entities with a `Position` and `Velocity`.
    /// // for (pos, vel) in world.query::<(&Position, &mut Velocity)>() { ... }
    ///
    /// // Find all entities with a `Transform` but without a `Parent`.
    /// // for (transform,) in world.query::<(&Transform, Without<Parent>)>() { ... }
    /// ```
    ///
    /// The method itself is cheap. It performs a single, efficient search to find all
    /// `ComponentPage`s that match the query's criteria. The returned iterator then
    /// efficiently iterates over the data in only those pages.
    pub fn query<'a, Q: WorldQuery>(&'a self) -> Query<'a, Q> {
        // 1. Get the component and filter signatures from the query type.
        let query_type_ids = Q::type_ids();
        let without_type_ids = Q::without_type_ids();

        // 2. Find all pages that match the query's criteria.
        let mut matching_page_indices = Vec::new();
        'page_loop: for (page_id, page) in self.pages.iter().enumerate() {
            // --- Filtering Logic ---

            // A) Check for required components.
            // The page must contain ALL component types requested by the query.
            for required_type in &query_type_ids {
                // `binary_search` is fast on the sorted `page.type_ids` vector.
                if page.type_ids.binary_search(required_type).is_err() {
                    continue 'page_loop; // This page is missing a required component, skip it.
                }
            }

            // B) Check for excluded components.
            // The page must NOT contain ANY component types from the `without` filter.
            for excluded_type in &without_type_ids {
                if page.type_ids.binary_search(excluded_type).is_ok() {
                    continue 'page_loop; // This page contains an excluded component, skip it.
                }
            }

            // If we reach this point, the page is a match.
            matching_page_indices.push(page_id as u32);
        }

        // 3. Construct and return the `Query` iterator.
        Query::new(self, matching_page_indices)
    }

    /// Gets a mutable reference to a single component `T` for a given entity.
    ///
    /// This provides direct, random access to a component.
    ///
    /// Returns `None` if the entity is not alive, is not registered, or does
    /// not have the requested component.
    pub fn get_mut<T: Component>(&mut self, entity_id: EntityId) -> Option<&mut T> {
        // 1. Validate the entity ID.
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        // 2. Use the registry to find the component's domain and its location.
        let domain = self.registry.domain_of::<T>()?;
        let location = metadata.locations.get(&domain)?;

        // 3. Get the component data from the page.
        let type_id = TypeId::of::<T>();
        let page = self.pages.get_mut(location.page_id as usize)?;
        let column = page.columns.get_mut(&type_id)?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;

        vec.get_mut(location.row_index as usize)
    }

    /// Gets an immutable reference to a single component `T` for a given entity.
    ///
    /// Returns `None` if the entity is not alive, is not registered, or does
    /// not have the requested component.
    pub fn get<T: Component>(&self, entity_id: EntityId) -> Option<&T> {
        // 1. Validate the entity ID.
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        // 2. Use the registry to find the component's domain and its location.
        let domain = self.registry.domain_of::<T>()?;
        let location = metadata.locations.get(&domain)?;

        // 3. Get the component data from the page.
        let type_id = TypeId::of::<T>();
        let page = self.pages.get(location.page_id as usize)?;
        let vec = page
            .columns
            .get(&type_id)?
            .as_any()
            .downcast_ref::<Vec<T>>()?;

        // 4. Return the immutable reference.
        vec.get(location.row_index as usize)
    }

    /// Registers a component type with a specific semantic domain.
    ///
    /// This is a crucial setup step. Before a component of type `T` can be used
    /// in a bundle, it must be registered with the world to define where its
    /// data will be stored.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        self.registry.register::<T>(domain);
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
