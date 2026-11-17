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

use super::{has_foreign_excluded, is_live_row, FetchContext, NativeRowPlan, WorldQuery};
use crate::ecs::{DomainBitset, QueryMode, QueryPlan, Tick, World};
use std::marker::PhantomData;

/// An iterator that yields the results of a `WorldQuery`.
///
/// This struct is created by the [`World::query()`] method.
pub struct Query<'a, Q: WorldQuery> {
    /// The world, borrowed shared for `'a` (see `_phantom`).
    world_ptr: *const World,

    /// The pages whose signature matches the query.
    matching_page_indices: Vec<u32>,

    plan: QueryPlan,

    current_page_index: usize,

    /// The next row of the current page (in `EntityScan` mode: the next entity-store slot).
    current_row_index: usize,

    /// Holds the `&'a World` borrow for the iterator's lifetime.
    _phantom: PhantomData<(&'a (), Q)>,

    /// Pre-computed bitset intersection for fast-failing transversal lookups.
    combined_bitset: Option<DomainBitset>,

    /// The per-row work of a Native iteration beyond the driver row (see
    /// [`NativeRowPlan`]). Empty for most queries.
    row_plan: NativeRowPlan,
    /// The tick the query's `Changed` terms compare against (see `since`).
    since: Tick,
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// Creates the iterator `World::query` returns.
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
            since: Tick::default(),
        }
    }
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// Sets the tick the query's [`Changed`](super::Changed) terms compare against.
    ///
    /// They keep the rows written after it. Under the default `Tick(0)`, every
    /// row counts: an observer that has seen nothing sees everything.
    pub fn since(mut self, since: Tick) -> Self {
        self.since = since;
        self
    }
}

impl<'a, Q: WorldQuery> Iterator for Query<'a, Q> {
    type Item = Q::Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.plan.mode {
            QueryMode::Native => self.next_native(),
            QueryMode::Transversal => self.next_transversal(),
            QueryMode::EntityScan => self.next_entity_scan(),
        }
    }
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// Yields the next item of a query whose terms all live in one domain.
    fn next_native(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // SAFETY: `world_ptr` was obtained from the `&'a World` passed to
            // `Query::new`, and the borrow checker holds that shared borrow alive
            // for `'a` via `_phantom`. No `&mut World` can exist concurrently, so
            // reborrowing it as `&World` here is sound.
            let world = unsafe { &*self.world_ptr };

            // A page no row of which can pass the query's filters is skipped whole.
            let page_id = self.matching_page_indices[self.current_page_index];
            let page = &world.storage.pages[page_id as usize];
            if self.current_row_index == 0 && !Q::page_passes(page, self.since) {
                self.current_page_index += 1;
                continue;
            }
            let ctx = FetchContext::read_only(world.current_tick(), self.since);

            if self.current_row_index < page.row_count() {
                let row = self.current_row_index;
                self.current_row_index += 1;

                // A migration leaves the source page's signature unchanged, so
                // an entity can appear in a matched page it has left.
                if let Some(domain) = self.plan.driver_domain {
                    if !is_live_row(world, page_id, row, page.entities[row], domain) {
                        continue;
                    }
                }
                if !Q::row_passes(page, row, self.since) {
                    continue;
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
                    match unsafe { Q::fetch_from_world(world as *const _, entity, ctx) } {
                        Some(item) => return Some(item),
                        None => continue,
                    }
                }

                // SAFETY: `page` lives in `world`, which is borrowed for `'a`, and
                // `matching_page_indices` only contains pages whose signature
                // satisfies `Q`, so every column `Q::fetch` reads is present.
                // `row < page.row_count()` keeps the row in bounds.
                let item = unsafe { Q::fetch(page as *const _, row, ctx) };
                return Some(item);
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }

    /// Yields the next item of a query naming no component, once per live entity.
    fn next_entity_scan(&mut self) -> Option<Q::Item<'a>> {
        // SAFETY: same invariant as `next_native` — `world_ptr` came from the
        // `&'a World` given to `Query::new` and that shared borrow is kept
        // alive for `'a`, so no aliasing `&mut World` exists.
        let world = unsafe { &*self.world_ptr };
        let ctx = FetchContext::read_only(world.current_tick(), self.since);
        while let Some((entity, metadata)) = world.entities.get(self.current_row_index) {
            self.current_row_index += 1;
            // A vacated slot is a dead entity; an occupied slot's id carries
            // the live generation.
            if metadata.is_none() {
                continue;
            }
            // SAFETY: `world` is a valid `&World` borrowed for `'a`;
            // `fetch_from_world` only reads the live rows of a live entity.
            if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, *entity, ctx) } {
                return Some(item);
            }
        }
        None
    }

    /// Yields the next item of a query joining several domains, driven by one domain's pages.
    fn next_transversal(&mut self) -> Option<Q::Item<'a>> {
        // No page or row filter here: a term outside the driver domain may
        // find a column of its own on the driver page — a mixed page left
        // behind when its domain moved — which is not the entity's live row.
        // `fetch_from_world` judges every term through the live locations.
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
            let ctx = FetchContext::read_only(world.current_tick(), self.since);

            if self.current_row_index < page.row_count() {
                let row = self.current_row_index;
                let entity_id = page.entities[row];
                self.current_row_index += 1;

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
                let item = unsafe { Q::fetch_from_world(world as *const _, entity_id, ctx) };
                if let Some(item) = item {
                    return Some(item);
                }
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }
}
