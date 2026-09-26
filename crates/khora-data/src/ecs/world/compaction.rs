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

//! Reclaiming the dead rows left behind by migrations, within a budget.

use khora_core::ecs::entity::EntityId;

use super::World;
use crate::ecs::{page::PageIndex, SemanticDomain};

impl World {
    /// Returns `true` if any of `entity`'s live metadata locations references
    /// `(page_id, row)`. Domain-**agnostic** on purpose: in a multi-domain page a
    /// physical row is dead only when *no* domain still points at it, so a
    /// per-domain check (like the query layer's `is_live_row`) would wrongly
    /// classify a row still live for another domain as an orphan and destroy it.
    ///
    /// A dead/recycled entity (generation mismatch or vacated metadata) counts as
    /// not referencing the row, so its leftover row is reclaimable.
    fn entity_references_row(&self, entity: EntityId, page_id: u32, row: usize) -> bool {
        let Some((slot_id, metadata_opt)) = self.entities.get(entity.index as usize) else {
            return false;
        };
        if slot_id.generation != entity.generation {
            return false;
        }
        let Some(metadata) = metadata_opt.as_ref() else {
            return false;
        };
        metadata
            .locations
            .values()
            .any(|loc| loc.page_id == page_id && loc.row_index as usize == row)
    }
}

impl World {
    /// Compacts a single page: physically drops every row no live entity
    /// references (a migration orphan), preserving order for the surviving rows.
    ///
    /// Reuses [`remove_from_page`](Self::remove_from_page) as the removal
    /// primitive, so the survivor moved into each hole has its metadata repaired
    /// across **all** its domains — the validation the former `cleanup_orphan_at`
    /// lacked. Representation-only, but reordering rows changes query iteration
    /// order, so the page's domain epochs are bumped when at least one row is
    /// removed (order-sensitive Views — index-aligned light/audio lists — depend
    /// on the bump). No bump when nothing was removed.
    pub(crate) fn compact_page(&mut self, page_id: u32) {
        match self.storage.pages.get(page_id as usize) {
            Some(page) if !page.entities.is_empty() => {}
            _ => return,
        }

        let mut removed_any = false;
        let mut row = 0usize;
        loop {
            let len = self.storage.pages[page_id as usize].entities.len();
            if row >= len {
                break;
            }
            let entity = self.storage.pages[page_id as usize].entities[row];
            if self.entity_references_row(entity, page_id, row) {
                // Live for some domain — keep it and advance.
                row += 1;
            } else {
                // Orphan: `remove_from_page` swap-removes it and repoints the
                // survivor moved into the slot (across all its domains). The
                // swapped-in row now sits at `row`, so re-check the same index.
                self.remove_from_page(PageIndex {
                    page_id,
                    row_index: row as u32,
                });
                removed_any = true;
            }
        }

        if removed_any {
            // Iteration order for every domain this page participates in changed.
            let mut domains: Vec<SemanticDomain> = {
                let page = &self.storage.pages[page_id as usize];
                page.type_ids
                    .iter()
                    .filter_map(|t| self.storage.registry.get_domain(*t))
                    .collect()
            };
            domains.sort_by_key(|d| d.index());
            domains.dedup();
            for domain in domains {
                self.bump_domain_epoch(domain);
            }

            // If compaction drained the page completely, recycle its slot so a
            // later allocation reuses it instead of growing the pages vec.
            if self.storage.pages[page_id as usize].entities.is_empty() {
                self.storage.mark_page_free(page_id);
            }
        }
    }
}

impl World {
    /// Drains up to `budget` dirty pages and compacts each, returning the number
    /// of pages processed. Called once per frame by
    /// [`EcsMaintenance`](crate::ecs::EcsMaintenance) in `TickPhase::Maintenance`.
    /// Pages beyond the budget stay queued for the next frame — harmless, since
    /// the query layer already skips orphan rows via `is_live_row`.
    pub(crate) fn run_compaction(&mut self, budget: usize) -> usize {
        if budget == 0 || self.storage.dirty_pages.is_empty() {
            return 0;
        }
        let take: Vec<u32> = self
            .storage
            .dirty_pages
            .iter()
            .copied()
            .take(budget)
            .collect();
        for &page_id in &take {
            self.storage.dirty_pages.remove(&page_id);
        }
        for &page_id in &take {
            self.compact_page(page_id);
        }
        take.len()
    }
}
