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

//! Adding, removing, reading and writing one entity's components.

use std::any::TypeId;

use khora_core::ecs::entity::EntityId;

use super::World;
use crate::ecs::{page::PageIndex, Component, SemanticDomain};

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

impl World {
    /// Registers a component type with a specific semantic domain.
    ///
    /// This is a crucial setup step. Before a component of type `T` can be used
    /// in a bundle, it must be registered with the world to define which semantic
    /// page group its data will be stored in.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        self.storage.registry.register::<T>(domain);
        self.type_registry.register::<T>();
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

            // Push through the component's own column hook: the column is a
            // `Vec<C>` for AoS but a `FieldSoaColumn<C>` for field-SoA.
            let dest_col = dest_page
                .columns
                .get_mut(&TypeId::of::<C>())
                .expect("destination page signature includes the added component");
            component.push_into_column(dest_col.as_mut());

            dest_page.add_entity(entity_id);
        }

        // 5. Update metadata and put it back.
        //
        // The migration copied the entity's *whole* archetype row (every
        // component in the source page, across all its domains) into the
        // destination page. A multi-domain entity is stored in one page under
        // several domain keys all addressing the same `(page, row)` (see
        // `remove_from_page`), so repoint EVERY co-located domain — not just the
        // added component's — to keep the entity in one page (the CRPECS
        // archetype model) and leave the old row fully dead (reclaimable by
        // compaction) instead of a partial orphan with duplicated columns.
        let new_location = PageIndex {
            page_id: dest_page_id,
            row_index: dest_row_index,
        };
        match old_location_opt {
            Some(old) => {
                for loc in metadata.locations.values_mut() {
                    if *loc == old {
                        *loc = new_location;
                    }
                }
            }
            // First component in this domain — no prior row to migrate from.
            None => {
                metadata.locations.insert(domain, new_location);
            }
        }

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

        // 7. Update entity metadata to point at the new (page, row). As in
        //    `add_component`, the whole archetype row migrated, so repoint every
        //    co-located domain (not just this one) to keep the entity in one page
        //    and leave the old row fully dead.
        let new_location = PageIndex {
            page_id: dest_page_id,
            row_index: dest_row_index,
        };
        for l in metadata.locations.values_mut() {
            if *l == loc {
                *l = new_location;
            }
        }
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
    /// bulk SIMD entry point. Each call hands the kernel a [`FieldSoaColumn`](crate::ecs::FieldSoaColumn)
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
}
