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
    /// Returns whether any live location of `entity`, in any domain, references `(page_id, row)`.
    ///
    /// Domain-agnostic: a row of a mixed page is dead only when no domain
    /// points at it. A dead or recycled entity references nothing.
    pub(super) fn entity_references_row(&self, entity: EntityId, page_id: u32, row: usize) -> bool {
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

    /// Drops every row of page `page_id` no live entity references (migration orphans).
    ///
    /// Removing a row reorders the page, so the epochs of its domains rise
    /// when at least one row went; order-sensitive Views depend on it.
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
                row += 1;
            } else {
                // The swapped-in row now sits at `row`: checked next.
                self.remove_from_page(PageIndex {
                    page_id,
                    row_index: row as u32,
                });
                removed_any = true;
            }
        }

        if removed_any {
            let mut domains: Vec<SemanticDomain> = {
                let page = &self.storage.pages[page_id as usize];
                page.keys
                    .iter()
                    .filter_map(|key| self.storage.registry.domain_of(*key))
                    .collect()
            };
            domains.sort_by_key(|d| d.index());
            domains.dedup();
            for domain in domains {
                self.bump_domain_epoch(domain);
            }

            // A drained page's slot is reused rather than growing the pages.
            if self.storage.pages[page_id as usize].entities.is_empty() {
                self.storage.mark_page_free(page_id);
            }
        }
    }

    /// Compacts up to `budget` dirty pages and returns how many it processed.
    ///
    /// Pages beyond the budget stay queued; queries skip orphan rows meanwhile.
    /// See the book: concepts/ecs.md, "Maintenance is a service, not an agent".
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
