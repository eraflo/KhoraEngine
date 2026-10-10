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

//! What the loaded scripts declare, for the layers that cannot see them.
//!
//! The script lane knows every loaded program; the flow that projects what
//! scripts may read lives below it and cannot name the lane's types. This is
//! the vocabulary they share: the lane writes the read set, the flow reads it.

use std::sync::RwLock;

use super::ComponentName;

/// What the loaded scripts declare. Written by the script lane, read by the
/// script flow.
#[derive(Debug, Default)]
pub struct ScriptCatalog(RwLock<CatalogData>);

/// The catalog's contents.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CatalogData {
    /// Every component some loaded program reads with `Get` or `Has`.
    pub reads: Vec<ComponentName>,
}

impl ScriptCatalog {
    /// The components some loaded program reads.
    pub fn reads(&self) -> Vec<ComponentName> {
        self.0
            .read()
            .map(|data| data.reads.clone())
            .unwrap_or_default()
    }

    /// Replaces the read set; `true` when it changed.
    ///
    /// Kept sorted and without duplicates, so two lists naming the same
    /// components are one read set.
    pub fn set_reads(&self, mut reads: Vec<ComponentName>) -> bool {
        reads.sort();
        reads.dedup();
        let mut data = match self.0.write() {
            Ok(data) => data,
            Err(poisoned) => poisoned.into_inner(),
        };
        if data.reads == reads {
            return false;
        }
        data.reads = reads;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<ComponentName> {
        list.iter().map(|name| ComponentName::new(*name)).collect()
    }

    /// An empty catalog reads nothing — the flow then projects nothing.
    #[test]
    fn an_empty_catalog_reads_nothing() {
        assert!(ScriptCatalog::default().reads().is_empty());
    }

    /// What the lane writes is what the flow reads back.
    #[test]
    fn the_read_set_is_what_was_set() {
        let catalog = ScriptCatalog::default();
        catalog.set_reads(names(&["Collider", "RigidBody"]));

        let mut reads = catalog.reads();
        reads.sort();
        assert_eq!(reads, names(&["Collider", "RigidBody"]));
    }

    /// **A change is reported only when there is one**, so the lane writing
    /// the same set every frame is not mistaken for a new read set.
    #[test]
    fn setting_the_same_reads_again_is_no_change() {
        let catalog = ScriptCatalog::default();

        assert!(catalog.set_reads(names(&["RigidBody"])), "the first set");
        assert!(
            !catalog.set_reads(names(&["RigidBody"])),
            "the same set again"
        );
        assert!(
            catalog.set_reads(names(&["Collider", "RigidBody"])),
            "a grown set"
        );
        assert!(catalog.set_reads(Vec::new()), "an emptied set");
    }
}
