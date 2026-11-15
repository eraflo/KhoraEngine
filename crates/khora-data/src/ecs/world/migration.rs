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
use crate::ecs::{AnyVec, ComponentKey, Tick};

impl World {
    /// Adds component `key` to `entity`, `push` writing its value as the new row of its column.
    ///
    /// The entity's row migrates to the page of its new signature; the old row
    /// is returned and left for compaction.
    pub(crate) fn attach(
        &mut self,
        entity_id: EntityId,
        key: ComponentKey,
        push: impl FnOnce(&mut dyn AnyVec),
    ) -> Result<Option<PageIndex>, AddComponentError> {
        let now = self.clock.tick;
        self.attach_at(entity_id, key, push, now)
    }

    /// Adds component `key` as [`attach`](Self::attach) does, its row noted as
    /// written at `added_at`.
    ///
    /// A component moved rather than added keeps its tick.
    pub(crate) fn attach_at(
        &mut self,
        entity_id: EntityId,
        key: ComponentKey,
        push: impl FnOnce(&mut dyn AnyVec),
        added_at: Tick,
    ) -> Result<Option<PageIndex>, AddComponentError> {
        // A copy of the metadata, written back when the move is done: the
        // entity is never seen dead halfway through, even if it unwinds.
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

        let old_keys = old_location_opt.map_or(Vec::new(), |loc| {
            self.storage.pages[loc.page_id as usize].keys.clone()
        });
        let Err(at) = old_keys.binary_search(&key) else {
            return Err(AddComponentError::ComponentAlreadyExists);
        };
        let mut new_keys = old_keys.clone();
        new_keys.insert(at, key);

        let dest_page_id = self.find_or_create_page_for_signature(&new_keys);

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

            // A kept component moves with the ticks it had: a migration is not
            // a change. The added one is written now.
            let mut carried: Vec<(ComponentKey, Tick)> = Vec::new();
            if let (Some(src_page), Some(loc)) = (src_page_opt, old_location_opt) {
                for kept in &old_keys {
                    carried.push((
                        *kept,
                        src_page
                            .ticks
                            .get(kept)
                            .map_or(Tick::default(), |t| t.carried(loc.row_index as usize)),
                    ));
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

            dest_page.add_entity(entity_id, &self.clock.observed, |k| {
                carried
                    .iter()
                    .find(|(kept, _)| *kept == k)
                    .map_or(added_at, |(_, tick)| *tick)
            });
        }
        // The whole row moved: every domain of the destination page sees it.
        self.note_page_changed(dest_page_id);

        // The whole row moved, so every domain addressing it is repointed,
        // not only the added component's: the old row is left fully dead
        // rather than a partial orphan.
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
            None => {
                metadata.locations.insert(domain, new_location);
            }
        }

        self.storage
            .domain_bitsets
            .entry(domain)
            .or_default()
            .set(entity_id.index);

        if let Some(slot) = self.entities.get_metadata_mut(entity_id) {
            *slot = metadata;
        }

        self.bump_domain_epoch(domain);

        if let Some(old) = old_location_opt {
            self.orphan_row(old.page_id);
        }

        Ok(old_location_opt)
    }

    /// Removes component `key` from `entity`, keeping every other component:
    /// the reverse of [`attach`](Self::attach).
    pub(crate) fn detach(
        &mut self,
        entity_id: EntityId,
        key: ComponentKey,
    ) -> Result<Option<PageIndex>, RemoveComponentError> {
        // A copy of the metadata, written back when the change is done, so
        // the slot is never empty while it runs.
        let Some((id_in_world, Some(current))) = self.entities.get(entity_id.index as usize) else {
            return Err(RemoveComponentError::EntityNotFound);
        };
        let mut metadata = current.clone();
        if id_in_world.generation != entity_id.generation {
            return Err(RemoveComponentError::EntityNotFound);
        }

        let Some(domain) = self.storage.registry.domain_of(key) else {
            return Err(RemoveComponentError::ComponentNotRegistered);
        };

        let Some(loc) = metadata.locations.get(&domain).copied() else {
            return Err(RemoveComponentError::ComponentNotPresent);
        };

        let old_keys = &self.storage.pages[loc.page_id as usize].keys;
        let Ok(at) = old_keys.binary_search(&key) else {
            return Err(RemoveComponentError::ComponentNotPresent);
        };

        let mut new_keys = old_keys.clone();
        new_keys.remove(at);

        // The last component of the row: nothing to migrate.
        if new_keys.is_empty() {
            metadata.locations.remove(&domain);
            if let Some(bitset) = self.storage.domain_bitsets.get_mut(&domain) {
                bitset.clear(entity_id.index);
            }
            if let Some(slot) = self.entities.get_metadata_mut(entity_id) {
                *slot = metadata;
            }
            self.bump_domain_epoch(domain);
            self.orphan_row(loc.page_id);
            return Ok(Some(loc));
        }

        let dest_page_id = self.find_or_create_page_for_signature(&new_keys);

        let dest_row_index;
        {
            let (src_page, dest_page) = two_pages(
                &mut self.storage.pages,
                loc.page_id as usize,
                dest_page_id as usize,
            );
            dest_row_index = dest_page.entities.len() as u32;
            let src_row = loc.row_index as usize;

            let carried: Vec<(ComponentKey, Tick)> = new_keys
                .iter()
                .map(|kept| {
                    (
                        *kept,
                        src_page
                            .ticks
                            .get(kept)
                            .map_or(Tick::default(), |t| t.carried(src_row)),
                    )
                })
                .collect();
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

            // Every survivor moves with the ticks it had: not a change.
            let now = self.clock.tick;
            dest_page.add_entity(entity_id, &self.clock.observed, |k| {
                carried
                    .iter()
                    .find(|(kept, _)| *kept == k)
                    .map_or(now, |(_, tick)| *tick)
            });
        }
        // The whole row moved: every domain of the destination page sees it.
        self.note_page_changed(dest_page_id);

        // As in `attach`, every domain addressing the moved row is repointed.
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

        self.bump_domain_epoch(domain);
        self.orphan_row(loc.page_id);

        Ok(Some(loc))
    }
}

/// Returns pages `src` (shared) and `dest` (writable) of `pages`.
///
/// # Panics
///
/// If `src == dest`: a migration moves a row between two signatures, so two pages.
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
