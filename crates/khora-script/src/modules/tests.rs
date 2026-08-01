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

//! Module resolution tests.
//!
//! The milestone for this piece: a circular import must **fail to compile**,
//! and say where the loop closes.

use super::{report_name_clashes, resolve, MemoryLoader};

/// The paths resolved, in the order they were produced.
fn order(resolved: &super::Resolved) -> Vec<&str> {
    resolved.modules.iter().map(|m| m.path.as_str()).collect()
}

fn messages(resolved: &super::Resolved) -> Vec<String> {
    resolved
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn a_lone_module_resolves() {
    let loader = MemoryLoader::new().with("main.erg", "fn void Main() { }");
    let resolved = resolve("main.erg", &loader);

    assert!(!resolved.has_errors(), "{:?}", messages(&resolved));
    assert_eq!(order(&resolved), vec!["main.erg"]);
}

/// Imports come before importers, so a later stage can walk the list forward
/// and know every name it meets has been seen.
#[test]
fn modules_arrive_in_dependency_order() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "combat/damage.erg";
               fn void Main() { }"#,
        )
        .with(
            "combat/damage.erg",
            r#"import "math/util.erg";
               fn void Apply() { }"#,
        )
        .with("math/util.erg", "fn void Helper() { }");

    let resolved = resolve("main.erg", &loader);
    assert!(!resolved.has_errors(), "{:?}", messages(&resolved));
    assert_eq!(
        order(&resolved),
        vec!["math/util.erg", "combat/damage.erg", "main.erg"]
    );
}

/// A diamond must load its shared module once. Twice would mean duplicate
/// declarations colliding with themselves, and every diagnostic doubled.
#[test]
fn a_shared_module_is_resolved_once() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "left.erg";
               import "right.erg";
               fn void Main() { }"#,
        )
        .with(
            "left.erg",
            r#"import "shared.erg";
               fn void Left() { }"#,
        )
        .with(
            "right.erg",
            r#"import "shared.erg";
               fn void Right() { }"#,
        )
        .with("shared.erg", "fn void Shared() { }");

    let resolved = resolve("main.erg", &loader);
    assert!(!resolved.has_errors(), "{:?}", messages(&resolved));

    let paths = order(&resolved);
    assert_eq!(
        paths.iter().filter(|p| **p == "shared.erg").count(),
        1,
        "resolved once, got {paths:?}"
    );
    assert_eq!(paths[0], "shared.erg", "and before both importers");
}

/// The milestone. A cycle has no compilation order, and the report names the
/// loop — "circular import" alone leaves the author to find it.
#[test]
fn a_cycle_is_refused_and_names_the_loop() {
    let loader = MemoryLoader::new()
        .with(
            "a.erg",
            r#"import "b.erg";
               fn void A() { }"#,
        )
        .with(
            "b.erg",
            r#"import "a.erg";
               fn void B() { }"#,
        );

    let resolved = resolve("a.erg", &loader);
    assert!(resolved.has_errors());

    let found = messages(&resolved);
    let cycle = found
        .iter()
        .find(|m| m.contains("imports itself"))
        .unwrap_or_else(|| panic!("expected a cycle report, got {found:?}"));
    assert!(cycle.contains("a.erg"), "the chain names both: {cycle}");
    assert!(cycle.contains("b.erg"), "the chain names both: {cycle}");
    assert!(cycle.contains('→'), "the chain is shown: {cycle}");
}

#[test]
fn a_module_importing_itself_is_refused() {
    let loader = MemoryLoader::new().with(
        "solo.erg",
        r#"import "solo.erg";
        fn void Solo() { }"#,
    );
    let resolved = resolve("solo.erg", &loader);
    assert!(resolved.has_errors());
    assert!(messages(&resolved)
        .iter()
        .any(|m| m.contains("imports itself")));
}

/// A longer loop is reported with its whole chain, which is where finding it by
/// hand actually costs something.
#[test]
fn a_three_module_cycle_reports_the_whole_chain() {
    let loader = MemoryLoader::new()
        .with(
            "a.erg",
            r#"import "b.erg";
               fn void A() { }"#,
        )
        .with(
            "b.erg",
            r#"import "c.erg";
               fn void B() { }"#,
        )
        .with(
            "c.erg",
            r#"import "a.erg";
               fn void C() { }"#,
        );

    let resolved = resolve("a.erg", &loader);
    let found = messages(&resolved);
    let cycle = found
        .iter()
        .find(|m| m.contains("imports itself"))
        .unwrap_or_else(|| panic!("expected a cycle, got {found:?}"));

    for module in ["a.erg", "b.erg", "c.erg"] {
        assert!(cycle.contains(module), "{module} missing from: {cycle}");
    }
}

/// The error points at the `import` statement that asked for the module, not
/// at the top of a file the author did not write.
#[test]
fn a_missing_module_is_reported_at_the_import() {
    let loader = MemoryLoader::new().with("main.erg", "import \"nope.erg\";\nfn void Main() { }");
    let resolved = resolve("main.erg", &loader);

    let diagnostic = resolved
        .diagnostics
        .iter()
        .find(|d| d.message.contains("cannot find module"))
        .expect("the missing module is reported");
    assert!(
        diagnostic.span.start < 20,
        "the span points at the import on line 1"
    );
}

/// Twenty importers of one missing module should not produce twenty messages.
#[test]
fn a_missing_module_is_reported_once() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "left.erg";
               import "right.erg";
               fn void Main() { }"#,
        )
        .with(
            "left.erg",
            r#"import "gone.erg";
               fn void Left() { }"#,
        )
        .with(
            "right.erg",
            r#"import "gone.erg";
               fn void Right() { }"#,
        );

    let resolved = resolve("main.erg", &loader);
    let complaints = messages(&resolved)
        .into_iter()
        .filter(|m| m.contains("cannot find module"))
        .count();
    assert_eq!(complaints, 1, "reported once, not once per importer");
}

/// The containment rule reaches import paths too: a script cannot reach code
/// the project does not own.
#[test]
fn an_escaping_import_is_refused() {
    let loader = MemoryLoader::new().with(
        "main.erg",
        r#"import "../../secrets.erg";
           fn void Main() { }"#,
    );
    let resolved = resolve("main.erg", &loader);
    assert!(messages(&resolved)
        .iter()
        .any(|m| m.contains("cannot leave the script root")));
}

#[test]
fn an_absolute_import_is_refused() {
    let loader = MemoryLoader::new().with(
        "main.erg",
        r#"import "/etc/passwd.erg";
           fn void Main() { }"#,
    );
    let resolved = resolve("main.erg", &loader);
    assert!(messages(&resolved)
        .iter()
        .any(|m| m.contains("cannot be absolute")));
}

/// Two spellings of one path are one module, or a diamond would compile it
/// twice.
#[test]
fn paths_are_normalised_before_they_are_compared() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "ai/steering.erg";
               import "ai//steering.erg";
               fn void Main() { }"#,
        )
        .with("ai/steering.erg", "fn void Steer() { }");

    let resolved = resolve("main.erg", &loader);
    assert!(!resolved.has_errors(), "{:?}", messages(&resolved));
    assert_eq!(order(&resolved), vec!["ai/steering.erg", "main.erg"]);
}

/// Syntax errors in an imported module surface with the rest, rather than the
/// resolver stopping at the first bad file.
#[test]
fn errors_from_every_module_are_collected() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "broken.erg";
               fn void Main() { }"#,
        )
        .with("broken.erg", "fn void ( ) { }");

    let resolved = resolve("main.erg", &loader);
    assert!(resolved.has_errors());
    assert!(
        resolved.get("main.erg").is_some(),
        "the good module is still resolved"
    );
}

/// Declarations share one global namespace today, so the same name in two
/// modules would silently give one meaning to both.
#[test]
fn a_name_declared_in_two_modules_is_reported() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "other.erg";
               struct Health { int current; }"#,
        )
        .with("other.erg", "struct Health { int hp; }");

    let resolved = resolve("main.erg", &loader);
    assert!(!resolved.has_errors(), "resolution itself succeeds");

    let clashes = report_name_clashes(&resolved);
    assert_eq!(clashes.len(), 1, "one clash: {clashes:?}");
    assert!(clashes[0].message.contains("Health"));
    assert!(clashes[0].message.contains("other.erg"));
}

#[test]
fn distinct_names_across_modules_do_not_clash() {
    let loader = MemoryLoader::new()
        .with(
            "main.erg",
            r#"import "other.erg";
               struct Health { int current; }"#,
        )
        .with("other.erg", "struct Loot { int value; }");

    let resolved = resolve("main.erg", &loader);
    assert!(report_name_clashes(&resolved).is_empty());
}

#[test]
fn a_bad_entry_path_is_reported() {
    let loader = MemoryLoader::new();
    let resolved = resolve("../escape.erg", &loader);
    assert!(messages(&resolved)
        .iter()
        .any(|m| m.contains("cannot leave the script root")));
}
