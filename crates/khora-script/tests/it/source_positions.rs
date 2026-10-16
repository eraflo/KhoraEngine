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

//! **Positions across modules** (Ergon spec 11): a program is compiled from
//! every module its entry imports, merged into one tree, and each of its
//! diagnostics is located — module, 1-based line, column in characters —
//! through the outcome's source map. Parse, resolve, check and compile
//! diagnostics alike; their text is what it always was.

use khora_script::{compile_module, CompileOutcome, Location, MemoryLoader, Severity, Span};

/// `entry` compiled among `modules`.
fn compile(modules: &[(&str, &str)], entry: &str) -> CompileOutcome {
    let mut loader = MemoryLoader::new();
    for &(path, source) in modules {
        loader = loader.with(path, source);
    }
    compile_module(&loader, entry)
}

/// Where each diagnostic whose message is `message` sits, in report order.
fn located(outcome: &CompileOutcome, message: &str) -> Vec<Location> {
    let matching: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message == message)
        .collect();
    assert!(
        !matching.is_empty(),
        "a diagnostic `{message}` was reported: {:?}",
        outcome.diagnostics
    );
    matching
        .iter()
        .map(|diagnostic| {
            outcome
                .sources
                .locate(diagnostic.span)
                .unwrap_or_else(|| panic!("`{message}` at {:?} is located", diagnostic.span))
        })
        .collect()
}

fn at(module: &str, line: u32, column: u32) -> Location {
    Location {
        module: module.to_owned(),
        line,
        column,
    }
}

/// The first warning `outcome` reports, located.
fn first_warning(outcome: &CompileOutcome) -> Option<Location> {
    let warning = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == Severity::Warning)
        .expect("the unreachable arm is warned about");
    outcome.sources.locate(warning.span)
}

/// A behavior whose `match` has an arm nothing reaches: a warning at the `_`
/// arm, line 3, column 10, and a program all the same.
const UNREACHABLE_ARM: &str = "behavior Guard {\n    void Update(float dt) { match (n) { int x => { }\n         _ => { } } }\n    int n = 0;\n}\n";

/// **A type error in the entry module** is located at the operand the
/// checker underlines: line 2, column 16 of `main.erg`.
#[test]
fn a_type_error_in_the_entry_module_is_located_in_it() {
    let outcome = compile(
        &[("main.erg", "fn float Main() {\n    return Abs(true);\n}\n")],
        "main.erg",
    );

    assert_eq!(
        located(&outcome, "expected `float`, found `bool`"),
        vec![at("main.erg", 2, 16)]
    );
}

/// **A type error in an imported module is located in that module**, not in
/// the entry the program was compiled from — the checker runs over the
/// merged tree, where a span alone names no file.
#[test]
fn a_type_error_in_an_imported_module_is_located_in_the_imported_module() {
    let outcome = compile(
        &[
            (
                "math.erg",
                "fn float Double(float x) {\n    return Abs(true);\n}\n",
            ),
            (
                "main.erg",
                "import \"math.erg\";\nfn float Main() { return Double(1.0); }\n",
            ),
        ],
        "main.erg",
    );

    assert!(!outcome.succeeded());
    assert_eq!(
        located(&outcome, "expected `float`, found `bool`"),
        vec![at("math.erg", 2, 16)]
    );
}

/// **A parse error in an imported module** is located in it: the `(` where
/// a function name was expected, line 2, column 9 of `broken.erg`.
#[test]
fn a_parse_error_in_an_imported_module_is_located_in_the_imported_module() {
    let outcome = compile(
        &[
            ("broken.erg", "// a broken helper\nfn void ( ) { }\n"),
            ("main.erg", "import \"broken.erg\";\nfn void Main() { }\n"),
        ],
        "main.erg",
    );

    assert_eq!(
        located(&outcome, "expected a function name, found `(`"),
        vec![at("broken.erg", 2, 9)]
    );
}

/// **An import of a missing module is located at the `import`**, in the
/// module that wrote it — the message as it always read.
#[test]
fn an_import_of_a_missing_module_is_located_at_the_import() {
    let outcome = compile(
        &[(
            "main.erg",
            "// the guard\nimport \"gone.erg\";\nfn void Main() { }\n",
        )],
        "main.erg",
    );

    assert_eq!(
        located(&outcome, "cannot find module `gone.erg`"),
        vec![at("main.erg", 2, 1)]
    );
}

/// **An imported module's own missing import is located in that module**:
/// the import that asked for it is in `combat.erg`, resolved while the
/// entry importing it is still being walked.
#[test]
fn a_missing_import_of_an_imported_module_is_located_in_the_importing_module() {
    let outcome = compile(
        &[
            (
                "combat.erg",
                "// combat\n\nimport \"gone.erg\";\nfn void Hit() { }\n",
            ),
            ("main.erg", "import \"combat.erg\";\nfn void Main() { }\n"),
        ],
        "main.erg",
    );

    assert_eq!(
        located(&outcome, "cannot find module `gone.erg`"),
        vec![at("combat.erg", 3, 1)]
    );
}

/// **Columns count characters, not bytes**: accents in a comment above and
/// in a string before the error do not push the column past what the reader
/// counts — `true` is the 46th character of its line, the 47th byte.
#[test]
fn characters_of_several_bytes_before_an_error_count_one_column_each() {
    let outcome = compile(
        &[
            (
                "accents.erg",
                "// é à ü\nfn float Bad() { var s = \"héllo\"; return Abs(true); }\n",
            ),
            ("main.erg", "import \"accents.erg\";\nfn void Main() { }\n"),
        ],
        "main.erg",
    );

    assert_eq!(
        located(&outcome, "expected `float`, found `bool`"),
        vec![at("accents.erg", 2, 46)]
    );
}

/// **Two modules whose spans would collide** — the same error at the same
/// offset of each file — are told apart: each parse error is located in its
/// own module, and their spans differ.
#[test]
fn the_same_parse_error_at_the_same_offset_of_two_modules_is_located_in_each() {
    let outcome = compile(
        &[
            ("a.erg", "fn void ( ) { }\n"),
            ("b.erg", "fn void ( ) { }\n"),
            (
                "main.erg",
                "import \"a.erg\";\nimport \"b.erg\";\nfn void Main() { }\n",
            ),
        ],
        "main.erg",
    );

    let spans: Vec<Span> = outcome.diagnostics.iter().map(|d| d.span).collect();
    assert_eq!(spans.len(), 2, "{:?}", outcome.diagnostics);
    assert_ne!(spans[0], spans[1], "one offset space, one span per place");

    let mut locations = located(&outcome, "expected a function name, found `(`");
    locations.sort_by(|a, b| a.module.cmp(&b.module));
    assert_eq!(locations, vec![at("a.erg", 1, 9), at("b.erg", 1, 9)]);
}

/// **The same for the checker**, which runs over the merged tree: a type
/// error at the same offset of two modules is located in each.
#[test]
fn the_same_type_error_at_the_same_offset_of_two_modules_is_located_in_each() {
    let outcome = compile(
        &[
            ("a.erg", "fn float A() {\n    return Abs(true);\n}\n"),
            ("b.erg", "fn float B() {\n    return Abs(true);\n}\n"),
            (
                "main.erg",
                "import \"a.erg\";\nimport \"b.erg\";\nfn void Main() { }\n",
            ),
        ],
        "main.erg",
    );

    let mut locations = located(&outcome, "expected `float`, found `bool`");
    locations.sort_by(|a, b| a.module.cmp(&b.module));
    assert_eq!(locations, vec![at("a.erg", 2, 16), at("b.erg", 2, 16)]);
}

/// **A name declared in two modules** is located at the second declaration,
/// in the module that made it.
#[test]
fn a_name_clash_is_located_at_the_second_declaration() {
    let outcome = compile(
        &[
            ("other.erg", "struct Health { int hp; }\n"),
            (
                "main.erg",
                "import \"other.erg\";\n\nstruct Health { int current; }\n",
            ),
        ],
        "main.erg",
    );

    assert_eq!(
        located(
            &outcome,
            "`Health` is declared in both `other.erg` and `main.erg`"
        ),
        vec![at("main.erg", 3, 8)]
    );
}

/// **A warning of a program that compiled is located too**: the unreachable
/// `_` arm, line 3, column 10.
#[test]
fn a_warning_of_a_successful_compile_is_located() {
    let outcome = compile(&[("main.erg", UNREACHABLE_ARM)], "main.erg");

    assert!(outcome.succeeded(), "{:?}", outcome.diagnostics);
    assert_eq!(first_warning(&outcome), Some(at("main.erg", 3, 10)));
}

/// **A warning in an imported module of a program that compiled** is
/// located in that module.
#[test]
fn a_warning_in_an_imported_module_is_located_in_it() {
    let outcome = compile(
        &[
            ("guard.erg", UNREACHABLE_ARM),
            ("main.erg", "import \"guard.erg\";\nfn void Main() { }\n"),
        ],
        "main.erg",
    );

    assert!(outcome.succeeded(), "{:?}", outcome.diagnostics);
    assert_eq!(first_warning(&outcome), Some(at("guard.erg", 3, 10)));
}

/// A span past the end of every module of the program falls in none.
#[test]
fn a_span_past_every_module_locates_nowhere() {
    let outcome = compile(&[("main.erg", "fn void Main() { }\n")], "main.erg");

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.sources.locate(Span::new(u32::MAX - 1, u32::MAX)),
        None
    );
}
