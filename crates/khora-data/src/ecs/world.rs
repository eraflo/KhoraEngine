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

use crate::ecs::{
    entity::EntityMetadata,
    page::{ComponentPage, PageIndex},
    ComponentBundle, EntityId,
};

/// The central container for the entire ECS, holding all entities, components, and metadata.
///
/// The `World` orchestrates the CRPECS architecture. It owns the data and provides the main
/// API for interacting with the ECS state.
#[derive(Default)]
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
    fn remove_from_page<F>(
        &mut self,
        entity_to_despawn: EntityId,
        location: PageIndex,
        mut update_fn: F,
    ) where
        // The closure now operates on the `Option<EntityMetadata>` part of the tuple.
        F: FnMut(&mut Option<EntityMetadata>, PageIndex),
    {
        let page = &mut self.pages[location.page_id as usize];
        if page.entities.is_empty() {
            return;
        }

        let last_entity_in_page = *page.entities.last().unwrap();
        page.swap_remove_row(location.row_index);

        if last_entity_in_page != entity_to_despawn {
            // We get a mutable reference to the entire tuple `(EntityId, Option<EntityMetadata>)`.
            let metadata_tuple = &mut self.entities[last_entity_in_page.index as usize];

            // Call the provided closure to update the `Option<EntityMetadata>` part.
            // We pass a mutable reference to the second element of the tuple.
            update_fn(&mut metadata_tuple.1, location);
        }
    }

    /// Creates a new, empty `World`.
    pub fn new() -> Self {
        Self::default()
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
        // Step 1: Allocate a new EntityId and its metadata slot.
        let entity_id = self.create_entity();

        // Step 2: Find or create a page for this specific bundle layout.
        let page_id = self.find_or_create_page_for_bundle::<B>();
        let page = &mut self.pages[page_id as usize];

        // Step 3: Push the component data into the page.
        // The row_index is the position where the new data will be.
        let row_index = page.entities.len() as u32;

        // This is safe because we've guaranteed that `page` has the correct
        // layout for the bundle `B`, fulfilling the contract of `add_to_page`.
        unsafe {
            bundle.add_to_page(page);
        }

        // Keep the entity list in sync with the component columns.
        page.add_entity(entity_id);

        // Step 4: Update the entity's metadata with the new location.
        let location = PageIndex { page_id, row_index };

        // Get a mutable reference to the `Option<EntityMetadata>` part of the tuple.
        let metadata_slot = &mut self.entities[entity_id.index as usize].1;

        // Convert the `&mut Option<T>` to `Option<&mut T>`, unwrap it (which is safe),
        // and then get a mutable reference to the `EntityMetadata`.
        let metadata = metadata_slot.as_mut().unwrap();

        // Delegate the update logic to the bundle itself.
        B::update_metadata(metadata, location);

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

        // --- Step 3: Remove components from pages using our helper function ---

        // Remove components from the physics page, if they exist.
        if let Some(location) = metadata.physics_location {
            self.remove_from_page(entity_id, location, |metadata_opt, new_loc| {
                metadata_opt.as_mut().unwrap().physics_location = Some(new_loc);
            });
        }

        // Remove components from the render page, if they exist.
        if let Some(location) = metadata.render_location {
            self.remove_from_page(entity_id, location, |metadata_opt, new_loc| {
                metadata_opt.as_mut().unwrap().render_location = Some(new_loc);
            });
        }

        // etc, for other component types

        true
    }
}
