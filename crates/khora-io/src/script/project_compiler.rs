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

//! A project's scripts, compiled from disk for the `loading` family, one
//! [`CompileRequest`] at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use khora_core::lane::Lease;
use khora_core::problems::{ProblemSource, Problems};
use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptReads};
use khora_core::Runtime;
use khora_script::compile_module;
use khora_script::vm::Program;

use crate::script::compile::DiskLoader;
use crate::script::declarations::ScriptDeclarations;
use crate::script::hot_reload::{importers_of, is_on_disk, modules_under};
use crate::script::layout::{script_dir, SCRIPT_DIR};
use crate::script::mirror::PreludeLoader;
use crate::script::mounted::MountedScripts;
use crate::trust::{FileGuard, ProjectTrustState, Refusals, Trust};

// What a compile is asked and hands on, named here so the engine's wiring
// reaches them through the compiler it installs, without a dependency on
// `khora-script` of its own.
pub use khora_script::reload::ScriptReload;
pub use khora_script::toolchain::{CompileRequest, Recompiled, ScriptCompiler};

/// The engine's [`ScriptCompiler`]: reads a project's script folder from disk, with the prelude.
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
        let refusals = self
            .refusals
            .clone()
            .unwrap_or_else(|| Arc::new(Refusals::new(None, None)));
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
            let outcome = compile_module(&loader, &module);
            self.report(&module, &outcome);
            match outcome.program {
                Some(program) => {
                    declare_through(&program, &mut compiled, &mut declared);
                    self.announce(&module, &program);
                    compiled.reloads.push(ScriptReload {
                        module,
                        program: Some(program),
                    });
                }
                None => log_diagnostics(&module, &outcome.diagnostics),
            }
        }
        compiled
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

    /// Returns what a request for a root no longer mounted hands on.
    ///
    /// A module another mounted root provides by name is compiled from there;
    /// any other is withdrawn, with its declarations, reads and problems.
    fn unmounted(&self, request: &CompileRequest) -> Recompiled {
        let mut compiled = Recompiled::default();
        let mut elsewhere: BTreeMap<std::path::PathBuf, BTreeSet<String>> = BTreeMap::new();
        for module in &request.modules {
            let provider = self.mounted.as_ref().and_then(|mounted| {
                mounted
                    .roots()
                    .into_iter()
                    .find(|other| is_on_disk(other, module))
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
    /// Withdraws those whose file is gone. Returns what came out, and every
    /// module it went through.
    fn compile_mounted(&self, request: &CompileRequest) -> (Recompiled, BTreeSet<String>) {
        let root = request.root.as_path();
        let changed = &request.modules;
        let script_root = root.join(SCRIPT_DIR);
        // The folder spelled exactly: a `Scripts/` holds no modules, so every
        // one changed under it is gone.
        let on_disk = script_dir(root)
            .map(|dir| modules_under(&dir, self.refusals.as_ref(), self.trust()))
            .unwrap_or_default();
        let affected = importers_of(&on_disk, changed);
        let touched = affected.clone();
        let loader = self.loader(&script_root);
        let mut compiled = Recompiled::default();
        let mut declared = BTreeSet::new();

        // A deleted file declares nothing any more: what it declared is
        // orphaned, kept with its data.
        for module in changed {
            if !is_on_disk(root, module) && declared.insert(module.clone()) {
                compiled.declarations.push(ScriptDeclarations {
                    module: module.clone(),
                    components: Vec::new(),
                });
            }
        }

        for module in affected {
            // Deleted, or moved away: its program leaves the runtime rather
            // than keep running code whose file is gone.
            if !is_on_disk(root, &module) {
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
                log::info!("script hot-reload: {module} is gone, its program withdrawn");
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
            let outcome = compile_module(&loader, &module);
            self.report(&module, &outcome);
            match outcome.program {
                Some(program) => {
                    log::info!("script hot-reload: recompiled {module}");
                    // What an edited module declares now — nothing, when the
                    // author removed its last component.
                    if changed.contains(&module) && declared.insert(module.clone()) {
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
                    // And every module it declares through: a struct edited
                    // in one module changes the defaults of a component
                    // another declares with it.
                    declare_through(&program, &mut compiled, &mut declared);
                    self.announce(&module, &program);
                    compiled.reloads.push(ScriptReload {
                        module,
                        program: Some(program),
                    });
                }
                // Kept running: a save mid-edit usually does not compile.
                None => log_diagnostics(&module, &outcome.diagnostics),
            }
        }
        (compiled, touched)
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
