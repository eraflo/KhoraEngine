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

//! Turning a module and everything it imports into a program.
//!
//! The whole reachable set is resolved through a [`SourceLoader`], so this
//! module never opens a file itself: the caller hands it a loader that reads
//! from disk, from memory, or from a pack.
//!
//! # Nothing is replaced until it compiles
//!
//! A failed compile yields no program and reports why, so the caller keeps the
//! program it was running. The alternative — swapping in whatever came out —
//! means a saved file with a typo stops the game, which is the opposite of what
//! an author reaches for hot-reload to get. Editing is iterative and most
//! intermediate states do not compile.

use crate::diagnostics::Diagnostic;
use crate::modules::SourceLoader;
use crate::vm::Program;

/// What compiling a module produced.
#[derive(Debug)]
pub struct CompileOutcome {
    /// The program, when every stage succeeded.
    ///
    /// `None` means the source did not compile. The caller keeps whatever it
    /// was running: a typo mid-edit must not stop the game.
    pub program: Option<Program>,
    /// Everything that went wrong, from whichever stage found it.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileOutcome {
    /// Whether a program came out.
    pub fn succeeded(&self) -> bool {
        self.program.is_some()
    }
}

/// Resolves `entry` and everything it imports, then compiles the lot.
///
/// The stages run in order and stop at the first that fails, because each one
/// assumes what the last proved: the compiler trusts the checker's types, and
/// the checker trusts the parser's tree. Running a later stage on a failed
/// earlier one produces noise, not more information.
pub fn compile_module(loader: &dyn SourceLoader, entry: &str) -> CompileOutcome {
    let resolved = crate::resolve(entry, loader);
    if resolved.has_errors() {
        return CompileOutcome {
            program: None,
            diagnostics: resolved.diagnostics,
        };
    }

    // Imports come before importers, so folding the modules in order gives the
    // compiler every declaration before the file that uses it.
    let merged = crate::ast::Module {
        imports: Vec::new(),
        items: resolved
            .modules
            .iter()
            .flat_map(|module| module.module.items.iter().cloned())
            .collect(),
    };

    let mut diagnostics = crate::modules::report_name_clashes(&resolved);
    diagnostics.extend(crate::modules::report_foreign_components(&resolved));
    if crate::diagnostics::has_errors(&diagnostics) {
        return CompileOutcome {
            program: None,
            diagnostics,
        };
    }

    let checked = crate::check(&merged);
    diagnostics.extend(checked.diagnostics);
    if crate::diagnostics::has_errors(&diagnostics) {
        return CompileOutcome {
            program: None,
            diagnostics,
        };
    }

    let compiled = crate::compile(&merged);
    diagnostics.extend(compiled.diagnostics);
    let failed = crate::diagnostics::has_errors(&diagnostics);

    CompileOutcome {
        program: (!failed).then_some(compiled.program),
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryLoader;

    #[test]
    fn a_lone_module_compiles() {
        let loader = MemoryLoader::new().with(
            "guard.erg",
            "behavior Guard { int health = 100; on Damaged(int by) { health -= by; } }",
        );
        let result = compile_module(&loader, "guard.erg");

        assert!(result.succeeded(), "{:?}", result.diagnostics);
        let program = result.program.expect("compiled");
        assert!(program.layout("Guard").is_some());
    }

    /// **What the decoder could not do alone.** An imported declaration has to
    /// be in scope, which needs the whole reachable set.
    #[test]
    fn an_imported_declaration_is_in_scope() {
        let loader = MemoryLoader::new()
            .with("math.erg", "fn float Double(float x) { return x * 2.0; }")
            .with(
                "guard.erg",
                r#"import "math.erg";
                   fn float Main() { return Double(21.0); }"#,
            );

        let result = compile_module(&loader, "guard.erg");
        assert!(result.succeeded(), "{:?}", result.diagnostics);
    }

    /// **Nothing is replaced until it compiles.** A typo mid-edit must not stop
    /// the game, so a failed compile yields no program and the caller keeps
    /// what it had.
    #[test]
    fn a_syntax_error_produces_no_program_and_says_why() {
        let loader = MemoryLoader::new().with("broken.erg", "fn void ( ) { }");
        let result = compile_module(&loader, "broken.erg");

        assert!(!result.succeeded());
        assert!(!result.diagnostics.is_empty());
    }

    #[test]
    fn a_type_error_produces_no_program() {
        let loader = MemoryLoader::new().with("bad.erg", "fn float Main() { return Abs(true); }");
        let result = compile_module(&loader, "bad.erg");

        assert!(!result.succeeded(), "a bool is not a float");
    }

    #[test]
    fn a_missing_import_produces_no_program() {
        let loader = MemoryLoader::new().with(
            "guard.erg",
            r#"import "gone.erg";
               fn void Main() { }"#,
        );
        let result = compile_module(&loader, "guard.erg");

        assert!(!result.succeeded());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("cannot find module")));
    }

    /// A cycle has no compilation order, so it is refused with the chain that
    /// closes it rather than compiled into something arbitrary.
    #[test]
    fn a_circular_import_produces_no_program() {
        let loader = MemoryLoader::new()
            .with("a.erg", "import \"b.erg\"; fn void A() { }")
            .with("b.erg", "import \"a.erg\"; fn void B() { }");

        let result = compile_module(&loader, "a.erg");
        assert!(!result.succeeded());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("imports itself")));
    }

    /// Two modules declaring the same name would give one meaning to both, so
    /// the clash is refused before anything is compiled against it.
    #[test]
    fn a_name_declared_in_two_modules_produces_no_program() {
        let loader = MemoryLoader::new()
            .with("other.erg", "struct Health { int hp; }")
            .with(
                "main.erg",
                r#"import "other.erg";
                   struct Health { int current; }"#,
            );

        let result = compile_module(&loader, "main.erg");
        assert!(!result.succeeded());
    }
}
