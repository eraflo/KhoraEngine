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

//! Internal component storage and page management.

use crate::ecs::component_registry::ComponentRegistry;
use crate::ecs::page::ComponentPage;
use crate::ecs::{
    ColumnMap, ComponentBundle, ComponentKey, DomainBitset, DomainStats, SemanticDomain,
};
use std::collections::{HashMap, HashSet};

/// Internal manager for component pages, domain bitsets, and archetype caching.
///
/// The `StorageManager` is responsible for organizing component data into `ComponentPage`s
/// and maintaining membership information via bitsets. It uses an `archetype_map` to
/// provide fast, constant-time lookups for pages matching specific component signatures.
pub(crate) struct StorageManager {
    /// All allocated component pages, grouped by their component signatures.
    pub(crate) pages: Vec<ComponentPage>,
    /// A cache mapping from a page signature (sorted component keys) to its page index.
    /// This enables $O(1)$ lookup for existing archetypes.
    pub(crate) archetype_map: HashMap<Vec<ComponentKey>, u32>,
    /// The registry defining which component types belong to which semantic domains.
    pub(crate) registry: ComponentRegistry,
    /// Bitsets for each domain where the $i$-th bit indicates if entity $i$ has components in that domain.
    pub(crate) domain_bitsets: HashMap<SemanticDomain, DomainBitset>,
    /// Running statistics for each semantic domain (e.g., entity count, page count).
    pub(crate) domain_stats: HashMap<SemanticDomain, DomainStats>,
    /// Pages that gained an orphaned row since the last maintenance pass.
    ///
    /// A component migration (`add_component` / `remove_component` /
    /// `remove_component_domain`) repoints the entity's metadata to a new page
    /// but leaves the old physical row in place. The source page id is recorded
    /// here so [`EcsMaintenance`](crate::ecs::EcsMaintenance) can compact it
    /// (drop the fully-dead rows) later in `TickPhase::Maintenance`, without any
    /// migration call site having to remember to forward the orphan.
    pub(crate) dirty_pages: HashSet<u32>,
    /// Slots in [`pages`](Self::pages) whose page became empty after compaction
    /// and can be recycled. The slot is *not* removed from `pages` (that would
    /// shift every higher `page_id`, and `page_id`s are stored in entity
    /// metadata); instead the next allocation reuses it. Safe because an empty
    /// page is referenced by no entity metadata.
    pub(crate) free_pages: Vec<u32>,
}

impl StorageManager {
    /// Creates a new `StorageManager` with the provided component registry.
    pub fn new(registry: ComponentRegistry) -> Self {
        Self {
            pages: Vec::new(),
            archetype_map: HashMap::new(),
            registry,
            domain_bitsets: HashMap::new(),
            domain_stats: HashMap::new(),
            dirty_pages: HashSet::new(),
            free_pages: Vec::new(),
        }
    }

    /// Allocates a page slot for `keys` with the given `columns`: recycles a
    /// freed slot when one is available (no `page_id` churn) and otherwise
    /// appends. Updates `archetype_map` and the domain page-count. Returns the
    /// page id.
    fn alloc_page(&mut self, keys: Vec<ComponentKey>, columns: ColumnMap) -> u32 {
        if let Some(first) = keys.first() {
            if let Some(domain) = self.registry.domain_of(*first) {
                self.domain_stats.entry(domain).or_default().page_count += 1;
            }
        }

        let page = ComponentPage {
            keys: keys.clone(),
            columns,
            entities: Vec::new(),
        };

        let page_id = if let Some(reused) = self.free_pages.pop() {
            // Reinitialise the freed slot in place — no `page_id` shifts.
            self.pages[reused as usize] = page;
            reused
        } else {
            let id = self.pages.len() as u32;
            self.pages.push(page);
            id
        };

        self.archetype_map.insert(keys, page_id);
        page_id
    }

    /// Recycles a now-empty page's slot. Called by `World::compact_page` when
    /// compaction drops a page's last row: drops the signature's `archetype_map`
    /// entry and the domain page-count (mirroring [`alloc_page`](Self::alloc_page)),
    /// then queues the slot for reuse. No-op if the page still holds rows, so a
    /// live page is never recycled.
    pub(crate) fn mark_page_free(&mut self, page_id: u32) {
        let Some(page) = self.pages.get(page_id as usize) else {
            return;
        };
        if !page.entities.is_empty() {
            return;
        }

        let keys = page.keys.clone();
        if let Some(first) = keys.first() {
            if let Some(domain) = self.registry.domain_of(*first) {
                if let Some(stats) = self.domain_stats.get_mut(&domain) {
                    stats.page_count = stats.page_count.saturating_sub(1);
                }
            }
        }
        self.archetype_map.remove(&keys);
        self.free_pages.push(page_id);
    }

    /// Finds or creates a page suitable for the given component bundle types.
    ///
    /// This method is highly optimized using an internal cache. If a page with the
    /// exact signature of the bundle already exists, its index is returned immediately.
    /// Otherwise, a new page is allocated and the cache is updated.
    pub fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        // A bundle's components are Rust ones, keyed by their `TypeId`s — in
        // the same order, so the signature stays sorted.
        let bundle_keys: Vec<ComponentKey> =
            B::type_ids().into_iter().map(ComponentKey::Rust).collect();

        // High-performance $O(1)$ lookup via archetype map.
        if let Some(&page_id) = self.archetype_map.get(&bundle_keys) {
            return page_id;
        }

        // Cache miss: allocate (recycling a freed slot when possible).
        self.alloc_page(bundle_keys, B::create_columns())
    }

    /// Finds or creates a page for a specific signature (sorted component keys).
    ///
    /// This is used during entity migrations or when adding/removing components
    /// where the components are only known dynamically, by key.
    pub fn find_or_create_page_for_signature(&mut self, signature: &[ComponentKey]) -> u32 {
        // High-performance $O(1)$ lookup via archetype map.
        if let Some(&page_id) = self.archetype_map.get(signature) {
            return page_id;
        }

        // Cache miss: allocate (recycling a freed slot when possible).
        let columns = self.registry.create_columns_for_signature(signature);
        self.alloc_page(signature.to_vec(), columns)
    }
}
