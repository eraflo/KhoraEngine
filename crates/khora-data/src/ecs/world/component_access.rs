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

impl World {
    /// Registers component type `T` in `domain`, which decides the pages that store it.
    ///
    /// Registering a type again in the domain it already has changes nothing.
    ///
    /// # Panics
    ///
    /// If the type is already registered in another domain (its stored values
    /// would be hidden), or another Rust type has its short name.
    /// [`try_register_component`](Self::try_register_component) returns the error instead.
    pub fn register_component<T: Component>(&mut self, domain: SemanticDomain) {
        if let Err(error) = self.try_register_component::<T>(domain) {
            panic!("{error}");
        }
    }

    /// Adds `component` to an entity, migrating its row of that domain to a matching page.
    ///
    /// Returns the old row it left behind, if it migrated; compaction reclaims it.
    ///
    /// # Errors
    ///
    /// [`AddComponentError`] if the entity is not alive, the type is not
    /// registered, or the entity already holds one.
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

    /// Removes component `C` from an entity, keeping every other component it holds.
    ///
    /// Returns the old row it left behind, as [`add_component`](Self::add_component)
    /// does. Unlike [`remove_component_domain`](Self::remove_component_domain),
    /// the entity's other components of the same domain stay.
    ///
    /// # Errors
    ///
    /// [`RemoveComponentError`] if the entity is not alive, the type is not
    /// registered, or the entity does not hold one.
    pub fn remove_component<C: Component>(
        &mut self,
        entity_id: EntityId,
    ) -> Result<Option<PageIndex>, RemoveComponentError> {
        self.detach(entity_id, ComponentKey::of::<C>())
    }

    /// Removes every component of `C`'s domain from an entity, in O(1).
    ///
    /// Only the entity's metadata changes; the row left behind is returned and
    /// waits for compaction. `None` if the entity is not alive or holds nothing
    /// in that domain.
    pub fn remove_component_domain<C: Component>(
        &mut self,
        entity_id: EntityId,
    ) -> Option<PageIndex> {
        let (id_in_world, metadata_slot) = self.entities.get_mut(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_slot.is_none() {
            return None;
        }

        let domain = self.storage.registry.get_domain(TypeId::of::<C>())?;

        let metadata = metadata_slot.as_mut().unwrap();
        let location = metadata.locations.remove(&domain);

        if let Some(loc) = location {
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            self.bump_domain_epoch(domain);
            self.orphan_row(loc.page_id);
        }

        location
    }

    /// Returns a [`Mut`] guard over an entity's component `T`, stamping the row on its first write.
    ///
    /// `None` if the entity is not alive or does not hold the component.
    pub fn get_mut<T: Component>(&mut self, entity_id: EntityId) -> Option<Mut<'_, T>> {
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = *metadata.locations.get(&domain)?;

        // Reached through the page buffer's pointer, which the guard notes its
        // write through.
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

    /// Returns one [`Mut`] guard over component `T` per entity of `ids`, all held at once.
    ///
    /// An entry is `None` if its entity is not alive or lacks the component.
    /// If two ids reach the same value, every entry is `None`.
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

        let mut locations = [(0u32, 0u32); N];
        let mut found_mask = [false; N];

        for i in 0..N {
            if let Some((stored_id, Some(metadata))) = self.entities.get(ids[i].index as usize) {
                if *stored_id == ids[i] {
                    if let Some(loc) = metadata.locations.get(&domain) {
                        locations[i] = (loc.page_id, loc.row_index);
                        found_mask[i] = true;
                    }
                }
            }
        }

        // Two guards over one (page, row) would alias.
        for i in 0..N {
            if !found_mask[i] {
                continue;
            }
            for j in (i + 1)..N {
                if found_mask[j] && locations[i] == locations[j] {
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

        // Pages reached through the buffer's pointer, never a reference to
        // one, which the next would invalidate: the guards note their writes
        // through theirs.
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

    /// Returns a reference to an entity's component `T`.
    ///
    /// `None` if the entity is not alive, does not hold the component, or
    /// stores it field-SoA; [`clone_component`](Self::clone_component) reads any layout.
    pub fn get<T: Component>(&self, entity_id: EntityId) -> Option<&T> {
        let (id_in_world, metadata_opt) = self.entities.get(entity_id.index as usize)?;
        if id_in_world.generation != entity_id.generation || metadata_opt.is_none() {
            return None;
        }
        let metadata = metadata_opt.as_ref().unwrap();

        let domain = self.storage.registry.get_domain(TypeId::of::<T>())?;
        let location = metadata.locations.get(&domain)?;

        let type_id = TypeId::of::<T>();
        let page = self.storage.pages.get(location.page_id as usize)?;

        let vec = page
            .columns
            .get(&ComponentKey::Rust(type_id))?
            .as_any()
            .downcast_ref::<Vec<T>>()?;
        vec.get(location.row_index as usize)
    }

    /// Returns a copy of an entity's component `T`, whatever its layout.
    ///
    /// A field-SoA column holds no contiguous `T` to lend. `None` if the
    /// entity is not alive or lacks the component.
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

    /// Writes an entity's component `T` by value, whatever its layout, and stamps the row.
    ///
    /// Returns `false`, writing nothing, if the entity is not alive or lacks the component.
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

    /// Runs `f` over every field-SoA column of component `T`, one page at a time.
    ///
    /// Each [`FieldSoaColumn`](crate::ecs::FieldSoaColumn) has contiguous
    /// per-field `f32` lanes, for SIMD kernels; every row of a column handed
    /// out counts as written. Pages whose `T` column is not field-SoA are skipped.
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
