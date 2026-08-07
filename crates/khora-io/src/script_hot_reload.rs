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
//! # Why not the asset index
//!
//! The index records each script's imports, so walking that edge backwards
//! looks like the obvious way to find importers. It is not: editing an `.erg`
//! file in place never reindexes it — the editor drops the cached handle and
//! leaves the edges alone, and nothing in the runtime path reindexes at all. So
//! the index's imports are the ones the file had at the last **create or
//! delete**, and an author who adds an `import` and saves would get an edge the
//! index has never seen. Reading the tree is slower and right; reading the
//! index would be faster and wrong.
//!
//! What *is* shared is the parser: [`imports_of`] answers the same question for
//! the index builder and for this pump, so there is one definition of what an
//! import is even though there are two ways of collecting them.
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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use khora_core::event::{Channel, WhenFull};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
use khora_script::reload::ScriptReload;

use crate::asset::decoders::script::imports_of;

/// Where this pump leaves what it recompiled, for the scripting agent to drain.
///
/// Named here rather than spelled out at each use: the pump is the only writer,
/// so the queue's identity belongs beside it — and it saves the engine wiring a
/// dependency on `khora-script` just to name the element type.
pub type PendingReloads = Channel<ScriptReload>;

/// How many recompiled modules may wait for the scripting agent.
///
/// A [`ScriptReload`] supersedes an earlier one for the same module, so repeats
/// of one file never fill this — it counts **distinct** modules changed between
/// two frames. What gets near it is a mass change: a branch switch, a generated
/// tree. Dropping the oldest then keeps what was saved most recently, which is
/// what the author is looking at.
const RELOAD_BACKLOG: usize = 1024;

/// A channel for what this pump recompiles.
pub fn reload_channel() -> PendingReloads {
    Channel::bounded(RELOAD_BACKLOG, WhenFull::DropOldest)
}
use crate::asset::AssetWatcher;
use crate::script_compile::{compile_module, DiskLoader};

/// The extension the pump reacts to.
const ERGON_EXTENSION: &str = ".erg";

/// The script root as a directory name, relative to the assets directory.
///
/// The same rule the asset index applies when it turns an import into a UUID,
/// borrowed rather than restated: a second copy of a convention is a convention
/// that can drift. The index concatenates its answer onto an import and wants
/// the trailing slash; this joins it as a path component and does not.
fn script_root_of(rel_path: &str) -> &str {
    crate::asset::dependencies::script_root(rel_path).trim_end_matches('/')
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

/// Every module in `imports` that reaches one of `changed`, transitively.
///
/// Walked outward from the edit rather than recompiling everything: a project
/// with two hundred scripts should not pay for all of them because one changed.
/// Bounded by the number of modules, since a module already in the set is not
/// expanded twice — which is also what stops a cyclic import from looping here,
/// even though the compiler will refuse it later.
///
/// Takes the graph rather than a directory, and that is the point: the previous
/// version re-walked the tree and re-read **every** file on each round of the
/// fixpoint. Handing it a map read once makes the repetition impossible instead
/// of merely avoided.
fn importers_of(
    imports: &BTreeMap<String, Vec<String>>,
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut affected = changed.clone();
    let mut grew = true;

    while grew {
        grew = false;
        for (module, imported) in imports {
            if affected.contains(module) {
                continue;
            }
            if imported.iter().any(|import| affected.contains(import)) {
                affected.insert(module.clone());
                grew = true;
            }
        }
    }

    affected
}

/// Compiles every module under `script_root` and queues them all.
///
/// The pump reacts to *changes*, which leaves the first frame with no programs
/// at all — a game whose scripts only start working once their author saves a
/// file. This is the initial load, and it takes the same road a reload does so
/// there is one way a program reaches the runtime rather than two.
///
/// Returns how many compiled. A module that does not compile is reported and
/// skipped: one broken script should not stop the others from running.
pub fn load_all(script_root: &Path, pending: &PendingReloads) -> usize {
    let loader = DiskLoader::new(script_root);
    let mut loaded = 0;

    for module in modules_under(script_root).into_keys() {
        let compiled = compile_module(&loader, &module);
        match compiled.program {
            Some(program) => {
                pending.send(ScriptReload { module, program });
                loaded += 1;
            }
            None => {
                for diagnostic in &compiled.diagnostics {
                    log::error!("{module}: {}", diagnostic.message);
                }
            }
        }
    }
    loaded
}

/// Every `.erg` module under `root`, each with what it imports.
///
/// One walk and one read per file, serving both callers: the initial load wants
/// the module names, and the pump wants the edges between them. Two walks lived
/// seventy lines apart here — one on `read_dir`, one on `walkdir` — which is one
/// walk more than there are questions to answer, and the surviving one is the
/// same `walkdir` the index builder uses so there is one way to cross a tree.
///
/// A file that cannot be read contributes its name with no imports rather than
/// vanishing: it still needs compiling, and the compiler is where an unreadable
/// file gets its diagnostic.
fn modules_under(root: &Path) -> BTreeMap<String, Vec<String>> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("erg"))
        })
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            // Forward slashes, because that is how an `import` writes a path
            // and how a module is named everywhere else.
            let module = relative.to_string_lossy().replace('\\', "/");
            let imports = std::fs::read_to_string(entry.path())
                .map(|source| imports_of(&source))
                .unwrap_or_default();
            Some((module, imports))
        })
        .collect()
}

fn script_hot_reload_system(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(watcher) = runtime.resources.get::<Arc<AssetWatcher>>() else {
        return; // No assets dir → no hot-reload, which is the shipping path.
    };
    let events = watcher.poll_for("script_hot_reload");
    if events.is_empty() {
        return;
    }
    let Some(pending) = runtime.resources.get::<PendingReloads>() else {
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
    let affected = importers_of(&modules_under(&script_root), &changed);
    let loader = DiskLoader::new(&script_root);

    for module in affected {
        let compiled = compile_module(&loader, &module);
        match compiled.program {
            Some(program) => {
                log::info!("script hot-reload: recompiled {module}");
                pending.send(ScriptReload { module, program });
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
        let affected = importers_of(&modules_under(dir.path()), &changed);

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

        let affected = importers_of(
            &modules_under(dir.path()),
            &BTreeSet::from(["base.erg".to_owned()]),
        );
        assert_eq!(affected.len(), 3, "{affected:?}");
    }

    /// A project with two hundred scripts should not pay for all of them
    /// because one changed.
    #[test]
    fn an_unrelated_module_is_left_alone() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("changed.erg"), "fn void A() { }").expect("writes");
        std::fs::write(dir.path().join("unrelated.erg"), "fn void B() { }").expect("writes");

        let affected = importers_of(
            &modules_under(dir.path()),
            &BTreeSet::from(["changed.erg".to_owned()]),
        );
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

        let affected = importers_of(
            &modules_under(dir.path()),
            &BTreeSet::from(["a.erg".to_owned()]),
        );
        assert_eq!(affected.len(), 2);
    }

    // ─── The closure, without a disk ────────────────────────────────────────
    //
    // These need no temp directory, which is the point of `importers_of` taking
    // the graph: what it computes is a reachability question, and a question
    // about a graph is answered fastest by being asked about a graph.

    /// Builds an import graph without touching a filesystem.
    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(module, imports)| {
                (
                    (*module).to_owned(),
                    imports.iter().map(|i| (*i).to_owned()).collect(),
                )
            })
            .collect()
    }

    /// **A chain propagates in one direction only.** `top` imports `middle`
    /// imports `base`: editing `base` affects both, and editing `top` affects
    /// nobody. The old shape passed this too, but only by re-reading three
    /// files three times to find out.
    #[test]
    fn the_closure_follows_imports_upward_and_not_down() {
        let imports = graph(&[
            ("base.erg", &[]),
            ("middle.erg", &["base.erg"]),
            ("top.erg", &["middle.erg"]),
        ]);

        let from_base = importers_of(&imports, &BTreeSet::from(["base.erg".to_owned()]));
        assert_eq!(from_base.len(), 3, "{from_base:?}");

        let from_top = importers_of(&imports, &BTreeSet::from(["top.erg".to_owned()]));
        assert_eq!(from_top, BTreeSet::from(["top.erg".to_owned()]));
    }

    /// A diamond delivers each importer once, not once per path that reaches
    /// it — recompiling a module twice is wasted work, and queuing it twice
    /// would make the second reload undo the first one's field values.
    #[test]
    fn a_module_reached_by_two_paths_appears_once() {
        let imports = graph(&[
            ("base.erg", &[]),
            ("left.erg", &["base.erg"]),
            ("right.erg", &["base.erg"]),
            ("top.erg", &["left.erg", "right.erg"]),
        ]);

        let affected = importers_of(&imports, &BTreeSet::from(["base.erg".to_owned()]));
        assert_eq!(affected.len(), 4, "{affected:?}");
    }

    /// An import naming a module that is not there — deleted, or mistyped —
    /// leaves the graph alone. The compiler is where that gets its diagnostic;
    /// the closure's job is not to have an opinion about it.
    #[test]
    fn an_import_of_a_missing_module_affects_nothing() {
        let imports = graph(&[("guard.erg", &["gone.erg"])]);

        let affected = importers_of(&imports, &BTreeSet::from(["other.erg".to_owned()]));
        assert_eq!(affected, BTreeSet::from(["other.erg".to_owned()]));
    }

    /// Several files saved at once is one closure, not one per file.
    #[test]
    fn two_changed_modules_bring_both_their_importers() {
        let imports = graph(&[
            ("a.erg", &[]),
            ("b.erg", &[]),
            ("uses_a.erg", &["a.erg"]),
            ("uses_b.erg", &["b.erg"]),
            ("uses_neither.erg", &[]),
        ]);

        let affected = importers_of(
            &imports,
            &BTreeSet::from(["a.erg".to_owned(), "b.erg".to_owned()]),
        );

        assert_eq!(affected.len(), 4, "{affected:?}");
        assert!(!affected.contains("uses_neither.erg"));
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;

    /// **Without this the first frame has no programs.** The pump reacts to
    /// changes, so a game would sit inert until its author happened to save.
    #[test]
    fn every_module_under_the_root_is_compiled_at_startup() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(dir.path().join("ai")).expect("makes a subdir");
        std::fs::write(
            dir.path().join("ai/guard.erg"),
            "behavior Guard { int health = 100; }",
        )
        .expect("writes");
        std::fs::write(
            dir.path().join("rules.erg"),
            "fn int Double(int n) { return n * 2; }",
        )
        .expect("writes");

        let pending = reload_channel();
        assert_eq!(load_all(dir.path(), &pending), 2);

        let queued: Vec<String> = pending.drain().into_iter().map(|r| r.module).collect();
        assert!(queued.contains(&"ai/guard.erg".to_owned()), "{queued:?}");
        assert!(queued.contains(&"rules.erg".to_owned()), "{queued:?}");
    }

    /// A module written with forward slashes is how an `import` names it, so
    /// that is how it has to be queued — on Windows too.
    #[test]
    fn a_nested_module_is_named_with_forward_slashes() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(dir.path().join("combat/melee")).expect("makes subdirs");
        std::fs::write(
            dir.path().join("combat/melee/sword.erg"),
            "fn void Swing() { }",
        )
        .expect("writes");

        let pending = reload_channel();
        load_all(dir.path(), &pending);

        assert_eq!(
            pending.drain().first().map(|r| r.module.clone()),
            Some("combat/melee/sword.erg".to_owned())
        );
    }

    /// One broken script must not stop the others: a project mid-edit usually
    /// has one, and refusing to load anything would make the engine unusable
    /// exactly when it is being worked on.
    #[test]
    fn a_module_that_does_not_compile_is_skipped_not_fatal() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("good.erg"), "fn void A() { }").expect("writes");
        std::fs::write(dir.path().join("broken.erg"), "behavior {{{").expect("writes");

        let pending = reload_channel();
        assert_eq!(load_all(dir.path(), &pending), 1);
        assert_eq!(
            pending.drain().first().map(|r| r.module.clone()),
            Some("good.erg".to_owned())
        );
    }

    #[test]
    fn an_empty_root_loads_nothing_and_does_not_fail() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let pending = reload_channel();

        assert_eq!(load_all(dir.path(), &pending), 0);
        assert!(pending.is_empty());
    }
}
