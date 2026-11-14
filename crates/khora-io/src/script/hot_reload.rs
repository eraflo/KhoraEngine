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

//! The `.erg` hot-reload pump: files a compile, per root, of every changed
//! module and every module importing it, compiled off the frame.
//!
//! Imports are read from the files with [`imports_of`], never from the asset
//! index: an `.erg` edited in place is not reindexed, so the index would miss a
//! new `import`. A save that does not compile replaces nothing.

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

/// How many requests may wait for the `loading` family.
///
/// Requests of one root replace a waiting one they cover, so repeated saves
/// never fill this; a mass change (a branch switch) files one per root a run.
const REQUEST_BACKLOG: usize = 1024;

/// Returns the channel the pump files its requests on, for the `loading` family.
pub fn request_channel() -> Channel<CompileRequest> {
    Channel::bounded(REQUEST_BACKLOG, WhenFull::Reject)
}

/// Whether a change at `rel_path` under `root` can change which Ergon files are misplaced.
///
/// True for an Ergon file, a folder that `arrived`, or anything gone: a gone
/// folder reports only itself and cannot be told from a file (`props.v2/`).
/// A folder merely modified is what a save inside it reports, and moves nothing.
fn may_move_a_script(root: &Path, rel_path: &str, arrived: bool) -> bool {
    is_script_file(rel_path)
        || !exists_exactly(root, rel_path)
        || (arrived && root.join(rel_path).is_dir())
}

/// Returns the prefix of the modules in the folder at `rel_path`, under the assets root.
///
/// Empty for the script folder itself, `ai/` for `scripts/ai`; `None` for a
/// folder outside the script folder.
fn module_prefix_of_folder(rel_path: &str) -> Option<String> {
    match rel_path.strip_prefix(SCRIPT_DIR)? {
        "" => Some(String::new()),
        rest => rest
            .strip_prefix('/')
            .filter(|inside| !inside.is_empty())
            .map(|inside| format!("{inside}/")),
    }
}

/// Returns the recorded modules of `root` that a gone folder at `rel_path` held.
///
/// A folder deleted or moved out reports only itself, so its modules are known
/// from the record alone. Empty unless the folder is in the script folder.
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

/// Whether `module`'s file is under `root`'s script folder, spelled exactly as the module.
///
/// A file renamed to another case is gone under its old name, whatever the
/// file system thinks of case.
pub(crate) fn is_on_disk(root: &Path, module: &str) -> bool {
    is_file_exactly(root, &format!("{SCRIPT_DIR}/{module}"))
}

/// Returns another mounted root whose script folder holds `module`, gone from `root`'s.
///
/// Modules are known by name across roots: one another root still provides
/// is that root's, as an unmount keeps it.
fn other_provider(mounted: Option<&MountedScripts>, root: &Path, module: &str) -> Option<PathBuf> {
    let here = crate::asset::root_path::absolute(root);
    mounted?
        .roots()
        .into_iter()
        .find(|other| *other != here && is_on_disk(other, module))
}

/// Returns the modules the folder at `rel_path` holds, when it is in the script folder.
///
/// A folder moved in reports only itself, and its scripts must compile as
/// surely as a file saved there.
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

/// Returns every module in `imports` that reaches one of `changed`, transitively.
///
/// `changed` is included. A cyclic import terminates here; the compiler
/// refuses it later.
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

/// Returns the modules under `script_root`, by the path they are known by.
pub fn module_names(script_root: &Path) -> Vec<String> {
    modules_under(script_root, None, crate::trust::Trust::Project)
        .into_keys()
        .collect()
}

/// Returns every `.erg` module under `root`, each with what it imports.
///
/// Files are read through a guard at `trust`, reporting to `refusals` (in
/// memory only without). A file that cannot be read is listed with no
/// imports: it still compiles, and the compiler gives its diagnostic.
pub(crate) fn modules_under(
    root: &Path,
    refusals: Option<&Arc<crate::trust::Refusals>>,
    trust: crate::trust::Trust,
) -> BTreeMap<String, Vec<String>> {
    // A module over its cap is read as importing nothing; the compile refuses it.
    let refusals = refusals
        .cloned()
        .unwrap_or_else(|| Arc::new(crate::trust::Refusals::new(None, None)));
    let guard = crate::trust::FileGuard::new(root, trust, refusals);
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
            let imports = guard
                .read_to_string(relative)
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

    // Per watched root: an import never crosses roots. A change outside the
    // script folder compiles nothing; it may only change which `.erg` files
    // are misplaced, which the root's list then shows.
    let mut by_root: BTreeMap<std::path::PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut rescan: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    for event in &events {
        // Too many changes came at once and some were dropped: every script
        // of the root compiles again, and its list of them is read again.
        if event.kind == AssetChangeKind::Rescan {
            // What is on disk compiles again; what was mounted and is gone —
            // its removal maybe among the changes dropped — is withdrawn.
            let gone: Vec<String> = mounted
                .map(|mounted| mounted.modules_of(&event.root))
                .unwrap_or_default()
                .into_iter()
                .filter(|module| !is_on_disk(&event.root, module))
                .collect();
            by_root.entry(event.root.to_path_buf()).or_default().extend(
                modules_of_folder(&event.root, SCRIPT_DIR)
                    .into_iter()
                    .chain(gone),
            );
            rescan.insert(event.root.to_path_buf());
            continue;
        }
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
    // Filed, never compiled here: the `loading` family compiles off the frame.
    if let Some(requests) = requests {
        for (root, modules) in by_provider {
            let refused = requests.dropped();
            requests.send(CompileRequest::new(root.clone(), modules));
            // A full backlog refuses the newest request rather than drop a
            // waiting one — a mount's, which a host waits on.
            if requests.dropped() > refused {
                log::error!(
                    "scripts of {}: {REQUEST_BACKLOG} compiles already wait; this change was refused",
                    root.display()
                );
            }
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

// Pinned by `tests/it/script_pump_files_requests.rs` and `tests/it/project_compiler.rs`.
