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

//! Resolving a module and everything it imports.
//!
//! Walks the import graph from an entry point, loading and parsing each module
//! once, and hands back the modules in **dependency order** — a module always
//! appears after everything it imports. Later stages can then walk the list
//! forward and know that every name they meet has already been seen.
//!
//! # Cycles
//!
//! Refused, with the chain that closes them. A cycle has no dependency order,
//! so there is no honest way to compile one; and "circular import" without the
//! path leaves the author to find the loop themselves, which in a large project
//! is the whole difficulty.
//!
//! # Compiled once
//!
//! A module imported from twenty places is loaded, parsed and reported on once.
//! Without that, a diamond would produce duplicate declarations that collide
//! with themselves, and the same syntax error twenty times.

pub mod loader;
pub mod path;

#[cfg(test)]
mod tests;

pub use loader::{MemoryLoader, SourceLoader};
pub use path::{normalise, PathError};

use std::collections::{HashMap, HashSet};

use crate::ast::Module;
use crate::diagnostics::{Diagnostic, SourceFile, Span};
use crate::lexer::lex;
use crate::parser::parse;

/// One resolved module.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Its normalised path, which is also its identity.
    pub path: String,
    /// Its source, kept so diagnostics can be rendered against it.
    pub source: SourceFile,
    /// Its syntax tree.
    pub module: Module,
}

/// The outcome of resolving an import graph.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// Modules in dependency order: imports before importers.
    pub modules: Vec<ResolvedModule>,
    /// Problems found, across every module walked.
    pub diagnostics: Vec<Diagnostic>,
}

impl Resolved {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        crate::diagnostics::has_errors(&self.diagnostics)
    }

    /// A resolved module by path.
    pub fn get(&self, path: &str) -> Option<&ResolvedModule> {
        self.modules.iter().find(|m| m.path == path)
    }
}

/// Resolves `entry` and everything it imports.
pub fn resolve(entry: &str, loader: &dyn SourceLoader) -> Resolved {
    let mut resolver = Resolver {
        loader,
        modules: Vec::new(),
        diagnostics: Vec::new(),
        done: HashSet::new(),
        in_progress: Vec::new(),
    };

    match normalise(entry) {
        Ok(path) => resolver.visit(&path, None),
        Err(error) => resolver.diagnostics.push(
            Diagnostic::error(format!("{}: `{entry}`", error.message()), Span::empty(0))
                .with_note(error.note()),
        ),
    }

    Resolved {
        modules: resolver.modules,
        diagnostics: resolver.diagnostics,
    }
}

struct Resolver<'a> {
    loader: &'a dyn SourceLoader,
    modules: Vec<ResolvedModule>,
    diagnostics: Vec<Diagnostic>,
    /// Paths already resolved, so a diamond loads once.
    done: HashSet<String>,
    /// The chain currently being walked, innermost last. A path reappearing
    /// here is a cycle, and the chain is the report.
    in_progress: Vec<String>,
}

impl<'a> Resolver<'a> {
    /// Depth-first, so a module's imports are appended before it is.
    ///
    /// `from` is the import statement that led here, so a missing file is
    /// reported at the line that asked for it rather than at the top of a
    /// file the author did not write.
    fn visit(&mut self, path: &str, from: Option<Span>) {
        if self.done.contains(path) {
            return;
        }

        if let Some(start) = self.in_progress.iter().position(|seen| seen == path) {
            let mut chain: Vec<String> = self.in_progress[start..].to_vec();
            chain.push(path.to_owned());
            self.diagnostics.push(
                Diagnostic::error(
                    format!("`{path}` imports itself, through {}", chain.join(" → ")),
                    from.unwrap_or(Span::empty(0)),
                )
                .with_note(
                    "a cycle has no order to compile in — move what both modules need into a third one they can each import",
                ),
            );
            return;
        }

        let Some(text) = self.loader.load(path) else {
            self.diagnostics.push(
                Diagnostic::error(
                    format!("cannot find module `{path}`"),
                    from.unwrap_or(Span::empty(0)),
                )
                .with_note("the path is relative to the project's script root"),
            );
            // Marked done so twenty importers of a missing module report once.
            self.done.insert(path.to_owned());
            return;
        };

        let source = SourceFile::new(path, text);
        let lexed = lex(source.text());
        self.diagnostics.extend(lexed.diagnostics);

        let parsed = parse(lexed.tokens);
        self.diagnostics.extend(parsed.diagnostics);
        let module = parsed.module;

        self.in_progress.push(path.to_owned());

        for import in &module.imports {
            match normalise(&import.path) {
                Ok(target) => self.visit(&target, Some(import.span)),
                Err(error) => self.diagnostics.push(
                    Diagnostic::error(
                        format!("{}: `{}`", error.message(), import.path),
                        import.span,
                    )
                    .with_note(error.note()),
                ),
            }
        }

        self.in_progress.pop();
        self.done.insert(path.to_owned());

        // Appended after its imports, which is what makes the list a
        // dependency order.
        self.modules.push(ResolvedModule {
            path: path.to_owned(),
            source,
            module,
        });
    }
}

/// Reports names declared in more than one module of a resolved graph.
///
/// Ergon has no namespaces yet: a name is global to the program, so two modules
/// declaring `Health` would silently give one meaning to both. Reporting is the
/// honest option until `as` prefixes cover declarations too.
pub fn report_name_clashes(resolved: &Resolved) -> Vec<Diagnostic> {
    let mut owners: HashMap<&str, &str> = HashMap::new();
    let mut clashes = Vec::new();

    for resolved_module in &resolved.modules {
        for item in &resolved_module.module.items {
            match owners.get(item.name()) {
                Some(first) => clashes.push(
                    Diagnostic::error(
                        format!(
                            "`{}` is declared in both `{first}` and `{}`",
                            item.name(),
                            resolved_module.path
                        ),
                        item.name_span(),
                    )
                    .with_note(
                        "declarations share one global namespace, so two modules cannot use the same name — rename one",
                    ),
                ),
                None => {
                    owners.insert(item.name(), &resolved_module.path);
                }
            }
        }
    }
    clashes
}
