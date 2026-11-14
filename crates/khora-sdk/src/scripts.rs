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
//! Pointing the engine at a project's scripts, whose place only the application
//! knows: [`mount`] and [`unmount`] take the runtime shared, at any frame.

use khora_io::systems::shader_hot_reload::OverlaysSeen;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::lane::Reserve;
use khora_core::problems::{ProblemSource, Problems};
use khora_core::projection::Interest;
use khora_core::script::{ScriptCatalog, ScriptReads};
use khora_core::Runtime;
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::{DeclaredComponents, ScriptDeclarations};
use khora_io::script::hot_reload::{module_names, request_channel};
use khora_io::script::layout::script_dir;
use khora_io::script::project_compiler::{
    CompileRequest, ProjectCompiler, Recompiled, ScriptCompiler, ScriptReload,
};

/// The module the engine serves its components as: a script imports it, and
/// no project holds it as a file.
pub const ENGINE_MODULE: &str = khora_io::script::mirror::MIRROR_MODULE;

/// Returns whether `name` is the engine's, so no script may declare a type by it.
///
/// The engine's names are its components' (current, former or retired), its
/// asset kinds' (`Prefab`, `Sound`, …) and the events it raises (`Touched`, …),
/// which every module has in scope. The match is exact, as Ergon names are.
pub fn is_engine_name(name: &str) -> bool {
    khora_io::script::mirror::engine_component_names()
        .binary_search_by(|known| known.as_str().cmp(name))
        .is_ok()
        || khora_core::asset::AssetKind::from_ergon_name(name).is_some()
        || khora_core::script::ENGINE_EVENTS
            .iter()
            .any(|event| event.name == name)
}

pub use khora_io::script::compile_hub::{CompileHub, CompileTicket, CompileTicketOutcome};
pub use khora_io::script::layout::{is_script_file, module_of, SCRIPT_DIR};
pub use khora_io::script::mounted::MountedScripts;

/// Puts what scripting needs into `runtime`, each resource only where it is absent.
///
/// The engine runs it for every application, before the application's
/// bootstrap, which may then mount, and again at boot. An empty queue costs
/// nothing; a missing one would leave scripting wired in name only. It also
/// installs the engine's [`Problems`] list, which other sources report to.
pub fn install(runtime: &mut Runtime) {
    let resources = &mut runtime.resources;
    if resources.get::<Arc<AssetWatch>>().is_none() {
        resources.insert(Arc::new(AssetWatch::default()));
    }
    // What the shader pump last applied the overlays of the watched roots
    // for: a project opened, closed, trusted or restricted applies them
    // afresh.
    if resources.get::<Arc<OverlaysSeen>>().is_none() {
        resources.insert(Arc::new(OverlaysSeen::default()));
    }
    // What the background staged waits here for a frame's boundary: the
    // programs and declarations compiled off the frame among it.
    if resources.get::<Reserve>().is_none() {
        resources.insert(Reserve::new());
    }
    if resources
        .get::<khora_core::event::Channel<CompileRequest>>()
        .is_none()
    {
        resources.insert(request_channel());
    }
    // Where a mount files its compile and reads its outcome, on the channel
    // the `loading` family takes from.
    if resources.get::<Arc<CompileHub>>().is_none() {
        let requests = resources
            .get::<khora_core::event::Channel<CompileRequest>>()
            .cloned()
            .unwrap_or_else(request_channel);
        resources.insert(Arc::new(CompileHub::new(requests)));
    }
    if resources
        .get::<khora_core::event::Channel<khora_core::script::ScriptEvent>>()
        .is_none()
    {
        resources.insert(khora_core::script::engine_event_channel());
    }
    if resources.get::<Arc<Mutex<DeclaredComponents>>>().is_none() {
        resources.insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    }
    if resources.get::<Arc<ScriptCatalog>>().is_none() {
        resources.insert(Arc::new(ScriptCatalog::default()));
    }
    if resources.get::<Arc<Interest<ScriptReads>>>().is_none() {
        resources.insert(Arc::new(Interest::<ScriptReads>::new()));
    }
    if resources.get::<Arc<Problems>>().is_none() {
        resources.insert(Arc::new(Problems::default()));
    }
    if resources.get::<Arc<MountedScripts>>().is_none() {
        resources.insert(Arc::new(MountedScripts::default()));
    }
    // The `loading` family's compiler, reporting to what is installed above.
    if resources.get::<Arc<dyn ScriptCompiler>>().is_none() {
        let compiler: Arc<dyn ScriptCompiler> = Arc::new(ProjectCompiler::from_runtime(runtime));
        runtime.resources.insert(compiler);
    }
}

/// Mounts the scripts under `assets_root`, at any frame, and returns the ticket of their compile.
///
/// The root is watched, and each `.erg` outside its [`SCRIPT_DIR`] is reported
/// to the `Problems`, never compiled. The compile lands at a later frame's
/// boundary; read it with [`outcome`]. One filed before the boot is compiled
/// before the application's `setup`.
///
/// Returns `None` when there is nothing to compile: no such directory, no
/// script folder, or scripting not installed. A project without scripts is
/// not an error.
pub fn mount(runtime: &Runtime, assets_root: &Path) -> Option<CompileTicket> {
    if !assets_root.is_dir() {
        log::info!(
            "scripts: {} does not exist — nothing to mount",
            assets_root.display()
        );
        return None;
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

    // Misplaced scripts are reported first, script folder or not: a project
    // whose only script sits in the wrong folder most needs telling. Under the
    // watch's name for the root, so the pump's rescans replace this list.
    let named = watched_name(runtime, assets_root);
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
        khora_io::script::problems::report_misplaced(problems, &named);
    }

    let script_dir = script_dir(assets_root)?;
    if runtime.resources.get::<Reserve>().is_none() {
        log::error!(
            "scripts: the engine holds no reserve — `scripts::install` was not run, so {} cannot load",
            script_dir.display()
        );
        return None;
    }
    let Some(hub) = runtime.resources.get::<Arc<CompileHub>>() else {
        log::error!(
            "scripts: the engine holds no compile hub — `scripts::install` was not run, so {} cannot load",
            script_dir.display()
        );
        return None;
    };
    // Under the root's name as the pump hears of it, so a folder later taken
    // out of the script folder is known to have held these — and recorded
    // now, before the compile, which compiles only a mounted root.
    let modules = module_names(&script_dir);
    if let Some(mounted) = runtime.resources.get::<Arc<MountedScripts>>() {
        mounted.record(&named, modules.clone());
    }
    let ticket = hub.file(named, modules);
    log::info!(
        "scripts: {} filed to be compiled (ticket {})",
        script_dir.display(),
        ticket.0
    );
    Some(ticket)
}

/// Takes the outcome of the mount filed as `ticket`: how many modules compiled, or why not.
///
/// `None` while it is in flight, and once taken.
pub fn outcome(runtime: &Runtime, ticket: CompileTicket) -> Option<CompileTicketOutcome> {
    runtime
        .resources
        .get::<Arc<CompileHub>>()?
        .take_outcome(ticket)
}

/// Stages `compiled` from the host in `reserve`: it lands at a frame start
/// once every compile under way now has ended, for the boundary to apply.
fn stage(reserve: &Reserve, compiled: Recompiled) {
    let mut staging = khora_core::lane::Staging::new();
    staging.stage(compiled.declarations);
    staging.stage(compiled.reloads);
    reserve.stage_from_host(staging);
}

/// Returns the name the engine's watch knows `assets_root` by, or `assets_root` when unwatched.
///
/// That name is the root as first named, whichever spelling is given now.
fn watched_name(runtime: &Runtime, assets_root: &Path) -> PathBuf {
    runtime
        .resources
        .get::<Arc<AssetWatch>>()
        .and_then(|watch| watch.named(assets_root))
        .unwrap_or_else(|| assets_root.to_path_buf())
}

/// Unmounts the scripts under `assets_root`, at any frame, so none of them runs again.
///
/// The root is no longer watched; what its modules declared is orphaned, kept
/// with its data as for a deleted file; their programs, queued or installed,
/// are withdrawn, and what they read is forgotten.
pub fn unmount(runtime: &Runtime, assets_root: &Path) {
    // Read before the watch forgets it: the name the mount reported under.
    let named = watched_name(runtime, assets_root);
    if let Some(watch) = runtime.resources.get::<Arc<AssetWatch>>() {
        watch.unwatch(assets_root);
    }
    // What was mounted, and what is there now: a module saved since the
    // mount is the root's too.
    let mounted = runtime.resources.get::<Arc<MountedScripts>>();
    let mut modules = mounted
        .map(|mounted| {
            let mut modules = mounted.forget(&named);
            modules.extend(mounted.forget(assets_root));
            modules
        })
        .unwrap_or_default();
    if let Some(dir) = script_dir(assets_root) {
        modules.extend(module_names(&dir));
    }
    // The files it held outside its script folder are no longer the
    // project's to show.
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
        khora_io::script::problems::clear_misplaced(problems, &named);
        if named != assets_root {
            khora_io::script::problems::clear_misplaced(problems, assets_root);
        }
    }
    // A mount waiting on this root lands as withdrawn — whatever the root
    // brought in, an empty script folder or only modules another root
    // provides: nothing of it will land.
    if let Some(hub) = runtime.resources.get::<Arc<CompileHub>>() {
        hub.withdraw(&named);
        hub.withdraw(assets_root);
    }
    // A compile the pump filed for this root before it closed is dropped;
    // and what one already under way hands on is taken in ahead of what this
    // stages below, however late it ends.
    if let Some(requests) = runtime
        .resources
        .get::<khora_core::event::Channel<CompileRequest>>()
    {
        requests.retain(|request| request.root != named && request.root != assets_root);
    }
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
    // Each module declares nothing and runs nothing any more. A withdrawal
    // supersedes a reload still waiting for the module, and takes an
    // installed program out of the runtime: a closed project's scripts stop,
    // whether or not the game ran them yet.
    if let Some(reserve) = runtime.resources.get::<Reserve>() {
        stage(
            reserve,
            Recompiled {
                declarations: modules
                    .iter()
                    .map(|module| ScriptDeclarations {
                        module: module.clone(),
                        components: Vec::new(),
                    })
                    .collect(),
                reloads: modules
                    .iter()
                    .map(|module| ScriptReload {
                        module: module.clone(),
                        program: None,
                    })
                    .collect(),
            },
        );
    }
    if let Some(reads) = runtime.resources.get::<Arc<Interest<ScriptReads>>>() {
        for module in &modules {
            reads.withdraw(&ScriptReads::incoming(module));
        }
    }
    // A module no longer mounted has nothing wrong with it to show.
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
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
