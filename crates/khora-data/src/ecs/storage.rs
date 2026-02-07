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

use crate::ecs::{
    page::ComponentPage, registry::ComponentRegistry, ComponentBundle, DomainBitset, DomainStats,
    SemanticDomain,
};
use std::any::TypeId;
use std::collections::HashMap;

/// Internal manager for component pages, domain bitsets, and archetype caching.
///
/// The `StorageManager` is responsible for organizing component data into `ComponentPage`s
/// and maintaining membership information via bitsets. It uses an `archetype_map` to
/// provide fast, constant-time lookups for pages matching specific component signatures.
pub(crate) struct StorageManager {
    /// All allocated component pages, grouped by their component signatures.
    pub(crate) pages: Vec<ComponentPage>,
    /// A cache mapping from a component type signature (list of `TypeId`s) to its page index.
    /// This enables $O(1)$ lookup for existing archetypes.
    pub(crate) archetype_map: HashMap<Vec<TypeId>, u32>,
    /// The registry defining which component types belong to which semantic domains.
    pub(crate) registry: ComponentRegistry,
    /// Bitsets for each domain where the $i$-th bit indicates if entity $i$ has components in that domain.
    pub(crate) domain_bitsets: HashMap<SemanticDomain, DomainBitset>,
    /// Running statistics for each semantic domain (e.g., entity count, page count).
    pub(crate) domain_stats: HashMap<SemanticDomain, DomainStats>,
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
        }
    }

    /// Finds or creates a page suitable for the given component bundle types.
    ///
    /// This method is highly optimized using an internal cache. If a page with the
    /// exact signature of the bundle already exists, its index is returned immediately.
    /// Otherwise, a new page is allocated and the cache is updated.
    pub fn find_or_create_page_for_bundle<B: ComponentBundle>(&mut self) -> u32 {
        let bundle_type_ids = B::type_ids();

        // High-performance $O(1)$ lookup via archetype map.
        if let Some(&page_id) = self.archetype_map.get(&bundle_type_ids) {
            return page_id;
        }

        // --- Cache Miss: Create a new page ---
        let new_page_id = self.pages.len() as u32;

        // Update domain statistics.
        if let Some(first_type) = bundle_type_ids.first() {
            if let Some(domain) = self.registry.get_domain(*first_type) {
                self.domain_stats.entry(domain).or_default().page_count += 1;
            }
        }

        let new_page = ComponentPage {
            type_ids: bundle_type_ids.clone(),
            columns: B::create_columns(),
            entities: Vec::new(),
        };

        // Store the page and update the lookup map.
        self.pages.push(new_page);
        self.archetype_map.insert(bundle_type_ids, new_page_id);

        new_page_id
    }

    /// Finds or creates a page for a specific type signature (sorted TypeIds).
    ///
    /// This is used during entity migrations or when adding/removing components
    /// where the bundle type is only known dynamically via a slice of `TypeId`s.
    pub fn find_or_create_page_for_signature(&mut self, signature: &[TypeId]) -> u32 {
        // High-performance $O(1)$ lookup via archetype map.
        if let Some(&page_id) = self.archetype_map.get(signature) {
            return page_id;
        }

        // --- Cache Miss: Create a new page ---
        let new_page_id = self.pages.len() as u32;

        // Update domain statistics.
        if let Some(first_type) = signature.first() {
            if let Some(domain) = self.registry.get_domain(*first_type) {
                self.domain_stats.entry(domain).or_default().page_count += 1;
            }
        }

        let new_page = ComponentPage {
            type_ids: signature.to_vec(),
            columns: self.registry.create_columns_for_signature(signature),
            entities: Vec::new(),
        };

        // Store the page and update the lookup map.
        self.pages.push(new_page);
        self.archetype_map.insert(signature.to_vec(), new_page_id);

        new_page_id
    }
}
