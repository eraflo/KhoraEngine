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

//! Checks the order of a file's top-level items: module doc, imports,
//! constants, the primary type, its impls, private helpers, tests.

use std::path::Path;

use super::allowlist::{add, Violations};
use super::items::{top_level_items, ItemKind, TopItem};
use super::modules::non_test_rust_files;
use super::tree::{rel, rust_src_roots};

/// Lists each non-test Rust file of `root`'s `src` trees with its top-level
/// items, sorted by path relative to `root`.
pub fn order_scanned_files(root: &Path) -> Vec<(String, Vec<TopItem>)> {
    let mut files = Vec::new();
    for src in rust_src_roots(root) {
        for (path, text) in non_test_rust_files(&src) {
            files.push((rel(root, &path), top_level_items(&text)));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Names the items that follow a file's test module, as one message at the
/// module's line; `None` when the test module comes last or there is none.
pub fn items_after_tests(items: &[TopItem]) -> Option<(usize, String)> {
    let at = items.iter().position(TopItem::is_test_module)?;
    let after: Vec<String> = items[at + 1..]
        .iter()
        .map(|item| format!("`{}` (line {})", item.label(), item.line))
        .collect();
    (!after.is_empty()).then(|| {
        let module = &items[at];
        let message = format!(
            "{} after the test module `{}` (line {}): {}",
            after.len(),
            module.name,
            module.line,
            after.join(", ")
        );
        (module.line, message)
    })
}

/// Names a file's inline test modules past the first, and each one not named
/// `tests`; a `#[cfg(test)] mod x;` declaration is not counted.
pub fn misplaced_inline_tests(items: &[TopItem]) -> Vec<(usize, String)> {
    let inline: Vec<&TopItem> = items
        .iter()
        .filter(|item| item.is_inline_test_module())
        .collect();
    let mut found = Vec::new();
    if inline.len() > 1 {
        let lines: Vec<String> = inline.iter().map(|item| item.line.to_string()).collect();
        found.push((
            inline[0].line,
            format!(
                "{} inline test modules (lines {})",
                inline.len(),
                lines.join(", ")
            ),
        ));
    }
    for item in inline.iter().filter(|item| item.name != "tests") {
        found.push((
            item.line,
            format!(
                "inline test module `{}` (line {}) is not named `tests`",
                item.name, item.line
            ),
        ));
    }
    found
}

/// Names the inline test modules of a folder's `mod.rs`, whose tests belong
/// in the folder's `tests.rs`; `None` for any other file.
pub fn inline_tests_in_mod_rs(path: &str, items: &[TopItem]) -> Option<(usize, String)> {
    if !path.ends_with("/mod.rs") {
        return None;
    }
    let inline: Vec<&TopItem> = items
        .iter()
        .filter(|item| item.is_inline_test_module())
        .collect();
    let first = inline.first()?.line;
    let listed: Vec<String> = inline
        .iter()
        .map(|item| format!("`{}` (line {})", item.name, item.line))
        .collect();
    let message = format!(
        "inline test module {} (move it to `tests.rs`, declared `#[cfg(test)] mod tests;`)",
        listed.join(", ")
    );
    Some((first, message))
}

/// Names each `const` or `static` declared after the file's first type,
/// `impl` or function; `const _` and `#[cfg(test)]` items are not read.
pub fn constants_after_code(items: &[TopItem]) -> Vec<(usize, String)> {
    let items: Vec<&TopItem> = items.iter().filter(|item| !item.cfg_test).collect();
    let Some(first) = items.iter().position(|item| {
        matches!(
            item.kind,
            ItemKind::Struct
                | ItemKind::Enum
                | ItemKind::Union
                | ItemKind::Trait
                | ItemKind::Impl
                | ItemKind::Fn
        )
    }) else {
        return Vec::new();
    };
    items[first + 1..]
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Const | ItemKind::Static) && item.name != "_")
        .map(|item| {
            let message = format!(
                "`{}` after `{}` (line {})",
                item.label(),
                items[first].label(),
                items[first].line
            );
            (item.line, message)
        })
        .collect()
}

/// Names each private free function between the file's first `struct` or
/// `enum` and the last `impl` of that type; `#[cfg(test)]` items are not read.
pub fn helpers_before_impls(items: &[TopItem]) -> Vec<(usize, String)> {
    let items: Vec<&TopItem> = items.iter().filter(|item| !item.cfg_test).collect();
    let Some(primary) = items
        .iter()
        .position(|item| matches!(item.kind, ItemKind::Struct | ItemKind::Enum))
    else {
        return Vec::new();
    };
    let name = &items[primary].name;
    let Some(last_impl) = items
        .iter()
        .rposition(|item| item.kind == ItemKind::Impl && &item.name == name)
    else {
        return Vec::new();
    };
    (items.iter().take(last_impl).skip(primary + 1))
        .filter(|item| item.kind == ItemKind::Fn && item.private)
        .map(|item| {
            let message = format!(
                "`{}` between `{}` (line {}) and its last impl (line {})",
                item.label(),
                items[primary].label(),
                items[primary].line,
                items[last_impl].line
            );
            (item.line, message)
        })
        .collect()
}

/// Names each function, type, trait, constant, static or alias standing
/// between a type and its last `impl`.
///
/// Each item is named once, after the first type it splits from its impls.
/// `const _` (an assertion about the type above it), macro calls and
/// `#[cfg(test)]` items pass.
pub fn items_between_type_and_impls(items: &[TopItem]) -> Vec<(usize, String)> {
    let items: Vec<&TopItem> = items.iter().filter(|item| !item.cfg_test).collect();
    let mut found: Vec<(usize, String)> = Vec::new();
    for (at, ty) in items.iter().enumerate() {
        let is_type = matches!(
            ty.kind,
            ItemKind::Struct
                | ItemKind::Enum
                | ItemKind::Union
                | ItemKind::Trait
                | ItemKind::TypeAlias
        );
        if !is_type || ty.name.is_empty() {
            continue;
        }
        let Some(last) = items
            .iter()
            .rposition(|item| item.kind == ItemKind::Impl && item.name == ty.name)
        else {
            continue;
        };
        for item in items.iter().take(last).skip(at + 1) {
            let splits = matches!(
                item.kind,
                ItemKind::Fn
                    | ItemKind::Struct
                    | ItemKind::Enum
                    | ItemKind::Union
                    | ItemKind::Trait
                    | ItemKind::TypeAlias
                    | ItemKind::Static
            ) || (item.kind == ItemKind::Const && item.name != "_");
            if splits && !found.iter().any(|(line, _)| *line == item.line) {
                let message = format!(
                    "`{}` between `{}` (line {}) and its impl (line {})",
                    item.label(),
                    ty.label(),
                    ty.line,
                    items[last].line
                );
                found.push((item.line, message));
            }
        }
    }
    found.sort();
    found
}

/// Collects every non-test file under `root` whose items run past its test
/// module, as `<path>: …`, sorted.
pub fn tests_last_problems(root: &Path) -> Vec<String> {
    order_scanned_files(root)
        .into_iter()
        .filter_map(|(path, items)| {
            items_after_tests(&items).map(|(_, message)| format!("{path}: {message}"))
        })
        .collect()
}

/// Collects every non-test file under `root` holding several inline test
/// modules, or one not named `tests`, as `<path>: …`, sorted.
pub fn inline_test_module_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, items) in order_scanned_files(root) {
        let found: Vec<String> = misplaced_inline_tests(&items)
            .into_iter()
            .map(|(_, message)| message)
            .collect();
        if !found.is_empty() {
            problems.push(format!("{path}: {}", found.join("; ")));
        }
    }
    problems
}

/// Collects every non-test folder `mod.rs` under `root` holding an inline
/// test module, as `<path>: …`, sorted.
pub fn mod_rs_inline_test_problems(root: &Path) -> Vec<String> {
    order_scanned_files(root)
        .into_iter()
        .filter_map(|(path, items)| {
            inline_tests_in_mod_rs(&path, &items).map(|(_, message)| format!("{path}: {message}"))
        })
        .collect()
}

/// Collects, per non-test file under `root`, each constant declared after
/// the code ([`constants_after_code`]).
pub fn constants_first_violations(root: &Path) -> Violations {
    per_file_violations(root, constants_after_code)
}

/// Collects, per non-test file under `root`, each private helper between the
/// primary type and its impls ([`helpers_before_impls`]).
pub fn helpers_after_impls_violations(root: &Path) -> Violations {
    per_file_violations(root, helpers_before_impls)
}

/// Collects, per non-test file under `root`, each item between a type and
/// its impls ([`items_between_type_and_impls`]).
pub fn contiguous_impl_violations(root: &Path) -> Violations {
    per_file_violations(root, items_between_type_and_impls)
}

/// Runs `check` on each order-scanned file, keyed by its path, each finding
/// as `line N: …`.
fn per_file_violations(root: &Path, check: fn(&[TopItem]) -> Vec<(usize, String)>) -> Violations {
    let mut violations = Violations::new();
    for (path, items) in order_scanned_files(root) {
        for (line, message) in check(&items) {
            add(
                &mut violations,
                path.clone(),
                format!("line {line}: {message}"),
            );
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helper_struct_between_a_type_and_its_impl_is_named() {
        let text = "pub struct IndexBuilder;\nstruct Scratch;\nimpl IndexBuilder {}\n";
        let found = items_between_type_and_impls(&top_level_items(text));
        assert_eq!(
            found,
            vec![(
                2,
                "`struct Scratch` between `struct IndexBuilder` (line 1) and its impl (line 3)"
                    .to_owned()
            )]
        );
    }

    #[test]
    fn a_type_followed_by_its_impls_then_helpers_passes() {
        let text =
            "pub struct A;\nimpl A {}\nimpl Default for A {\n    fn default() -> Self { A }\n}\n\
                    const _: () = ();\nfn helper() {}\nstruct Other;\n#[cfg(test)]\nmod tests {}\n";
        assert!(items_between_type_and_impls(&top_level_items(text)).is_empty());
    }

    #[test]
    fn a_test_only_item_or_an_assertion_between_a_type_and_its_impl_passes() {
        let text = "pub struct A;\nconst _: () = ();\n#[cfg(test)]\nfn probe() {}\nimpl A {}\n";
        assert!(items_between_type_and_impls(&top_level_items(text)).is_empty());
    }

    #[test]
    fn an_item_splitting_two_types_from_their_impls_is_named_once() {
        let text = "pub struct A;\npub struct B;\nfn f() {}\nimpl A {}\nimpl B {}\n";
        let lines: Vec<usize> = items_between_type_and_impls(&top_level_items(text))
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, vec![2, 3]);
    }
}
