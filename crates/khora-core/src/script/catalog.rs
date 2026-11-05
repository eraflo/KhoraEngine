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

//! What the loaded scripts declare, for the tools that cannot see them.
//!
//! The registration of what scripts declare writes the names they declare,
//! for the tools — an editor naming a new script keeps clear of them. What
//! scripts read is asked of the script flow through the
//! [`ScriptReads`](super::ScriptReads) demand, not here.

use std::sync::RwLock;

use super::ComponentName;

/// What the loaded scripts declare: written by the declaration registry,
/// read by the tools.
#[derive(Debug, Default)]
pub struct ScriptCatalog(RwLock<CatalogData>);

/// The catalog's contents.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CatalogData {
    /// Every name a loaded module declares — components, behaviors,
    /// systems — as the script registry lists them: sorted, each once. A new
    /// declaration of one of these is refused.
    pub declared: Vec<ComponentName>,
    /// Moves on every change of `declared`, never otherwise: a reader that
    /// saw this generation has the current set.
    pub declared_generation: u64,
}

impl ScriptCatalog {
    /// Replaces the declared names; `true` when they changed.
    ///
    /// Kept sorted and without duplicates, so two lists naming the same
    /// declarations are one set. The generation moves only on a change.
    pub fn set_declared(&self, mut names: Vec<ComponentName>) -> bool {
        names.sort();
        names.dedup();
        let mut data = match self.0.write() {
            Ok(data) => data,
            Err(poisoned) => poisoned.into_inner(),
        };
        if data.declared == names {
            return false;
        }
        data.declared = names;
        data.declared_generation += 1;
        true
    }

    /// Every name a loaded module declares now, sorted.
    pub fn declared(&self) -> Vec<ComponentName> {
        match self.0.read() {
            Ok(data) => data.declared.clone(),
            Err(poisoned) => poisoned.into_inner().declared.clone(),
        }
    }

    /// The generation of the declared names: moves on every change of the
    /// set, never otherwise.
    pub fn declared_generation(&self) -> u64 {
        match self.0.read() {
            Ok(data) => data.declared_generation,
            Err(poisoned) => poisoned.into_inner().declared_generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<ComponentName> {
        list.iter().map(|name| ComponentName::new(*name)).collect()
    }

    // ─── The names the loaded scripts declare ────────────────────────

    /// **An empty catalog declares nothing, at generation 0** — a tool that
    /// never saw a set has seen this one.
    #[test]
    fn an_empty_catalog_declares_nothing_at_generation_zero() {
        let catalog = ScriptCatalog::default();

        assert!(catalog.declared().is_empty(), "{:?}", catalog.declared());
        assert_eq!(catalog.declared_generation(), 0);
    }

    /// **The declared names read back sorted, each once**, however the
    /// registry listed them.
    #[test]
    fn the_declared_names_read_back_sorted_and_each_once() {
        let catalog = ScriptCatalog::default();

        assert!(
            catalog.set_declared(names(&["Patrol", "Health", "Clock", "Health"])),
            "a first set is a change"
        );

        assert_eq!(catalog.declared(), names(&["Clock", "Health", "Patrol"]));
    }

    /// **The same declared set again is no change**, reordered or with a
    /// name twice; a shrunk or an emptied set is one.
    #[test]
    fn setting_the_same_declared_names_again_is_no_change() {
        let catalog = ScriptCatalog::default();
        assert!(
            catalog.set_declared(names(&["Health", "Patrol"])),
            "the first set"
        );

        assert!(
            !catalog.set_declared(names(&["Health", "Patrol"])),
            "the same set again"
        );
        assert!(
            !catalog.set_declared(names(&["Patrol", "Health", "Patrol"])),
            "the same set, reordered and with a duplicate"
        );
        assert!(catalog.set_declared(names(&["Health"])), "a shrunk set");
        assert_eq!(catalog.declared(), names(&["Health"]));
        assert!(catalog.set_declared(Vec::new()), "an emptied set");
        assert!(catalog.declared().is_empty(), "{:?}", catalog.declared());
        assert!(
            !catalog.set_declared(Vec::new()),
            "an empty set again is no change"
        );
    }

    /// **The generation moves when the declared set changes, and only
    /// then** — so a tool copying on a moved generation copies once per
    /// change, and every change.
    #[test]
    fn the_declared_generation_moves_only_when_the_set_changes() {
        let catalog = ScriptCatalog::default();
        let start = catalog.declared_generation();

        catalog.set_declared(names(&["Health"]));
        let first = catalog.declared_generation();
        assert_ne!(first, start, "a first set moves it");

        catalog.set_declared(names(&["Health", "Health"]));
        assert_eq!(
            catalog.declared_generation(),
            first,
            "the same set does not move it"
        );

        catalog.set_declared(names(&["Health", "Patrol"]));
        let grown = catalog.declared_generation();
        assert!(
            grown != first && grown != start,
            "a grown set moves it to a generation not seen before"
        );

        catalog.set_declared(Vec::new());
        let emptied = catalog.declared_generation();
        assert!(
            emptied != grown && emptied != first && emptied != start,
            "an emptied set moves it to a generation not seen before — even              though the set is the empty one it started as"
        );
    }
}
