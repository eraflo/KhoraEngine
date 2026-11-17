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

//! A world's pages, the signature each is found by, and its domain bitsets.

use crate::ecs::component_registry::ComponentRegistry;
use crate::ecs::page::ComponentPage;
use crate::ecs::{
    ColumnMap, ComponentBundle, ComponentKey, DomainBitset, DomainStats, SemanticDomain,
};
use std::collections::{HashMap, HashSet};

/// A world's component pages, with the signature map and the per-domain bitsets.
pub(crate) struct StorageManager {
    /// Every page slot, indexed by page id.
    pub(crate) pages: Vec<ComponentPage>,
    /// The page of each signature (sorted component keys).
    pub(crate) archetype_map: HashMap<Vec<ComponentKey>, u32>,
    /// The world's components.
    pub(crate) registry: ComponentRegistry,
    /// Per domain, bit `i` set when entity `i` has components in it.
    pub(crate) domain_bitsets: HashMap<SemanticDomain, DomainBitset>,
    /// Running counts per domain.
    pub(crate) domain_stats: HashMap<SemanticDomain, DomainStats>,
    /// Pages that gained an orphaned row since the last maintenance pass.
    ///
    /// A migration leaves its old row in place;
    /// [`EcsMaintenance`](crate::ecs::EcsMaintenance) compacts these pages.
    pub(crate) dirty_pages: HashSet<u32>,
    /// Slots in [`pages`](Self::pages) emptied by compaction, for the next allocation.
    ///
    /// Kept in place: removing one would shift every higher page id, which
    /// entity metadata stores. No metadata names an empty page.
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

    /// Allocates a page for `keys` with `columns`, reusing a freed slot first; returns its id.
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
            ticks: Default::default(),
            order: 0,
        };

        let page_id = if let Some(reused) = self.free_pages.pop() {
            // The slot's order version carries on: (page id, ω) never names two
            // states of the slot.
            let mut page = page;
            page.order = self.pages[reused as usize].order + 1;
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

    /// Queues an empty page's slot for reuse, forgetting its signature.
    ///
    /// Does nothing to a page that still holds rows.
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

    /// Returns the id of the page whose signature is bundle `B`'s, creating it if needed.
    pub(crate) fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        // A bundle's components are Rust ones, keyed by their `TypeId`s — in
        // the same order, so the signature stays sorted.
        let bundle_keys: Vec<ComponentKey> =
            B::type_ids().into_iter().map(ComponentKey::Rust).collect();

        if let Some(&page_id) = self.archetype_map.get(&bundle_keys) {
            return page_id;
        }

        self.alloc_page(bundle_keys, B::create_columns())
    }

    /// Returns the id of the page whose signature is `signature`, creating it if needed.
    ///
    /// `signature` is sorted.
    pub(crate) fn find_or_create_page_for_signature(&mut self, signature: &[ComponentKey]) -> u32 {
        if let Some(&page_id) = self.archetype_map.get(signature) {
            return page_id;
        }

        let columns = self.registry.create_columns_for_signature(signature);
        self.alloc_page(signature.to_vec(), columns)
    }
}
