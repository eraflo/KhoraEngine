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
//! Pointing the engine at a project's scripts.
//!
//! The queues live in the engine for every application ([`install`]). What
//! only the application knows is **where the scripts are**: a game watches the
//! `assets/` beside its executable, the editor watches whichever project is
//! open — and changes it while running — and a packed runtime watches nothing
//! at all. So mounting and unmounting take the runtime shared, at any frame.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::script::{ProblemSource, ScriptCatalog, ScriptProblems};
use khora_core::Runtime;
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::{
    declaration_channel, DeclaredComponents, PendingDeclarations, ScriptDeclarations,
};
use khora_io::script::hot_reload::{
    load_all, load_all_announcing, module_names, reload_channel, PendingReloads,
};

/// The module the engine serves its components as: a script imports it, and
/// no project holds it as a file.
pub const ENGINE_MODULE: &str = khora_io::script::mirror::MIRROR_MODULE;

/// Where the scripts are found under an assets root.
const SCRIPT_DIR: &str = "scripts";

/// The modules each mounted root brought in, by root: what an unmount takes
/// back.
#[derive(Debug, Default)]
pub struct MountedScripts(Mutex<BTreeMap<PathBuf, BTreeSet<String>>>);

impl MountedScripts {
    fn record(&self, root: &Path, modules: impl IntoIterator<Item = String>) {
        let mut mounted = self.lock();
        mounted
            .entry(root.to_path_buf())
            .or_default()
            .extend(modules);
    }

    fn forget(&self, root: &Path) -> BTreeSet<String> {
        self.lock().remove(root).unwrap_or_default()
    }

    /// The modules the roots still mounted bring in — recorded at their mount,
    /// and on disk now.
    fn others(&self) -> BTreeSet<String> {
        self.lock()
            .iter()
            .flat_map(|(root, recorded)| {
                recorded
                    .iter()
                    .cloned()
                    .chain(module_names(&root.join(SCRIPT_DIR)))
            })
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<PathBuf, BTreeSet<String>>> {
        match self.0.lock() {
            Ok(mounted) => mounted,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Puts what scripting needs into `runtime`, each only where it is absent:
/// the engine's [`AssetWatch`], the queues of compiled programs and of
/// declarations, the engine's event channel, the memory of who declares
/// what, the [`ScriptCatalog`], and the record of what is mounted.
///
/// For every application, by the engine — before the application's own
/// bootstrap runs, so it may mount there, and again at the bootstrap, which
/// finds them in place. An empty queue costs nothing; a missing one makes a
/// subsystem that looks wired and is not.
pub fn install(runtime: &mut Runtime) {
    let resources = &mut runtime.resources;
    if resources.get::<Arc<AssetWatch>>().is_none() {
        resources.insert(Arc::new(AssetWatch::default()));
    }
    if resources.get::<PendingReloads>().is_none() {
        resources.insert(reload_channel());
    }
    if resources
        .get::<khora_core::event::Channel<khora_core::script::ScriptEvent>>()
        .is_none()
    {
        resources.insert(khora_core::script::engine_event_channel());
    }
    if resources.get::<PendingDeclarations>().is_none() {
        resources.insert(declaration_channel());
    }
    if resources.get::<Arc<Mutex<DeclaredComponents>>>().is_none() {
        resources.insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    }
    if resources.get::<Arc<ScriptCatalog>>().is_none() {
        resources.insert(Arc::new(ScriptCatalog::default()));
    }
    if resources.get::<Arc<ScriptProblems>>().is_none() {
        resources.insert(Arc::new(ScriptProblems::default()));
    }
    if resources.get::<Arc<MountedScripts>>().is_none() {
        resources.insert(Arc::new(MountedScripts::default()));
    }
}

/// Mounts the scripts under `assets_root`, at any frame: arms the root on the
/// engine's `AssetWatch`, compiles every module under `assets_root/scripts`,
/// sends what they declare and what they compiled to, and announces what each
/// reads to the `ScriptCatalog`.
///
/// Returns how many modules compiled. Safe to call with a directory that does
/// not exist — a project without scripts is not an error, and neither is a
/// packed build with no source tree.
pub fn mount(runtime: &Runtime, assets_root: &Path) -> usize {
    if !assets_root.is_dir() {
        log::info!(
            "scripts: {} does not exist — nothing to mount",
            assets_root.display()
        );
        return 0;
    }
    // Watched whether or not it holds scripts yet: the first one saved under
    // it is then compiled as it lands.
    match runtime.resources.get::<Arc<AssetWatch>>() {
        Some(watch) => {
            if let Err(error) = watch.watch(assets_root) {
                log::warn!(
                    "scripts: {} is not watched, edits take a restart: {error:#}",
                    assets_root.display()
                );
            }
        }
        None => log::warn!(
            "scripts: the engine holds no AssetWatch — {} is not watched",
            assets_root.display()
        ),
    }

    let script_dir = assets_root.join(SCRIPT_DIR);
    if !script_dir.is_dir() {
        return 0;
    }
    let (Some(pending), Some(declarations)) = (
        runtime.resources.get::<PendingReloads>(),
        runtime.resources.get::<PendingDeclarations>(),
    ) else {
        log::error!(
            "scripts: the engine's script queues are missing — `scripts::install` was not run, so {} cannot load",
            script_dir.display()
        );
        return 0;
    };
    if let Some(mounted) = runtime.resources.get::<Arc<MountedScripts>>() {
        mounted.record(assets_root, module_names(&script_dir));
    }
    let loaded = match runtime.resources.get::<Arc<ScriptCatalog>>() {
        Some(catalog) => load_all_announcing(
            &script_dir,
            pending,
            declarations,
            catalog,
            runtime
                .resources
                .get::<Arc<ScriptProblems>>()
                .map(Arc::as_ref),
        ),
        None => load_all(&script_dir, pending, declarations),
    };
    log::info!(
        "scripts: {loaded} module(s) loaded from {}",
        script_dir.display()
    );
    loaded
}

/// Unmounts the scripts under `assets_root`, at any frame: disarms the root,
/// orphans what its modules declared — kept with its data, as for a deleted
/// file — withdraws the programs they sent that no lane installed yet, and
/// forgets what they announced they read.
pub fn unmount(runtime: &Runtime, assets_root: &Path) {
    if let Some(watch) = runtime.resources.get::<Arc<AssetWatch>>() {
        watch.unwatch(assets_root);
    }
    // What was mounted, and what is there now: a module saved since the
    // mount is the root's too.
    let mounted = runtime.resources.get::<Arc<MountedScripts>>();
    let mut modules = mounted
        .map(|mounted| mounted.forget(assets_root))
        .unwrap_or_default();
    modules.extend(module_names(&assets_root.join(SCRIPT_DIR)));
    // A module is known by its path under its script root: one another root
    // still mounted brings in by the same name is that root's now, and stays.
    if let Some(mounted) = mounted {
        let kept = mounted.others();
        modules.retain(|module| !kept.contains(module));
    }
    if modules.is_empty() {
        return;
    }
    // Closed with the project: what they declared is orphaned quietly.
    if let Some(declared) = runtime.resources.get::<Arc<Mutex<DeclaredComponents>>>() {
        declared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unmounting(modules.iter().cloned());
    }
    if let Some(declarations) = runtime.resources.get::<PendingDeclarations>() {
        for module in &modules {
            declarations.send(ScriptDeclarations {
                module: module.clone(),
                components: Vec::new(),
            });
        }
    }
    if let Some(pending) = runtime.resources.get::<PendingReloads>() {
        pending.retain(|reload| !modules.contains(&reload.module));
    }
    if let Some(catalog) = runtime.resources.get::<Arc<ScriptCatalog>>() {
        for module in &modules {
            catalog.settle(module);
        }
    }
    // A module no longer mounted has nothing wrong with it to show.
    if let Some(problems) = runtime.resources.get::<Arc<ScriptProblems>>() {
        for module in &modules {
            problems.report(
                ProblemSource::Compile {
                    module: module.clone(),
                },
                Vec::new(),
            );
        }
    }
    log::info!(
        "scripts: {} module(s) unmounted from {}",
        modules.len(),
        assets_root.display()
    );
}
