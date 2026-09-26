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

//! `QueryMut`: iterating a query over an exclusive `&mut World`.

use super::{has_foreign_excluded, is_live_row, NativeRowPlan, WorldQuery};
use crate::ecs::{DomainBitset, QueryMode, QueryPlan, World};
use std::marker::PhantomData;

// ------------------------- //
// ---- QueryMut Part ---- //
// ------------------------- //

/// An iterator that yields the results of a mutable `WorldQuery`.
///
/// This struct is created by the [`World::query_mut()`] method.
pub struct QueryMut<'a, Q: WorldQuery> {
    world_ptr: *mut World,
    matching_page_indices: Vec<u32>,
    plan: QueryPlan,
    current_page_index: usize,
    current_row_index: usize,
    _phantom: PhantomData<(&'a (), Q)>,
    /// Pre-computed bitset intersection for fast-failing transversal lookups.
    combined_bitset: Option<DomainBitset>,
    /// The per-row work of a Native iteration beyond the driver row (see
    /// [`NativeRowPlan`]). Empty for most queries.
    row_plan: NativeRowPlan,
}

impl<'a, Q: WorldQuery> QueryMut<'a, Q> {
    /// Creates a new `QueryMut` iterator.
    ///
    /// This is intended to be called only by `World::query_mut()`.
    pub(crate) fn new(
        world: &'a mut World,
        plan: QueryPlan,
        matching_page_indices: Vec<u32>,
        row_plan: NativeRowPlan,
    ) -> Self {
        let combined_bitset = world.compute_query_bitset(&plan);
        Self {
            world_ptr: world as *mut _,
            matching_page_indices,
            plan,
            current_page_index: 0,
            current_row_index: 0,
            _phantom: PhantomData,
            combined_bitset,
            row_plan,
        }
    }
}

impl<'a, Q: WorldQuery> Iterator for QueryMut<'a, Q> {
    type Item = Q::Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.plan.mode {
            QueryMode::Native => self.next_native(),
            QueryMode::Transversal => self.next_transversal(),
            QueryMode::EntityScan => self.next_entity_scan(),
        }
    }
}

impl<'a, Q: WorldQuery> QueryMut<'a, Q> {
    fn next_native(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // SAFETY: `world_ptr` came from the `&'a mut World` passed to
            // `QueryMut::new`; that exclusive borrow is held for `'a` (via
            // `_phantom`), so no other reference to the `World` can be observed
            // while this reborrow lives. Each `next` call drops its `&mut World`
            // before returning, so reborrows never overlap.
            let world = unsafe { &mut *self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index] as usize;

            if self.current_row_index < world.storage.pages[page_id].row_count() {
                let row = self.current_row_index;
                self.current_row_index += 1;

                // Skip stale orphan rows (see `is_live_row`) and rows failing a
                // foreign-domain `Without` (see `has_foreign_excluded`), through
                // shared borrows taken before any `&mut` access below.
                let entity = world.storage.pages[page_id].entities[row];
                if let Some(domain) = self.plan.driver_domain {
                    if !is_live_row(world, page_id as u32, row, entity, domain) {
                        continue;
                    }
                }
                if !self.row_plan.foreign_without.is_empty()
                    && has_foreign_excluded(world, entity, &self.row_plan.foreign_without)
                {
                    continue;
                }
                if self.row_plan.fetch_from_world {
                    // SAFETY: `world_ptr` is the exclusively-borrowed `World` (see
                    // above) and keeps its write permission — unlike a pointer cast
                    // from the `world` reborrow, which `&mut T` terms may not write
                    // through; `world` is not used again. This is the entity's live
                    // driver row and each live entity has exactly one, so the
                    // `&mut` items this yields address disjoint rows.
                    match unsafe { Q::fetch_from_world(self.world_ptr as *const _, entity) } {
                        Some(item) => return Some(item),
                        None => continue,
                    }
                }

                // We get a mutable reference to the page, which is a safe operation
                // because `world` is a mutable reference.
                // SAFETY: `page` is borrowed from the exclusively-held `world`;
                // `matching_page_indices` only lists pages matching `Q`, so the
                // columns `Q::fetch` reads (and mutably aliases for `&mut`
                // queries) are present, and `row` is in bounds.
                let page = &mut world.storage.pages[page_id];
                let item = unsafe { Q::fetch(page as *mut _ as *const _, row) };
                return Some(item);
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }

    /// Mutable counterpart of `Query::next_entity_scan`: yields each live
    /// entity once, joining every term through its live locations.
    fn next_entity_scan(&mut self) -> Option<Q::Item<'a>> {
        loop {
            // SAFETY: same invariant as `next_native` — `world_ptr` came from the
            // `&'a mut World` given to `QueryMut::new` and that exclusive borrow is
            // held for `'a`. This shared reborrow ends before `fetch_from_world`
            // takes its own access below, so the two never overlap.
            let entity = {
                let world = unsafe { &*self.world_ptr };
                let (entity, metadata) = world.entities.get(self.current_row_index)?;
                self.current_row_index += 1;
                // A vacated slot is a dead entity.
                if metadata.is_none() {
                    continue;
                }
                *entity
            };
            // SAFETY: `world_ptr` is the exclusively-borrowed `World` (see above);
            // each live entity is visited once, so the `&mut` items this yields
            // address disjoint rows.
            if let Some(item) = unsafe { Q::fetch_from_world(self.world_ptr as *const _, entity) } {
                return Some(item);
            }
        }
    }

    fn next_transversal(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // SAFETY: same invariant as `next_native` — `world_ptr` came from the
            // `&'a mut World` given to `QueryMut::new`, that exclusive borrow is
            // held for `'a`, and each `next` call drops its reborrow before
            // returning, so no two `&mut World` reborrows overlap.
            let world = unsafe { &mut *self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index] as usize;
            let page = &world.storage.pages[page_id];

            // Check if there are rows left in the current page.
            if self.current_row_index < page.row_count() {
                let row = self.current_row_index;
                let entity_id = page.entities[row];
                self.current_row_index += 1;

                // Skip entities that are not in the combined bitset.
                if let Some(combined) = &self.combined_bitset {
                    if !combined.is_set(entity_id.index) {
                        continue;
                    }
                }

                // Skip stale orphan rows and dead entities (see `is_live_row`
                // and the matching check in `Query::next_transversal`).
                if let Some(domain) = self.plan.driver_domain {
                    if !is_live_row(world, page_id as u32, row, entity_id, domain) {
                        continue;
                    }
                }

                // SAFETY: `world` is the exclusively-borrowed `&mut World` reborrowed
                // above; passing it as `*const World` to `fetch_from_world` only reads
                // peer columns for an entity that exists in the driver page.
                if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, entity_id) } {
                    return Some(item);
                }
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }
}
