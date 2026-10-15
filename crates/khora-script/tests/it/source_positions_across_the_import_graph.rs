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

//! **Positions where modules meet** (Ergon spec 11): a module imported
//! through two paths is placed once and its error located once; an error at
//! the very end of an imported module stays in it, not in the module placed
//! after; a bad entry path is located nowhere. Placing a module at another
//! offset changes nothing a running program observes — its code and
//! fingerprints are those of the module at offset zero.

use khora_script::{compile_module, CompileOutcome, MemoryLoader};

fn compile(modules: &[(&str, &str)], entry: &str) -> CompileOutcome {
    let mut loader = MemoryLoader::new();
    for &(path, source) in modules {
        loader = loader.with(path, source);
    }
    compile_module(&loader, entry)
}

/// The module and line each error of `outcome` is located at.
fn errors_at(outcome: &CompileOutcome) -> Vec<(String, u32)> {
    outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| {
            let location = outcome
                .sources
                .locate(diagnostic.span)
                .unwrap_or_else(|| panic!("{diagnostic:?} is located"));
            (location.module, location.line)
        })
        .collect()
}

/// **A diamond places the shared module once**: its type error is reported
/// once, located in it.
#[test]
fn a_module_imported_through_two_paths_is_located_once() {
    let outcome = compile(
        &[
            ("shared.erg", "fn float S() {\n    return Abs(true);\n}\n"),
            ("left.erg", "import \"shared.erg\";\nfn void L() { }\n"),
            ("right.erg", "import \"shared.erg\";\nfn void R() { }\n"),
            (
                "main.erg",
                "import \"left.erg\";\nimport \"right.erg\";\nfn void Main() { }\n",
            ),
        ],
        "main.erg",
    );

    assert_eq!(errors_at(&outcome), vec![("shared.erg".to_owned(), 2)]);
}

/// **An error at the end of an imported module** — an unclosed body, the
/// parser out of input — is located in that module, on its last line, not
/// at the start of the module placed after it.
#[test]
fn an_error_at_the_end_of_an_imported_module_is_located_in_it() {
    let outcome = compile(
        &[
            ("unclosed.erg", "fn void U() {\n    var x = 1;\n"),
            ("after.erg", "fn void A() { }\n"),
            (
                "main.erg",
                "import \"unclosed.erg\";\nimport \"after.erg\";\nfn void Main() { }\n",
            ),
        ],
        "main.erg",
    );

    let errors = errors_at(&outcome);
    assert!(!errors.is_empty(), "{:?}", outcome.diagnostics);
    assert!(
        errors
            .iter()
            .all(|(module, line)| module == "unclosed.erg" && *line >= 2),
        "{errors:?}"
    );
}

/// **A bad entry path is located nowhere**: no module was read.
#[test]
fn a_bad_entry_path_is_located_nowhere() {
    let outcome = compile(&[], "../outside.erg");

    assert!(!outcome.succeeded());
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| outcome.sources.locate(diagnostic.span).is_none()),
        "{:?}",
        outcome.diagnostics
    );
}

/// **A module's place in the offset space is invisible to the program**:
/// `helper.erg` compiled as the entry (offset zero) and compiled inside an
/// importer (offset past it) gives its function the same code, sites and
/// fingerprint — a frozen machine standing in it resumes the same.
#[test]
fn a_module_placed_further_along_compiles_to_the_same_function() {
    let helper = "fn int Count(int n) {\n    var total = 0;\n    for (var i = 0; i < n; i += 1) { total += i; }\n    return total;\n}\n";
    let alone = compile(&[("helper.erg", helper)], "helper.erg");
    let imported = compile(
        &[
            ("helper.erg", helper),
            (
                "main.erg",
                "// a long preamble, so the helper sits well past offset zero\nimport \"helper.erg\";\nfn int Main() { return Count(3); }\n",
            ),
        ],
        "main.erg",
    );

    let alone = alone.program.expect("compiles alone");
    let imported = imported.program.expect("compiles imported");
    let (a, b) = (
        alone.function("Count").expect("Count"),
        imported.function("Count").expect("Count"),
    );
    assert_eq!(a.code, b.code);
    assert_eq!(a.sites, b.sites);
    assert_eq!(a.fingerprint, b.fingerprint);
}
