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

//! `Query`: iterating a query over a shared `&World`.

use super::{has_foreign_excluded, is_live_row, NativeRowPlan, WorldQuery};
use crate::ecs::{DomainBitset, QueryMode, QueryPlan, World};
use std::marker::PhantomData;

// -------------------- //
// ---- Query Part ---- //
// -------------------- //

/// An iterator that yields the results of a `WorldQuery`.
///
/// This struct is created by the [`World::query()`] method. It holds a reference
/// to the world and iterates through all `ComponentPage`s that match the query's
/// signature, fetching the requested data for each entity in those pages.
pub struct Query<'a, Q: WorldQuery> {
    /// A raw pointer to the world. Using a pointer avoids lifetime variance issues
    /// and gives us the flexibility to provide either mutable or immutable access.
    world_ptr: *const World,

    /// The list of page indices that match this query (used in Native mode).
    matching_page_indices: Vec<u32>,

    /// The execution plan for this query.
    plan: QueryPlan,

    /// The index of the current page we are iterating through.
    current_page_index: usize,

    /// The index of the next row to fetch within the current page (in
    /// EntityScan mode: the next entity-store slot).
    current_row_index: usize,

    /// A marker to associate this iterator with the lifetime `'a` and the query type `Q`.
    /// It tells Rust that our iterator behaves as if it's borrowing `&'a Q` from the world.
    _phantom: PhantomData<(&'a (), Q)>,

    /// Pre-computed bitset intersection for fast-failing transversal lookups.
    combined_bitset: Option<DomainBitset>,

    /// The per-row work of a Native iteration beyond the driver row (see
    /// [`NativeRowPlan`]). Empty for most queries.
    row_plan: NativeRowPlan,
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// (Internal) Creates a new `Query` iterator.
    ///
    /// This is intended to be called only by `World::query()`.
    /// It takes the world, the plan (strategy), the pre-calculated list
    /// of matching pages, and the Native per-row plan as arguments.
    pub(crate) fn new(
        world: &'a World,
        plan: QueryPlan,
        matching_page_indices: Vec<u32>,
        row_plan: NativeRowPlan,
    ) -> Self {
        let combined_bitset = world.compute_query_bitset(&plan);
        Self {
            world_ptr: world as *const _,
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

impl<'a, Q: WorldQuery> Iterator for Query<'a, Q> {
    /// The type of item yielded by this iterator, as defined by the `WorldQuery` trait.
    type Item = Q::Item<'a>;

    /// Advances the iterator and returns the next item.
    fn next(&mut self) -> Option<Self::Item> {
        // Delegate to the appropriate execution path based on the pre-calculated plan.
        match self.plan.mode {
            QueryMode::Native => self.next_native(),
            QueryMode::Transversal => self.next_transversal(),
            QueryMode::EntityScan => self.next_entity_scan(),
        }
    }
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// (Internal) Performs a "Native" iteration, fetching data from a single domain.
    /// This is the most efficient execution path.
    fn next_native(&mut self) -> Option<Q::Item<'a>> {
        loop {
            // 1. Check if there are any pages left to iterate through.
            if self.current_page_index >= self.matching_page_indices.len() {
                return None; // No more pages, iteration is finished.
            }

            // SAFETY: `world_ptr` was obtained from the `&'a World` passed to
            // `Query::new`, and the borrow checker holds that shared borrow alive
            // for `'a` via `_phantom`. No `&mut World` can exist concurrently, so
            // reborrowing it as `&World` here is sound.
            let world = unsafe { &*self.world_ptr };

            // 2. Get the current page.
            let page_id = self.matching_page_indices[self.current_page_index];
            let page = &world.storage.pages[page_id as usize];

            // 3. Check if there are rows left in the current page.
            if self.current_row_index < page.row_count() {
                let row = self.current_row_index;
                self.current_row_index += 1;

                // Skip stale orphan rows (see `is_live_row`): `find_matching_pages`
                // filters by page signature, which a migration leaves unchanged on
                // the abandoned old page, so an entity can appear in a matched page
                // it no longer lives in.
                if let Some(domain) = self.plan.driver_domain {
                    if !is_live_row(world, page_id, row, page.entities[row], domain) {
                        continue;
                    }
                }
                let entity = page.entities[row];
                if !self.row_plan.foreign_without.is_empty()
                    && has_foreign_excluded(world, entity, &self.row_plan.foreign_without)
                {
                    continue;
                }
                if self.row_plan.fetch_from_world {
                    // SAFETY: `world` is a valid `&World` borrowed for `'a`; this is
                    // the entity's live driver row, so `fetch_from_world` joins its
                    // live rows only.
                    match unsafe { Q::fetch_from_world(world as *const _, entity) } {
                        Some(item) => return Some(item),
                        None => continue,
                    }
                }

                // SAFETY: `page` lives in `world`, which is borrowed for `'a`, and
                // `matching_page_indices` only contains pages whose signature
                // satisfies `Q`, so every column `Q::fetch` reads is present.
                // `row < page.row_count()` keeps the row in bounds.
                let item = unsafe { Q::fetch(page as *const _, row) };
                return Some(item);
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0; // Reset the row index for the new page.
                                            // The `loop` will then re-evaluate with the new page index.
            }
        }
    }

    /// (Internal) Performs an "EntityScan" iteration for a query naming no
    /// component: walks the entity store and yields each live entity once,
    /// joining every term through the entity's live locations.
    fn next_entity_scan(&mut self) -> Option<Q::Item<'a>> {
        // SAFETY: same invariant as `next_native` — `world_ptr` came from the
        // `&'a World` given to `Query::new` and that shared borrow is kept
        // alive for `'a`, so no aliasing `&mut World` exists.
        let world = unsafe { &*self.world_ptr };
        while let Some((entity, metadata)) = world.entities.get(self.current_row_index) {
            self.current_row_index += 1;
            // A vacated slot is a dead entity; an occupied slot's id carries
            // the live generation.
            if metadata.is_none() {
                continue;
            }
            // SAFETY: `world` is a valid `&World` borrowed for `'a`;
            // `fetch_from_world` only reads the live rows of a live entity.
            if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, *entity) } {
                return Some(item);
            }
        }
        None
    }

    /// (Internal) Performs a "Transversal" iteration, joining data across domains.
    /// This uses the driver domain to find entities and then pulls peer data from other domains.
    fn next_transversal(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // SAFETY: same invariant as `next_native` — `world_ptr` came from the
            // `&'a World` given to `Query::new` and that shared borrow is kept
            // alive for `'a`, so no aliasing `&mut World` exists.
            let world = unsafe { &*self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index];
            let page = &world.storage.pages[page_id as usize];

            if self.current_row_index < page.row_count() {
                // In transversal mode, we use the EntityId from the driver page to look up
                // counterpart components in the peer domains.
                let row = self.current_row_index;
                let entity_id = page.entities[row];
                self.current_row_index += 1;

                // Optimization: Skip metadata lookup if the entity is not in the combined bitset.
                if let Some(bitset) = &self.combined_bitset {
                    if !bitset.is_set(entity_id.index) {
                        continue;
                    }
                }

                // Skip stale orphan rows and dead entities (see `is_live_row`):
                // the bitset is keyed by entity index only, so it still matches
                // an orphan row of a live entity (a duplicate item) and a dead
                // orphan whose index was recycled (a dead `EntityId`).
                if let Some(domain) = self.plan.driver_domain {
                    if !is_live_row(world, page_id, row, entity_id, domain) {
                        continue;
                    }
                }

                // SAFETY: `world` is a valid `&World` borrowed for `'a` (see the
                // deref above); `fetch_from_world` only reads peer columns through
                // it for an entity that exists in the driver page.
                if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, entity_id) } {
                    return Some(item);
                }
                // If the join failed for this entity, we continue to the next one.
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }
}
