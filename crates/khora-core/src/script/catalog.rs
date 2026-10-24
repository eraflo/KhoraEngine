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
//! The registration of what scripts declare writes the names they declare,
//! for the tools — an editor naming a new script keeps clear of them.

use std::sync::RwLock;

use super::ComponentName;

/// What the loaded scripts declare. The read set is written by the script
/// lane and read by the script flow; the declared names are written by the
/// declaration registry and read by the tools.
#[derive(Debug, Default)]
pub struct ScriptCatalog(RwLock<CatalogData>);

/// The catalog's contents.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CatalogData {
    /// Every component some loaded program reads with `Get` or `Has`.
    pub reads: Vec<ComponentName>,
    /// What each module's incoming program reads, announced by whatever
    /// compiled it until the lane installs it.
    pub announced: std::collections::BTreeMap<String, Vec<ComponentName>>,
    /// Every name a loaded module declares — components, behaviors,
    /// systems — as the script registry lists them: sorted, each once. A new
    /// declaration of one of these is refused.
    pub declared: Vec<ComponentName>,
    /// Moves on every change of `declared`, never otherwise: a reader that
    /// saw this generation has the current set.
    pub declared_generation: u64,
}

impl ScriptCatalog {
    /// The components some loaded program reads, and those a program about
    /// to be installed reads — so the frame that installs it has them.
    pub fn reads(&self) -> Vec<ComponentName> {
        self.0
            .read()
            .map(|data| {
                let mut reads: Vec<ComponentName> = data
                    .reads
                    .iter()
                    .chain(data.announced.values().flatten())
                    .cloned()
                    .collect();
                reads.sort();
                reads.dedup();
                reads
            })
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

    /// Records what the program `module` is about to be replaced with reads,
    /// before the lane installs it — so the flow projects those components
    /// the frame the program lands. A second announcement of the same module
    /// replaces the first.
    pub fn announce(&self, module: impl Into<String>, reads: Vec<ComponentName>) {
        let mut data = match self.0.write() {
            Ok(data) => data,
            Err(poisoned) => poisoned.into_inner(),
        };
        data.announced.insert(module.into(), reads);
    }

    /// Forgets what `module` announced: its program was installed, or
    /// replaced before it could be.
    pub fn settle(&self, module: &str) {
        let mut data = match self.0.write() {
            Ok(data) => data,
            Err(poisoned) => poisoned.into_inner(),
        };
        data.announced.remove(module);
    }

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

    fn sorted(mut reads: Vec<ComponentName>) -> Vec<ComponentName> {
        reads.sort();
        reads
    }

    /// **What the flow projects is what is loaded and what is about to be.**
    /// The read set and every module's announcement together — each
    /// component once, however many name it.
    #[test]
    fn the_reads_are_the_loaded_ones_and_the_announced_ones_together() {
        let catalog = ScriptCatalog::default();
        catalog.set_reads(names(&["RigidBody"]));
        catalog.announce("ai/guard.erg", names(&["Collider", "RigidBody"]));

        assert_eq!(
            sorted(catalog.reads()),
            names(&["Collider", "RigidBody"]),
            "the union, each name once"
        );
    }

    /// **The lane writing its read set every frame does not erase an
    /// announcement**: the program announced has not been installed yet, and
    /// the flow must go on projecting what it will read.
    #[test]
    fn setting_the_reads_keeps_what_was_announced() {
        let catalog = ScriptCatalog::default();
        catalog.announce("ai/guard.erg", names(&["Collider"]));

        catalog.set_reads(names(&["RigidBody"]));
        assert_eq!(sorted(catalog.reads()), names(&["Collider", "RigidBody"]));

        catalog.set_reads(Vec::new());
        assert_eq!(
            catalog.reads(),
            names(&["Collider"]),
            "an emptied read set leaves the announcement"
        );
    }

    /// **Settling a module drops its announcement, and only its.** Another
    /// module still waiting keeps what it announced.
    #[test]
    fn settling_a_module_drops_its_announcement_alone() {
        let catalog = ScriptCatalog::default();
        catalog.announce("ai/guard.erg", names(&["Collider"]));
        catalog.announce("ai/lamp.erg", names(&["Light"]));

        catalog.settle("ai/guard.erg");

        assert_eq!(catalog.reads(), names(&["Light"]));
    }

    /// Settling a module that announced nothing changes nothing.
    #[test]
    fn settling_a_module_that_announced_nothing_changes_nothing() {
        let catalog = ScriptCatalog::default();
        catalog.set_reads(names(&["RigidBody"]));
        catalog.announce("ai/lamp.erg", names(&["Light"]));

        catalog.settle("ai/guard.erg");

        assert_eq!(sorted(catalog.reads()), names(&["Light", "RigidBody"]));
    }

    /// **A superseded program's reads do not linger.** A module saved twice
    /// before its first version installed announces twice; only the second
    /// version will run, so only what it reads is projected.
    #[test]
    fn announcing_a_module_again_replaces_what_it_announced() {
        let catalog = ScriptCatalog::default();
        catalog.announce("ai/guard.erg", names(&["Collider"]));
        catalog.announce("ai/guard.erg", names(&["RigidBody"]));

        assert_eq!(catalog.reads(), names(&["RigidBody"]));

        catalog.announce("ai/guard.erg", Vec::new());
        assert!(
            catalog.reads().is_empty(),
            "a version reading nothing announces nothing"
        );
    }

    /// Once settled, the read set alone is what is read.
    #[test]
    fn a_settled_module_leaves_the_read_set_as_it_is() {
        let catalog = ScriptCatalog::default();
        catalog.announce("ai/guard.erg", names(&["Collider"]));
        catalog.set_reads(names(&["Collider"]));

        catalog.settle("ai/guard.erg");

        assert_eq!(
            catalog.reads(),
            names(&["Collider"]),
            "the installed program still reads it"
        );
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

    /// **The read set and the announcements leave the declared names
    /// alone** — they are what the programs read, not what the modules
    /// declare — and the declared generation does not move for them.
    #[test]
    fn reads_and_announcements_leave_the_declared_names_and_their_generation_alone() {
        let catalog = ScriptCatalog::default();
        catalog.set_declared(names(&["Health"]));
        let generation = catalog.declared_generation();

        catalog.set_reads(names(&["RigidBody"]));
        catalog.announce("ai/guard.erg", names(&["Collider"]));
        catalog.settle("ai/guard.erg");
        catalog.set_reads(Vec::new());

        assert_eq!(catalog.declared(), names(&["Health"]));
        assert_eq!(catalog.declared_generation(), generation);
    }

    /// **A declared name is not read**: the flow projects what programs read
    /// with `Get` or `Has`, not every name a module declares.
    #[test]
    fn the_declared_names_are_not_part_of_the_reads() {
        let catalog = ScriptCatalog::default();
        catalog.set_reads(names(&["RigidBody"]));

        catalog.set_declared(names(&["Health", "Patrol"]));

        assert_eq!(catalog.reads(), names(&["RigidBody"]));
    }
}
