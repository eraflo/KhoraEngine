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
//! A `PreExtract` data system that reads the engine's [`AssetWatch`] and files a
//! [`CompileRequest`] per root for every `.erg` file that changed. The compile
//! family compiles it — **and every file that imports it** — off the frame
//! (`crate::script::project_compiler`), and what it hands on lands at a later
//! frame's boundary.
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

use khora_data::ecs::SystemContext;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::event::{Channel, WhenFull};
use khora_core::lane::OutputDeck;
use khora_core::problems::Problems;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
use khora_script::toolchain::CompileRequest;

use khora_script::modules::imports_of;

use crate::asset::{AssetChangeKind, AssetWatch};

use crate::script::layout::{
    exists_exactly, is_file_exactly, is_module_file, is_script_file, module_of, SCRIPT_DIR,
};
use crate::script::mounted::MountedScripts;

/// How many requests may wait for the `loading` family. Requests of one root
/// replace a waiting one they cover, so repeated saves never fill this; a mass
/// change (a branch switch) files one request per root a run.
const REQUEST_BACKLOG: usize = 1024;

/// The channel the pump files its requests on, for the `loading` family to take.
pub fn request_channel() -> Channel<CompileRequest> {
    Channel::bounded(REQUEST_BACKLOG, WhenFull::DropOldest)
}

/// Whether a change at `rel_path` under `root` can change which Ergon files
/// are misplaced there: an Ergon file's, a folder's that `arrived`, or one
/// that is gone. A folder moved or deleted carries its files with it and
/// reports only itself, and once it is gone nothing tells it from a file —
/// `props.v2/` is named like one — so whatever is no longer there is taken as
/// possibly one. A folder merely modified is what a save inside it reports,
/// and moves nothing.
fn may_move_a_script(root: &Path, rel_path: &str, arrived: bool) -> bool {
    is_script_file(rel_path)
        || !exists_exactly(root, rel_path)
        || (arrived && root.join(rel_path).is_dir())
}

/// What the modules inside the folder at `rel_path` (under the assets root)
/// start with, when it is the script folder or a folder in it: nothing for
/// the script folder itself, whose modules go by their own names; `ai/` for
/// `scripts/ai`. `None` for any other folder.
fn module_prefix_of_folder(rel_path: &str) -> Option<String> {
    match rel_path.strip_prefix(SCRIPT_DIR)? {
        "" => Some(String::new()),
        rest => rest
            .strip_prefix('/')
            .filter(|inside| !inside.is_empty())
            .map(|inside| format!("{inside}/")),
    }
}

/// The recorded modules of `root` a folder at `rel_path` (under it) held, when
/// that folder is the script folder or one inside it and is gone now: each
/// whose file is no longer there. A folder deleted, or moved out, reports
/// only itself — its modules are known from the record alone.
fn modules_gone_with(mounted: Option<&MountedScripts>, root: &Path, rel_path: &str) -> Vec<String> {
    let Some(mounted) = mounted else {
        return Vec::new();
    };
    let Some(prefix) = module_prefix_of_folder(rel_path) else {
        return Vec::new();
    };
    if exists_exactly(root, rel_path) {
        return Vec::new();
    }
    mounted
        .modules_of(root)
        .into_iter()
        .filter(|module| module.starts_with(&prefix) && !is_on_disk(root, module))
        .collect()
}

/// Whether `module`'s file is under `root`'s script folder, spelled as the
/// module goes by — a file renamed to another case is gone under its old
/// name, whatever the file system thinks of case.
pub(crate) fn is_on_disk(root: &Path, module: &str) -> bool {
    is_file_exactly(root, &format!("{SCRIPT_DIR}/{module}"))
}

/// Another mounted root whose script folder holds `module`, when `root`'s no
/// longer does. Modules are known by name across roots, so one gone from a
/// root another still provides is that root's now — as an unmount keeps it.
fn other_provider(mounted: Option<&MountedScripts>, root: &Path, module: &str) -> Option<PathBuf> {
    let here = crate::asset::root_path::absolute(root);
    mounted?
        .roots()
        .into_iter()
        .find(|other| *other != here && is_on_disk(other, module))
}

/// The modules a folder at `rel_path` (under the assets root) holds, when it
/// is a folder now under the script folder: a folder moved in reports only
/// itself, and its scripts must compile as surely as a file saved there.
fn modules_of_folder(root: &Path, rel_path: &str) -> Vec<String> {
    let Some(prefix) = module_prefix_of_folder(rel_path) else {
        return Vec::new();
    };
    let folder = root.join(rel_path);
    if !folder.is_dir() {
        return Vec::new();
    }
    module_names(&folder)
        .into_iter()
        .map(|module| format!("{prefix}{module}"))
        .collect()
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
pub(crate) fn importers_of(
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

/// The modules under `script_root`, by the path they are known by.
pub fn module_names(script_root: &Path) -> Vec<String> {
    modules_under(script_root).into_keys().collect()
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
pub(crate) fn modules_under(root: &Path) -> BTreeMap<String, Vec<String>> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            // Forward slashes, because that is how an `import` writes a path
            // and how a module is named everywhere else.
            let module = relative.to_string_lossy().replace('\\', "/");
            if !is_module_file(&module) {
                return None;
            }
            let imports = std::fs::read_to_string(entry.path())
                .map(|source| {
                    // As the resolver reads them: `./helper.erg` is `helper.erg`.
                    imports_of(&source)
                        .into_iter()
                        .filter_map(|import| khora_script::modules::normalise(&import).ok())
                        .collect()
                })
                .unwrap_or_default();
            Some((module, imports))
        })
        .collect()
}

fn script_hot_reload_system(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(watch) = runtime.resources.get::<Arc<AssetWatch>>() else {
        return; // No assets dir → no hot-reload, which is the shipping path.
    };
    let events = watch.poll_for("script_hot_reload");
    if events.is_empty() {
        return;
    }
    let requests = runtime.resources.get::<Channel<CompileRequest>>();
    let mounted = runtime
        .resources
        .get::<Arc<MountedScripts>>()
        .map(Arc::as_ref);

    // Per watched root: each holds its scripts in its own script folder, and
    // an import never crosses from one into another. A change outside that
    // folder compiles nothing — it may only add, move or take away an `.erg`
    // that is not a script, which the root's list of them then shows.
    let mut by_root: BTreeMap<std::path::PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut rescan: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    for event in &events {
        // A folder that arrives brings its scripts; one merely modified —
        // which is what a save inside it also reports — brings nothing new.
        let arrived = event.kind == AssetChangeKind::Created;
        let changed = match module_of(&event.rel_path) {
            Some(module) => vec![module.to_owned()],
            None if arrived => modules_of_folder(&event.root, &event.rel_path),
            // A folder gone from the script folder — deleted, or moved out —
            // reports only itself: what it held is known from the record.
            None => modules_gone_with(mounted, &event.root, &event.rel_path),
        };
        if !changed.is_empty() {
            by_root
                .entry(event.root.to_path_buf())
                .or_default()
                .extend(changed);
        }
        if module_of(&event.rel_path).is_none()
            && may_move_a_script(&event.root, &event.rel_path, arrived)
        {
            rescan.insert(event.root.to_path_buf());
        }
    }
    // A module gone from one root that another mounted root still provides
    // is that root's change: its copy compiles in, rather than the module
    // being withdrawn from under it.
    let mut by_provider: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (root, changed) in by_root {
        for module in changed {
            let provider = (!is_on_disk(&root, &module))
                .then(|| other_provider(mounted, &root, &module))
                .flatten()
                .unwrap_or_else(|| root.clone());
            by_provider.entry(provider).or_default().insert(module);
        }
    }
    // Filed, never compiled here: the `loading` family compiles off the frame,
    // and what it hands on lands at a later frame's boundary.
    if let Some(requests) = requests {
        for (root, modules) in by_provider {
            requests.send(CompileRequest { root, modules });
        }
    }
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
        for root in rescan {
            crate::script::problems::report_misplaced(problems, &root);
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
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}

// What the pump files is pinned in `tests/it/script_pump_files_requests.rs`,
// what a compile of it hands on in `tests/it/project_compiler.rs`; which file
// is which module is `crate::script::layout`'s answer, and is pinned there.
