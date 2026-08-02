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

//! `.erg` hot-reload pump.
//!
//! A `PreExtract` data system that drains the [`AssetWatcher`], recompiles every
//! `.erg` file that changed **and every file that imports it**, and queues the
//! results for the script agent to pick up next frame.
//!
//! # Why the importers too
//!
//! Editing a module changes what its importers compile *to*, not just what it
//! compiles to. Reloading only the edited file would leave every importer
//! running code built against the previous version — the half of hot-reload that
//! produces a game whose behaviour depends on which file was saved last.
//!
//! The set of importers is derived from the same import lists the asset index
//! already extracts, so nothing here maintains a second graph.
//!
//! # Nothing is replaced until it compiles
//!
//! A save mid-edit usually does not compile. The previous program keeps running
//! and the diagnostics are logged; a pump that swapped in whatever came out
//! would make every stray keystroke stop the game.
//!
//! With no assets directory present, no watcher is registered and this does
//! nothing — hot-reload is a development affordance, and its absence is the
//! shipping path.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
use khora_data::flow::{PendingScriptReloads, ScriptReload};

use crate::asset::decoders::script::imports_of;
use crate::asset::AssetWatcher;
use crate::script_compile::{compile_module, DiskLoader};

/// The extension the pump reacts to.
const ERGON_EXTENSION: &str = ".erg";

/// The script root, relative to the assets directory.
///
/// A convention rather than a setting, and the same one the asset index applies
/// when it turns an import into a UUID — the two have to agree, so they agree by
/// both being derived from where the file is rather than from a value someone
/// can set in one place and forget in the other.
fn script_root_of(rel_path: &str) -> &str {
    match rel_path.find('/') {
        Some(index) => &rel_path[..index],
        None => "",
    }
}

/// The module path an `.erg` file is known by: its path under the script root.
fn module_path_of(rel_path: &str) -> Option<&str> {
    if !rel_path.ends_with(ERGON_EXTENSION) {
        return None;
    }
    let root = script_root_of(rel_path);
    if root.is_empty() {
        return Some(rel_path);
    }
    rel_path.strip_prefix(root)?.strip_prefix('/')
}

/// Every module under `root` that imports one of `changed`, transitively.
///
/// Walked outward from the edit rather than recompiling everything: a project
/// with two hundred scripts should not pay for all of them because one changed.
/// Bounded by the number of modules, since a module already in the set is not
/// expanded twice — which is also what stops a cyclic import from looping here,
/// even though the compiler will refuse it later.
fn importers_of(root: &Path, changed: &BTreeSet<String>) -> BTreeSet<String> {
    let mut affected = changed.clone();
    let mut grew = true;

    while grew {
        grew = false;
        let Ok(entries) = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
        else {
            break;
        };

        for entry in entries {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            let module = relative.to_string_lossy().replace('\\', "/");
            if !module.ends_with(ERGON_EXTENSION) || affected.contains(&module) {
                continue;
            }

            let Ok(source) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if imports_of(&source)
                .iter()
                .any(|import| affected.contains(import))
            {
                affected.insert(module);
                grew = true;
            }
        }
    }

    affected
}

fn script_hot_reload_system(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(watcher) = runtime.resources.get::<Arc<AssetWatcher>>() else {
        return; // No assets dir → no hot-reload, which is the shipping path.
    };
    let events = watcher.poll();
    if events.is_empty() {
        return;
    }
    let Some(pending) = runtime.resources.get::<PendingScriptReloads>() else {
        return;
    };

    // One script root per batch: the convention says a project keeps its
    // scripts under one directory, and mixing two would make an import
    // ambiguous.
    let mut changed = BTreeSet::new();
    let mut root = None;
    for event in &events {
        let Some(module) = module_path_of(&event.rel_path) else {
            continue;
        };
        root.get_or_insert_with(|| script_root_of(&event.rel_path).to_owned());
        changed.insert(module.to_owned());
    }
    if changed.is_empty() {
        return;
    }

    let script_root = watcher.assets_root().join(root.unwrap_or_default());
    let affected = importers_of(&script_root, &changed);
    let loader = DiskLoader::new(&script_root);

    for module in affected {
        let compiled = compile_module(&loader, &module);
        match compiled.program {
            Some(program) => {
                log::info!("script hot-reload: recompiled {module}");
                pending.push(ScriptReload { module, program });
            }
            None => {
                // Kept running rather than replaced. A save mid-edit usually
                // does not compile, and stopping the game for one would make
                // the feature unusable for what it is for.
                for diagnostic in &compiled.diagnostics {
                    log::error!("{module}: {}", diagnostic.message);
                }
            }
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "script_hot_reload",
        phase: TickPhase::PreExtract,
        run: script_hot_reload_system,
        order_hint: -10,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_path_drops_the_script_root() {
        assert_eq!(
            module_path_of("scripts/ai/guard.erg"),
            Some("ai/guard.erg"),
            "an import names it without the root"
        );
    }

    #[test]
    fn a_script_at_the_top_level_is_its_own_path() {
        assert_eq!(module_path_of("guard.erg"), Some("guard.erg"));
    }

    #[test]
    fn a_file_that_is_not_ergon_is_not_a_module() {
        assert_eq!(module_path_of("textures/wood.png"), None);
        assert_eq!(module_path_of("shaders/lit.wgsl"), None);
    }

    /// **The reason importers are recompiled too.** Reloading only the edited
    /// file leaves every importer running code built against the old version.
    #[test]
    fn an_importer_is_affected_by_a_change_to_what_it_imports() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("shared.erg"), "fn void Helper() { }").expect("writes");
        std::fs::write(
            dir.path().join("guard.erg"),
            "import \"shared.erg\"; fn void Main() { }",
        )
        .expect("writes");

        let changed = BTreeSet::from(["shared.erg".to_owned()]);
        let affected = importers_of(dir.path(), &changed);

        assert!(affected.contains("shared.erg"));
        assert!(affected.contains("guard.erg"), "the importer came too");
    }

    /// Transitively: an importer of an importer is running affected code as
    /// surely as the direct one.
    #[test]
    fn the_walk_reaches_an_importer_of_an_importer() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("base.erg"), "fn void Base() { }").expect("writes");
        std::fs::write(
            dir.path().join("middle.erg"),
            "import \"base.erg\"; fn void Middle() { }",
        )
        .expect("writes");
        std::fs::write(
            dir.path().join("top.erg"),
            "import \"middle.erg\"; fn void Top() { }",
        )
        .expect("writes");

        let affected = importers_of(dir.path(), &BTreeSet::from(["base.erg".to_owned()]));
        assert_eq!(affected.len(), 3, "{affected:?}");
    }

    /// A project with two hundred scripts should not pay for all of them
    /// because one changed.
    #[test]
    fn an_unrelated_module_is_left_alone() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("changed.erg"), "fn void A() { }").expect("writes");
        std::fs::write(dir.path().join("unrelated.erg"), "fn void B() { }").expect("writes");

        let affected = importers_of(dir.path(), &BTreeSet::from(["changed.erg".to_owned()]));
        assert_eq!(affected.len(), 1);
        assert!(!affected.contains("unrelated.erg"));
    }

    /// The compiler refuses a cycle later; the walk must not hang on one first.
    #[test]
    fn a_cyclic_import_does_not_loop_the_walk() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(
            dir.path().join("a.erg"),
            "import \"b.erg\"; fn void A() { }",
        )
        .expect("writes");
        std::fs::write(
            dir.path().join("b.erg"),
            "import \"a.erg\"; fn void B() { }",
        )
        .expect("writes");

        let affected = importers_of(dir.path(), &BTreeSet::from(["a.erg".to_owned()]));
        assert_eq!(affected.len(), 2);
    }
}
