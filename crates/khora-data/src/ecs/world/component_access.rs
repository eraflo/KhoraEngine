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
use crate::ecs::{page::PageIndex, Component, ComponentKey, Mut, SemanticDomain};

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

/// Errors that can occur when spawning an entity with a bundle of components.
#[derive(Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// A component type of the bundle is not registered in the ECS: it has no
    /// domain, so nothing could ever find it once stored.
    ComponentNotRegistered {
        /// The name of the unregistered component type.
        component: &'static str,
    },
    /// The bundle names one component type twice: an entity holds one of each.
    ComponentNamedTwice {
        /// The name of the repeated component type.
        component: &'static str,
    },
}

impl World {
    /// Registers a component type with a specific semantic domain.
    ///
    /// This is a crucial setup step. Before a component of type `T` can be used
    /// in a bundle, it must be registered with the world to define which semantic
    /// page group its data will be stored in.
    ///
    /// Registering a type again in the domain it already has changes nothing.
    ///
    /// # Panics
    ///
    /// When the type is already registered in another domain — every value
    /// stored so far is reached through the domain it was stored under, and
    /// moving the type would hide them all — or when another Rust type has
    /// its short name, the one name scenes, the editor and scripts know it by.
    /// [`try_register_component`](Self::try_register_component) is the
    /// fallible form.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        if let Err(error) = self.try_register_component::<T>(domain) {
            panic!("{error}");
        }
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
        // Pushed through the component's own column hook: the column is a
        // `Vec<C>` for AoS but a `FieldSoaColumn<C>` for field-SoA.
        self.attach(entity_id, ComponentKey::of::<C>(), |column| {
            component.push_into_column(column)
        })
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
        self.detach(entity_id, ComponentKey::of::<C>())
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
            self.orphan_row(loc.page_id);
        }

        location
    }

    /// Gets writable access to a single component `T` for a given entity: a
    /// [`Mut`] guard, which stamps the row on its first write.
    ///
    /// This provides direct, "random" access to a component, which can be less
    /// performant than querying but is useful for targeted modifications.
    ///
    /// # Returns
    ///
    /// `None` if the entity is not alive or does not have the requested component.
    pub fn get_mut<T: Component>(&mut self, entity_id: EntityId) -> Option<Mut<'_, T>> {
        // 1. Validate the entity ID. An out-of-range index means "not alive"
        // (e.g. a stale or malformed EntityId), so return None rather than panic.
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        // 2. Use the registry to find the component's domain and its location.
        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = *metadata.locations.get(&domain)?;

        // 3. Get the component data from the page, through the page buffer's
        // pointer, which the guard notes its write through.
        let key = ComponentKey::of::<T>();
        let observed = self.clock.observed.contains(&key);
        let tick = self.clock.tick;
        let page_id = location.page_id as usize;
        if page_id >= self.storage.pages.len() {
            return None;
        }
        // SAFETY: `self` is exclusively borrowed for the guard's life; the
        // slot is a place projection into it.
        let epoch = unsafe { World::domain_epoch_slot(self, domain) };
        let row = location.row_index as usize;
        // SAFETY: `page_id` indexes the page buffer of the exclusively
        // borrowed world; the `&mut` to the page's columns ends before the
        // guard is made. `value` is row `row` of `T`'s column, in bounds; the
        // guard holds the world's borrow, so nothing else reaches the value
        // or the page's ticks while it lives.
        unsafe {
            let page = self.storage.pages.as_mut_ptr().add(page_id);
            let vec = (*page)
                .columns
                .get_mut(&key)?
                .as_any_mut()
                .downcast_mut::<Vec<T>>()?;
            if row >= vec.len() {
                return None;
            }
            let value = vec.as_mut_ptr().add(row);
            Some(Mut::in_page(value, page, key, observed, epoch, row, tick))
        }
    }

    /// Gets writable access to components of type `T` for multiple entities
    /// simultaneously: one [`Mut`] guard each, which stamps its row on its
    /// first write.
    ///
    /// This is safer than `get_mut` in a loop because it allows retrieving multiple
    /// disjoint mutable references to components of the same type.
    ///
    /// # Returns
    ///
    /// An array of `Option<Mut<T>>`. If any entity is not found, does not have the component,
    /// or if there are duplicate requests for the same component instance, that entry will be `None`.
    pub fn get_many_mut<T: Component, const N: usize>(
        &mut self,
        ids: [EntityId; N],
    ) -> [Option<Mut<'_, T>>; N] {
        let mut results: [Option<Mut<'_, T>>; N] = std::array::from_fn(|_| None);

        let type_id = TypeId::of::<T>();
        let domain = match self.storage.registry.get_domain(type_id) {
            Some(d) => d,
            None => return results,
        };

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

        let key = ComponentKey::Rust(type_id);
        let observed = self.clock.observed.contains(&key);
        let tick = self.clock.tick;
        // SAFETY: as `get_mut`'s; the guards share the slot and write it one
        // at a time.
        let epoch = unsafe { World::domain_epoch_slot(self, domain) };

        // 3. Retrieve the guards through raw pointers to bypass split_at_mut
        // complexity: the pages through the page buffer's pointer, never a
        // reference to one, which the next would invalidate — the guards note
        // their writes through theirs.
        let page_count = self.storage.pages.len();
        let pages = self.storage.pages.as_mut_ptr();
        for i in 0..N {
            if found_mask[i] {
                let (page_id, row_index) = locations[i];
                if page_id as usize >= page_count {
                    continue;
                }
                // SAFETY: `page_id` indexes the page buffer of the exclusively
                // borrowed world, and all (page, row) pairs are unique (checked
                // above), so no two guards reach one value. The `&mut` to the
                // page's columns ends before the next page is reached, and no
                // reference to a page's ticks is made while the guards live
                // except by a guard noting its write.
                unsafe {
                    let page = pages.add(page_id as usize);
                    if let Some(column) = (*page).columns.get_mut(&key) {
                        if let Some(vec) = column.as_any_mut().downcast_mut::<Vec<T>>() {
                            // Through the buffer's pointer: a slice over
                            // the column would invalidate the items taken
                            // from it before this one.
                            let row = row_index as usize;
                            if row < vec.len() {
                                let value = vec.as_mut_ptr().add(row);
                                results[i] = Some(Mut::in_page(
                                    value, page, key, observed, epoch, row, tick,
                                ));
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
            .get(&ComponentKey::Rust(type_id))?
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
        let column = page.columns.get(&ComponentKey::of::<T>())?;
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
        let Some(column) = page.columns.get_mut(&ComponentKey::of::<T>()) else {
            return false;
        };
        value.set_in_column(column.as_mut(), location.row_index as usize);
        self.stamp_row(
            location.page_id,
            ComponentKey::of::<T>(),
            location.row_index as usize,
        );
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
        let key = ComponentKey::Rust(type_id);
        for page_id in 0..self.storage.pages.len() {
            let page = &mut self.storage.pages[page_id];
            let rows = page.row_count();
            let handed = match page.columns.get_mut(&key).and_then(|column| {
                column
                    .as_any_mut()
                    .downcast_mut::<crate::ecs::FieldSoaColumn<T>>()
            }) {
                Some(soa) => {
                    f(soa);
                    true
                }
                None => false,
            };
            // Every row handed out may have been written.
            if handed && rows > 0 {
                self.stamp_column(page_id as u32, key);
            }
        }
    }
}
