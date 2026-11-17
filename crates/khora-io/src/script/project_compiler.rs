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

//! A project's scripts, compiled from disk or from a game's pack for the
//! `loading` family, one [`CompileRequest`] at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::lane::Lease;
use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptReads};
use khora_core::Runtime;
use khora_script::compile_module;
use khora_script::modules::SourceLoader;
use khora_script::vm::Program;

use crate::asset::AssetReader;
use crate::script::compile::DiskLoader;
use crate::script::declarations::ScriptDeclarations;
use crate::script::hot_reload::{importers_of, is_on_disk, modules_under};
use crate::script::mirror::PreludeLoader;
use crate::script::mounted::MountedScripts;
use crate::script::sources::{pack_modules, DiskModules, ModuleSource, PackModules, SourcesOf};
use crate::trust::{FileGuard, ProjectTrustState, Refusals, Trust};

// What a compile is asked and hands on, named here so the engine's wiring
// reaches them through the compiler it installs, without a dependency on
// `khora-script` of its own.
pub use khora_script::reload::ScriptReload;
pub use khora_script::toolchain::{CompileRequest, Recompiled, ScriptCompiler};

/// The engine's [`ScriptCompiler`]: a project's scripts from disk, or a game's pack, with
/// the prelude.
///
/// Reports to the problems, the script flow's demand and the mounted roots
/// of the runtime it was built from, those it holds.
#[derive(Debug, Default)]
pub struct ProjectCompiler {
    problems: Option<Arc<Problems>>,
    reads: Option<Arc<Interest<ScriptReads>>>,
    mounted: Option<Arc<MountedScripts>>,
    refusals: Option<Arc<Refusals>>,
    trust_state: Option<Arc<ProjectTrustState>>,
    /// The game's pack, when the modules are read from it rather than from disk.
    pack: Option<PackScripts>,
}

impl ProjectCompiler {
    /// Creates a compiler reporting to what `runtime` holds.
    ///
    /// Its problems, script reads, mounted scripts, refusals and project trust;
    /// `ProjectCompiler::default()` reports to none.
    pub fn from_runtime(runtime: &Runtime) -> Self {
        Self {
            problems: runtime.resources.get::<Arc<Problems>>().cloned(),
            reads: runtime
                .resources
                .get::<Arc<Interest<ScriptReads>>>()
                .cloned(),
            mounted: runtime.resources.get::<Arc<MountedScripts>>().cloned(),
            refusals: runtime.resources.get::<Arc<Refusals>>().cloned(),
            trust_state: runtime.resources.get::<Arc<ProjectTrustState>>().cloned(),
            pack: None,
        }
    }

    /// Creates a compiler of the modules a game's pack holds, read through `reader`.
    ///
    /// Reports to what `runtime` holds, as [`from_runtime`](Self::from_runtime).
    /// Its modules are compiled when filed under `root`; filed under any other
    /// root, nothing is read and every module named is withdrawn.
    pub fn for_pack(runtime: &Runtime, root: impl Into<PathBuf>, reader: Arc<AssetReader>) -> Self {
        Self {
            pack: Some(PackScripts {
                root: root.into(),
                reader,
            }),
            ..Self::from_runtime(runtime)
        }
    }

    /// Returns the trust the project's scripts are read at now.
    fn trust(&self) -> Trust {
        self.trust_state
            .as_ref()
            .map_or(Trust::Project, |state| state.get().trust())
    }

    /// Returns a loader of the modules under `script_root`, reporting to the runtime's refusals.
    fn loader(&self, script_root: &Path) -> PreludeLoader<DiskLoader> {
        let refusals = self.refusals.clone().unwrap_or_else(Refusals::detached);
        PreludeLoader::new(DiskLoader::guarded(FileGuard::new(
            script_root,
            self.trust(),
            refusals,
        )))
    }

    /// Compiles every module under the script folder `script_root`, as a mount does.
    ///
    /// Each module's declarations are handed on once, however many import it.
    /// A module that does not compile is reported and left out.
    pub fn compile_all(&self, script_root: &Path) -> Recompiled {
        let loader = self.loader(script_root);
        let mut compiled = Recompiled::default();
        let mut declared = BTreeSet::new();
        for module in modules_under(script_root, self.refusals.as_ref(), self.trust()).into_keys() {
            self.compile_into(&loader, module, false, &mut compiled, &mut declared);
        }
        compiled
    }

    /// Compiles `module` through `loader`, reports it, and hands on its program and what it declares.
    ///
    /// An `edited` module hands on what it declares itself, nothing included.
    /// Returns whether it compiled; one that did not is left out, logged.
    fn compile_into(
        &self,
        loader: &dyn SourceLoader,
        module: String,
        edited: bool,
        compiled: &mut Recompiled,
        declared: &mut BTreeSet<String>,
    ) -> bool {
        let outcome = compile_module(loader, &module);
        self.report(&module, &outcome);
        let Some(program) = outcome.program else {
            log_diagnostics(&module, &outcome.diagnostics);
            return false;
        };
        // What an edited module declares now — nothing, when the author
        // removed its last component.
        if edited && declared.insert(module.clone()) {
            compiled.declarations.push(ScriptDeclarations {
                module: module.clone(),
                components: program
                    .declared
                    .iter()
                    .filter(|component| component.module == module)
                    .cloned()
                    .collect(),
            });
        }
        // And every module it declares through: a struct edited in one module
        // changes the defaults of a component another declares with it.
        declare_through(&program, compiled, declared);
        self.announce(&module, &program);
        compiled.reloads.push(ScriptReload {
            module,
            program: Some(program),
        });
        true
    }

    /// Reports that `module` cannot be read, under its own name; its program, if any, keeps running.
    fn report_unreadable(&self, module: &str) {
        log::error!("scripts: {module} cannot be read, so it is not compiled");
        if let Some(problems) = &self.problems {
            problems.report(
                ProblemSource::Compile {
                    module: module.to_owned(),
                },
                vec![Problem::of_file(
                    ProblemSeverity::Error,
                    module,
                    0,
                    0,
                    format!("`{module}` cannot be read: it was refused, or is not text"),
                )],
            );
        }
    }

    fn report(&self, module: &str, outcome: &khora_script::CompileOutcome) {
        if let Some(problems) = &self.problems {
            crate::script::problems::report_compile(problems, module, outcome);
        }
    }

    /// Announces what `module`'s program reads, before the intake installs it.
    ///
    /// The flow then projects those components the frame the program lands.
    fn announce(&self, module: &str, program: &Program) {
        if let Some(reads) = &self.reads {
            reads.announce(
                ScriptReads::incoming(module),
                program
                    .reads
                    .iter()
                    .map(|read| ComponentName::new(read.as_str())),
            );
        }
    }
}

impl ScriptCompiler for ProjectCompiler {
    /// Recompiles the modules `request` names and every module importing one
    /// of them; withdraws those whose file is gone.
    fn compile(&self, request: &CompileRequest) -> Recompiled {
        self.compile_leased(request).0
    }

    /// Compiles as [`compile`](Self::compile), leased while the request's root stays mounted.
    ///
    /// A request for a root already unmounted hands on withdrawals, or modules
    /// compiled from another root, with no lease.
    fn compile_leased(&self, request: &CompileRequest) -> (Recompiled, Option<Lease>) {
        // A root unmounted since the request was filed is no project's any
        // more: its modules are withdrawn, never compiled back in.
        if !self.is_mounted(&request.root) {
            return (self.unmounted(request), None);
        }
        let lease = self.mounted.as_ref().map(|mounted| {
            let (mounted, root) = (Arc::clone(mounted), request.root.clone());
            Lease::new(move || !mounted.is_closed(&root))
        });
        let (compiled, touched) = self.compile_mounted(request);
        // Unmounted while it compiled: the problems and reads of every module
        // it went through, importers included, are taken back too.
        if !self.is_mounted(&request.root) {
            let mut modules = request.modules.clone();
            modules.extend(touched);
            return (
                self.unmounted(&CompileRequest::new(request.root.clone(), modules)),
                None,
            );
        }
        (compiled, lease)
    }
}

impl ProjectCompiler {
    /// Whether `root` is mounted: not unmounted since. With no record of mounts, every root is.
    fn is_mounted(&self, root: &Path) -> bool {
        self.mounted
            .as_ref()
            .is_none_or(|mounted| !mounted.is_closed(root))
    }

    /// Whether the modules filed under `root` hold `module` now.
    fn provides(&self, root: &Path, module: &str) -> bool {
        match &self.pack {
            Some(pack) => {
                pack.is_root(root) && pack_modules(pack.reader.index()).contains_key(module)
            }
            None => is_on_disk(root, module),
        }
    }

    /// Returns what a request for a root no longer mounted hands on.
    ///
    /// A module another mounted root provides by name is compiled from there;
    /// any other is withdrawn, with its declarations, reads and problems.
    fn unmounted(&self, request: &CompileRequest) -> Recompiled {
        let mut compiled = Recompiled::default();
        let mut elsewhere: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
        for module in &request.modules {
            let provider = self.mounted.as_ref().and_then(|mounted| {
                mounted
                    .roots()
                    .into_iter()
                    .find(|other| self.provides(other, module))
            });
            match provider {
                Some(other) => {
                    elsewhere.entry(other).or_default().insert(module.clone());
                }
                None => {
                    compiled.declarations.push(ScriptDeclarations {
                        module: module.clone(),
                        components: Vec::new(),
                    });
                    compiled.reloads.push(ScriptReload {
                        module: module.clone(),
                        program: None,
                    });
                    if let Some(reads) = &self.reads {
                        reads.withdraw(&ScriptReads::incoming(module));
                    }
                    if let Some(problems) = &self.problems {
                        problems.report(
                            ProblemSource::Compile {
                                module: module.clone(),
                            },
                            Vec::new(),
                        );
                    }
                }
            }
        }
        for (root, modules) in elsewhere {
            let (from_there, _) = self.compile_mounted(&CompileRequest::new(root, modules));
            compiled.reloads.extend(from_there.reloads);
            compiled.declarations.extend(from_there.declarations);
        }
        compiled
    }

    /// Recompiles the modules `request` names and every module importing one.
    ///
    /// Withdraws those its root no longer holds. Returns what came out, and
    /// every module it went through.
    fn compile_mounted(&self, request: &CompileRequest) -> (Recompiled, BTreeSet<String>) {
        let root = request.root.as_path();
        match &self.pack {
            Some(pack) => self.compile_from(&pack.modules_under(root), root, &request.modules),
            None => {
                let source = DiskModules::open(root, self.refusals.as_ref(), self.trust());
                self.compile_from(&source, root, &request.modules)
            }
        }
    }

    /// Recompiles the `changed` modules of `root`, read from `source`, and every module importing one.
    ///
    /// Withdraws those `source` does not hold. Returns what came out, and every
    /// module it went through.
    fn compile_from(
        &self,
        source: &impl ModuleSource,
        root: &Path,
        changed: &BTreeSet<String>,
    ) -> (Recompiled, BTreeSet<String>) {
        let affected = importers_of(source.modules(), changed);
        let touched = affected.clone();
        let loader = PreludeLoader::new(SourcesOf(source));
        let mut compiled = Recompiled::default();
        let mut declared = BTreeSet::new();

        // A deleted file declares nothing any more: what it declared is
        // orphaned, kept with its data.
        for module in changed {
            if !source.holds(module) && declared.insert(module.clone()) {
                compiled.declarations.push(ScriptDeclarations {
                    module: module.clone(),
                    components: Vec::new(),
                });
            }
        }

        for module in affected {
            // Deleted, or moved away: its program leaves the runtime rather
            // than keep running code whose file is gone.
            if !source.holds(&module) {
                if let Some(problems) = &self.problems {
                    problems.report(
                        ProblemSource::Compile {
                            module: module.clone(),
                        },
                        Vec::new(),
                    );
                }
                if let Some(mounted) = &self.mounted {
                    mounted.forget_module(root, &module);
                }
                log::info!("scripts: {module} is gone, its program withdrawn");
                compiled.reloads.push(ScriptReload {
                    module,
                    program: None,
                });
                continue;
            }
            // Known from now on, compiled or not, so a folder that later takes
            // it away takes its program — or its errors — with it.
            if let Some(mounted) = &self.mounted {
                mounted.record_module(root, &module);
            }
            // Refused, or no text: what refused it reported it. Compiled, it
            // would only be "not found", at a place in no file of it.
            if source.source(&module).is_none() {
                self.report_unreadable(&module);
                continue;
            }
            let edited = changed.contains(&module);
            let name = module.clone();
            // Kept running when it does not compile: a save mid-edit usually does not.
            if self.compile_into(&loader, module, edited, &mut compiled, &mut declared) {
                log::info!("scripts: compiled {name}");
            }
        }
        (compiled, touched)
    }
}

/// A game's pack, read through its reader, and the root its scripts are filed under.
struct PackScripts {
    root: PathBuf,
    reader: Arc<AssetReader>,
}

impl std::fmt::Debug for PackScripts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackScripts")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl PackScripts {
    /// Whether `root` is the one the pack's scripts are filed under, however it is spelled.
    fn is_root(&self, root: &Path) -> bool {
        crate::asset::root_path::absolute(root) == crate::asset::root_path::absolute(&self.root)
    }

    /// Returns the modules a compile under `root` reads: the pack's, or none under another root.
    fn modules_under(&self, root: &Path) -> PackModules {
        if self.is_root(root) {
            PackModules::read(&self.reader)
        } else {
            PackModules::none()
        }
    }
}

/// Hands on, once per module, the components `program` says that module
/// declares, skipping the modules already in `declared`.
fn declare_through(program: &Program, compiled: &mut Recompiled, declared: &mut BTreeSet<String>) {
    let mut by_module: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for component in &program.declared {
        by_module
            .entry(component.module.as_str())
            .or_default()
            .push(component.clone());
    }
    for (module, components) in by_module {
        if declared.insert(module.to_owned()) {
            compiled.declarations.push(ScriptDeclarations {
                module: module.to_owned(),
                components,
            });
        }
    }
}

fn log_diagnostics(module: &str, diagnostics: &[khora_script::diagnostics::Diagnostic]) {
    for diagnostic in diagnostics {
        log::error!("{module}: {}", diagnostic.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetService;

    const GUARD: &str = "fn void Main() { }";

    /// Whether `compiled` hands on a program for `module`; `None` when it names it not.
    fn compiled_program(compiled: &Recompiled, module: &str) -> Option<bool> {
        (compiled.reloads.iter())
            .find(|reload| reload.module == module)
            .map(|reload| reload.program.is_some())
    }

    /// **A pack's compiler reads its modules from the pack alone.**
    ///
    /// A module on disk, filed under the pack's root or under another, is
    /// withdrawn, never compiled.
    #[test]
    fn a_pack_compiler_withdraws_a_module_its_pack_does_not_hold() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let assets = dir.path().join("assets");
        let scripts = assets.join("scripts");
        std::fs::create_dir_all(&scripts).expect("the script folder");
        std::fs::write(scripts.join("guard.erg"), GUARD).expect("the module is written");
        let request =
            |root: &Path| CompileRequest::new(root.to_path_buf(), ["guard.erg".to_owned()]);

        let from_disk = ProjectCompiler::default().compile(&request(&assets));
        assert_eq!(
            compiled_program(&from_disk, "guard.erg"),
            Some(true),
            "the module compiles from disk"
        );

        let empty_pack = AssetService::empty().reader();
        let compiler = ProjectCompiler::for_pack(&Runtime::default(), &assets, empty_pack);
        for root in [assets.as_path(), dir.path()] {
            assert_eq!(
                compiled_program(&compiler.compile(&request(root)), "guard.erg"),
                Some(false),
                "a module on disk under {} is withdrawn",
                root.display()
            );
        }
    }
}
