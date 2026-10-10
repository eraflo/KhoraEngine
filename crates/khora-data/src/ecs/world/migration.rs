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

//! Moving an entity's row between pages, by component key: what adding or
//! removing one component does, whether the component has a Rust type or is
//! known only at run time.

use khora_core::ecs::entity::EntityId;

use super::component_access::{AddComponentError, RemoveComponentError};
use super::World;
use crate::ecs::page::PageIndex;
use crate::ecs::{AnyVec, ComponentKey};

impl World {
    /// Adds component `key` to `entity`, `push` writing its value as the new
    /// row of its column.
    ///
    /// This operation is designed to be fast. It performs the necessary data
    /// migration to move the entity's row for the component's domain to a new
    /// `ComponentPage` that matches the new layout.
    ///
    /// Crucially, it does NOT clean up the "hole" left in the old page. Instead,
    /// it returns the location of the orphaned data, delegating the cleanup task
    /// to the maintenance pass.
    pub(crate) fn attach(
        &mut self,
        entity_id: EntityId,
        key: ComponentKey,
        push: impl FnOnce(&mut dyn AnyVec),
    ) -> Result<Option<PageIndex>, AddComponentError> {
        // 1. Validate EntityId and get metadata — a copy, written back when
        // the move is done. The slot keeps its metadata meanwhile: an entity
        // is never seen dead halfway through a change, even one that unwinds.
        let Some((id_in_world, Some(current))) = self.entities.get(entity_id.index as usize) else {
            return Err(AddComponentError::EntityNotFound);
        };
        let mut metadata = current.clone();

        if id_in_world.generation != entity_id.generation {
            return Err(AddComponentError::EntityNotFound);
        }

        let Some(domain) = self.storage.registry.domain_of(key) else {
            return Err(AddComponentError::ComponentNotRegistered);
        };

        let old_location_opt = metadata.locations.get(&domain).copied();

        // 2. Determine old and new page signatures
        let old_keys = old_location_opt.map_or(Vec::new(), |loc| {
            self.storage.pages[loc.page_id as usize].keys.clone()
        });
        let Err(at) = old_keys.binary_search(&key) else {
            return Err(AddComponentError::ComponentAlreadyExists);
        };
        let mut new_keys = old_keys.clone();
        new_keys.insert(at, key);

        // 3. Find or create the destination page
        let dest_page_id = self.find_or_create_page_for_signature(&new_keys);

        // 4. Perform the migration
        let dest_row_index;
        {
            let (src_page_opt, dest_page) = match old_location_opt {
                Some(loc) => {
                    // A differing signature is a different page.
                    let (src, dest) = two_pages(
                        &mut self.storage.pages,
                        loc.page_id as usize,
                        dest_page_id as usize,
                    );
                    (Some(src), dest)
                }
                None => (None, &mut self.storage.pages[dest_page_id as usize]),
            };

            dest_row_index = dest_page.entities.len() as u32;

            if let (Some(src_page), Some(loc)) = (src_page_opt, old_location_opt) {
                for kept in &old_keys {
                    let src_col = &src_page.columns[kept];
                    let dest_col = dest_page
                        .columns
                        .get_mut(kept)
                        .expect("the destination page holds every kept component");
                    self.storage.registry.copy_row(
                        *kept,
                        src_col.as_ref(),
                        loc.row_index as usize,
                        dest_col.as_mut(),
                    );
                }
            }

            push(
                dest_page
                    .columns
                    .get_mut(&key)
                    .expect("destination page signature includes the added component")
                    .as_mut(),
            );

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

        if let Some(slot) = self.entities.get_metadata_mut(entity_id) {
            *slot = metadata;
        }

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

    /// Removes component `key` from `entity`, keeping every other component it
    /// carries (in any domain): the reverse of [`attach`](Self::attach).
    pub(crate) fn detach(
        &mut self,
        entity_id: EntityId,
        key: ComponentKey,
    ) -> Result<Option<PageIndex>, RemoveComponentError> {
        // 1. Validate the entity, and copy its metadata — written back when
        // the change is done, so the slot is never empty while it runs.
        let Some((id_in_world, Some(current))) = self.entities.get(entity_id.index as usize) else {
            return Err(RemoveComponentError::EntityNotFound);
        };
        let mut metadata = current.clone();
        if id_in_world.generation != entity_id.generation {
            return Err(RemoveComponentError::EntityNotFound);
        }

        // 2. Resolve the component's domain.
        let Some(domain) = self.storage.registry.domain_of(key) else {
            return Err(RemoveComponentError::ComponentNotRegistered);
        };

        let Some(loc) = metadata.locations.get(&domain).copied() else {
            // Entity isn't in this domain at all.
            return Err(RemoveComponentError::ComponentNotPresent);
        };

        let old_keys = &self.storage.pages[loc.page_id as usize].keys;
        let Ok(at) = old_keys.binary_search(&key) else {
            // Entity is in this domain but doesn't have this component.
            return Err(RemoveComponentError::ComponentNotPresent);
        };

        // 3. Build the new signature (without the component).
        let mut new_keys = old_keys.clone();
        new_keys.remove(at);

        // 4. If it was the last component of the row, just drop the domain
        //    location and bitset bit. No page migration needed.
        if new_keys.is_empty() {
            metadata.locations.remove(&domain);
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            if let Some(slot) = self.entities.get_metadata_mut(entity_id) {
                *slot = metadata;
            }
            // The entity left this domain entirely — invalidate cached Views.
            self.bump_domain_epoch(domain);
            // The row is orphaned (metadata no longer references it) — schedule
            // its page for compaction.
            self.storage.dirty_pages.insert(loc.page_id);
            return Ok(Some(loc));
        }

        // 5. Find or create the destination page for the reduced signature.
        let dest_page_id = self.find_or_create_page_for_signature(&new_keys);

        // 6. Migrate every surviving component from src → dest.
        let dest_row_index;
        {
            let (src_page, dest_page) = two_pages(
                &mut self.storage.pages,
                loc.page_id as usize,
                dest_page_id as usize,
            );
            dest_row_index = dest_page.entities.len() as u32;
            let src_row = loc.row_index as usize;

            for kept in &new_keys {
                let src_col = &src_page.columns[kept];
                let dest_col = dest_page
                    .columns
                    .get_mut(kept)
                    .expect("the destination page holds every kept component");
                self.storage
                    .registry
                    .copy_row(*kept, src_col.as_ref(), src_row, dest_col.as_mut());
            }

            dest_page.add_entity(entity_id);
        }

        // 7. Update entity metadata to point at the new (page, row). As in
        //    `attach`, the whole archetype row migrated, so repoint every
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
        if let Some(slot) = self.entities.get_metadata_mut(entity_id) {
            *slot = metadata;
        }

        // The entity lost a component in this domain — invalidate cached Views.
        self.bump_domain_epoch(domain);

        // 8. Record the abandoned source page so maintenance compacts its
        //    now-orphaned row later.
        self.storage.dirty_pages.insert(loc.page_id);

        // 9. Hand the old location off to the GC.
        Ok(Some(loc))
    }
}

/// Pages `src` and `dest` of `pages`, one shared and one writable.
///
/// # Panics
///
/// When `src == dest`: a migration always moves a row between two signatures,
/// so two different pages.
fn two_pages<T>(pages: &mut [T], src: usize, dest: usize) -> (&T, &mut T) {
    assert_ne!(src, dest, "a migration moves a row between two pages");
    if src < dest {
        let (low, high) = pages.split_at_mut(dest);
        (&low[src], &mut high[0])
    } else {
        let (low, high) = pages.split_at_mut(src);
        (&high[0], &mut low[dest])
    }
}
