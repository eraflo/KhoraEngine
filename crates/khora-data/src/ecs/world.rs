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

use std::{any::TypeId, collections::HashMap};

use bincode::config;
use khora_core::{
    ecs::entity::EntityId,
    renderer::{GpuMesh, Mesh},
};

use crate::ecs::{
    components::HandleComponent,
    entity::EntityMetadata,
    page::{ComponentPage, PageIndex},
    query::{Query, WorldQuery},
    registry::ComponentRegistry,
    serialization::SceneMemoryLayout,
    AudioListener, AudioSource, Children, Component, ComponentBundle, GlobalTransform,
    MaterialComponent, Parent, QueryMut, SemanticDomain, SerializedPage, Transform, TypeRegistry,
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

/// A trait providing low-level access to the World for maintenance tasks.
///
/// This trait should only be used by trusted, engine-internal systems like
/// a `CompactionLane`, which need to perform dangerous operations like
/// cleaning up orphaned data. It is not part of the public API for game logic.
pub trait WorldMaintenance {
    /// Cleans up an orphaned data slot in a page.
    fn cleanup_orphan_at(&mut self, location: PageIndex, domain: SemanticDomain);
}

/// The central container for the entire ECS, holding all entities, components, and metadata.
///
/// The `World` orchestrates the CRPECS architecture. It owns all ECS data and provides the main
/// API for creating and destroying entities, and for querying their component data.
pub struct World {
    /// A dense list of metadata for every entity slot that has ever been created.
    /// The index into this vector is used as the `index` part of an `EntityId`.
    /// The `Option<EntityMetadata>` is `None` if the entity slot is currently free.
    pub(crate) entities: Vec<(EntityId, Option<EntityMetadata>)>,

    /// A list of all allocated `ComponentPage`s, where component data is stored.
    /// A `page_id` in a `PageIndex` corresponds to an index in this vector.
    pub(crate) pages: Vec<ComponentPage>,

    /// A list of entity indices that have been freed by `despawn` and are available
    /// for reuse by `spawn`. This recycling mechanism keeps the `entities` vector dense.
    pub(crate) freed_entities: Vec<u32>,

    /// The registry that maps component types to their storage domains.
    registry: ComponentRegistry,

    /// The type registry for serialization purposes.
    type_registry: TypeRegistry,
    // We will add more fields here later, such as a resource manager
    // or an entity ID allocator to manage recycled generations.
}

impl World {
    /// (Internal) Allocates a new or recycled `EntityId` and reserves its metadata slot.
    ///
    /// This is the first step in the spawning process. It prioritizes recycling
    /// freed entity indices to keep the entity list dense. If no indices are free,
    /// it creates a new entry. It also handles incrementing the generation count
    /// for recycled entities to prevent the ABA problem.
    fn create_entity(&mut self) -> EntityId {
        if let Some(index) = self.freed_entities.pop() {
            // --- Recycle an existing slot ---
            let index = index as usize;
            let (id_slot, metadata_slot) = &mut self.entities[index];
            id_slot.generation += 1;
            *metadata_slot = Some(EntityMetadata::default());
            *id_slot
        } else {
            // --- Allocate a new slot ---
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

    /// (Internal) Finds a page suitable for the given `ComponentBundle`, or creates one if none exists.
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
            if page.type_ids == bundle_type_ids {
                return page_id as u32;
            }
        }

        // 3. --- Create a new page if none was found ---
        // At this point, the loop has finished and found no match.
        // The ID for the new page will be the current number of pages.
        let new_page_id = self.pages.len() as u32;
        let new_page = ComponentPage {
            type_ids: bundle_type_ids,
            columns: B::create_columns(),
            entities: Vec::new(),
        };
        self.pages.push(new_page);
        new_page_id
    }

    /// (Internal) A helper function to handle the `swap_remove` logic for a single component group.
    ///
    /// It removes component data from the specified page location. If another entity's
    /// data is moved during this process, its metadata is updated with its new location.
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
            metadata.locations.insert(domain, location);
        }
    }

    /// Finds or creates a page for the given signature of component `TypeId`s.
    fn find_or_create_page_for_signature(&mut self, signature: &[TypeId]) -> u32 {
        if let Some((id, _)) = self
            .pages
            .iter()
            .enumerate()
            .find(|(_, p)| p.type_ids == signature)
        {
            return id as u32;
        }

        let new_page_id = self.pages.len() as u32;
        let mut columns = HashMap::new();
        for type_id in signature {
            let constructor = self.registry.get_column_constructor(type_id).unwrap();
            columns.insert(*type_id, constructor());
        }

        self.pages.push(ComponentPage {
            type_ids: signature.to_vec(),
            columns,
            entities: Vec::new(),
        });
        new_page_id
    }

    /// Creates a new, empty `World` with pre-registered internal component types.
    pub fn new() -> Self {
        let mut world = Self {
            entities: Vec::new(),
            pages: Vec::new(),
            freed_entities: Vec::new(),
            registry: ComponentRegistry::default(),
            type_registry: TypeRegistry::default(),
        };
        // Registration of built-in components
        world.register_component::<Transform>(SemanticDomain::Spatial);
        world.register_component::<GlobalTransform>(SemanticDomain::Spatial);
        world.register_component::<Parent>(SemanticDomain::Spatial);
        world.register_component::<Children>(SemanticDomain::Spatial);

        // Registration of render components
        world.register_component::<HandleComponent<Mesh>>(SemanticDomain::Render);
        world.register_component::<HandleComponent<GpuMesh>>(SemanticDomain::Render);
        world.register_component::<MaterialComponent>(SemanticDomain::Render);

        // Registration of audio components
        world.register_component::<AudioSource>(SemanticDomain::Audio);
        world.register_component::<AudioListener>(SemanticDomain::Audio);

        world
    }

    /// Spawns a new entity with the given bundle of components.
    ///
    /// This is the primary method for creating entities. It orchestrates the entire process:
    /// 1. Allocates a new `EntityId`.
    /// 2. Finds or creates a suitable `ComponentPage` for the component bundle.
    /// 3. Pushes the component data into the page's columns.
    /// 4. Updates the entity's metadata to point to the new data's location.
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
            let page = &mut self.pages[page_id as usize];
            row_index = page.entities.len() as u32;
            unsafe {
                bundle.add_to_page(page);
            }
            page.add_entity(entity_id);
        }

        // --- Step 4: Update the entity's metadata. ---
        let location = PageIndex { page_id, row_index };
        let (_id, metadata_opt) = &mut self.entities[entity_id.index as usize];
        let metadata = metadata_opt.as_mut().unwrap();
        B::update_metadata(metadata, location, &self.registry);
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
        let (id_in_world, metadata_slot) = &self.entities[entity_id.index as usize];

        // An ID is valid if its generation matches the one in the world,
        // AND if the metadata slot is currently occupied (`is_some`).
        if id_in_world.generation != entity_id.generation || metadata_slot.is_none() {
            return false;
        }

        // --- At this point, the ID is valid. ---

        // Step 2: Take the metadata out of the slot, leaving it `None`.
        // This is what officially "kills" the entity.
        let metadata = self.entities[entity_id.index as usize].1.take().unwrap();
        self.freed_entities.push(entity_id.index);

        // --- Step 3: Iterate over the entity's component locations and remove them ---
        for (domain, location) in metadata.locations {
            self.remove_from_page(entity_id, location, domain);
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

    /// Creates a mutable iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This method is similar to `query`, but it allows mutable access to the components.
    /// The same filtering logic applies, ensuring that only pages containing the required
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
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
        QueryMut::new(self, matching_page_indices)
    }

    /// Registers a component type with a specific semantic domain.
    ///
    /// This is a crucial setup step. Before a component of type `T` can be used
    /// in a bundle, it must be registered with the world to define which semantic
    /// page group its data will be stored in.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        self.registry.register::<T>(domain);
        self.type_registry.register::<T>();
    }

    /// Adds a new component `C` to an existing entity, migrating its domain data.
    ///
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

        let Some(domain) = self.registry.domain_of::<C>() else {
            return Err(AddComponentError::ComponentNotRegistered);
        };

        let mut metadata = self.entities[entity_id.index as usize].1.take().unwrap();
        let old_location_opt = metadata.locations.get(&domain).copied();

        // 2. Determine old and new page signatures
        let old_type_ids = old_location_opt.map_or(Vec::new(), |loc| {
            self.pages[loc.page_id as usize].type_ids.clone()
        });
        let mut new_type_ids = old_type_ids.clone();
        new_type_ids.push(TypeId::of::<C>());
        new_type_ids.sort();
        new_type_ids.dedup();

        if new_type_ids == old_type_ids {
            self.entities[entity_id.index as usize].1 = Some(metadata); // Put it back
            return Err(AddComponentError::ComponentAlreadyExists);
        }

        // 3. Find or create the destination page
        let dest_page_id = self.find_or_create_page_for_signature(&new_type_ids);

        // 4. Perform the migration
        let dest_row_index;
        unsafe {
            let (src_page_opt, dest_page) = if let Some(loc) = old_location_opt {
                if loc.page_id == dest_page_id {
                    unreachable!(); // Should be caught by signature check above
                } else {
                    // This unsafe block is needed to get mutable access to two different pages
                    let all_pages_ptr = self.pages.as_mut_ptr();
                    let dest_page = &mut *all_pages_ptr.add(dest_page_id as usize);
                    let src_page = &*all_pages_ptr.add(loc.page_id as usize);
                    (Some(src_page), dest_page)
                }
            } else {
                (None, &mut self.pages[dest_page_id as usize])
            };

            dest_row_index = dest_page.entities.len() as u32;

            if let Some(src_page) = src_page_opt {
                let src_row = old_location_opt.unwrap().row_index as usize;
                for type_id in &old_type_ids {
                    let copier = self.registry.get_row_copier(type_id).unwrap();
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
        self.entities[entity_id.index as usize].1 = Some(metadata);

        // 6. Return the old location for cleanup, without performing swap_remove
        Ok(old_location_opt)
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
        let domain = self.registry.domain_of::<C>()?;

        // 3. Remove the location entry from the entity's metadata.
        //    `HashMap::remove` returns the value that was at that key, which is exactly what we need.
        let metadata = metadata_slot.as_mut().unwrap();
        metadata.locations.remove(&domain)
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
    /// This provides direct, "random" access to a component.
    ///
    /// # Returns
    ///
    /// `None` if the entity is not alive or does not have the requested component.
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

        // 4. Return the immutable reference.
        let vec = page
            .columns
            .get(&type_id)?
            .as_any()
            .downcast_ref::<Vec<T>>()?;
        vec.get(location.row_index as usize)
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
        let mut serialized_pages = Vec::with_capacity(self.pages.len());
        for page in &self.pages {
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
                // UNSAFE: Copying raw bytes from the component vector.
                let bytes = unsafe { column.as_bytes() };
                serialized_columns.insert(type_name.to_string(), bytes.to_vec());
            }

            serialized_pages.push(SerializedPage {
                type_names,
                entities: page.entities.clone(),
                columns: serialized_columns,
            });
        }
        let layout = SceneMemoryLayout {
            entities: self.entities.clone(),
            freed_entities: self.freed_entities.clone(),
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
    ) -> Result<(), bincode::error::DecodeError> {
        let (layout, _): (SceneMemoryLayout, _) =
            bincode::decode_from_slice(data, config::standard())?;

        self.entities = layout.entities;
        self.freed_entities = layout.freed_entities;
        self.pages.clear();

        for serialized_page in layout.pages {
            // Use the TypeRegistry to convert string names back to TypeIds.
            let type_ids: Vec<TypeId> = serialized_page
                .type_names
                .iter()
                .map(|name| {
                    self.type_registry
                        .get_id_of(name)
                        .expect("Serialized component type not registered")
                })
                .collect();

            let mut new_page = ComponentPage {
                type_ids,
                entities: serialized_page.entities,
                columns: HashMap::new(),
            };

            for (type_name, bytes) in &serialized_page.columns {
                let type_id = self.type_registry.get_id_of(type_name).unwrap();
                let constructor = self.registry.get_column_constructor(&type_id).unwrap();
                let mut column = constructor();
                // UNSAFE: Writing raw bytes into the newly created component vector.
                unsafe {
                    column.set_from_bytes(bytes);
                }
                new_page.columns.insert(type_id, column);
            }
            self.pages.push(new_page);
        }

        Ok(())
    }
}

impl WorldMaintenance for World {
    fn cleanup_orphan_at(&mut self, location: PageIndex, domain: SemanticDomain) {
        let page = &mut self.pages[location.page_id as usize];
        if page.entities.is_empty() || location.row_index as usize >= page.entities.len() {
            return;
        }

        let last_entity_in_page = *page.entities.last().unwrap();
        page.swap_remove_row(location.row_index);

        let (_id, metadata_opt) = &mut self.entities[last_entity_in_page.index as usize];
        if let Some(metadata) = metadata_opt.as_mut() {
            metadata.locations.insert(domain, location);
        }
    }
}

impl Default for World {
    /// Creates a new, empty `World` via `World::new()`.
    fn default() -> Self {
        Self::new()
    }
}
