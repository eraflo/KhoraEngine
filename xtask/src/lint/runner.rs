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

//! Runs every per-file check on the files `cargo xtask lint-file` is given.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::allowlist::read_allowlist;
use super::boundaries::{runtime_reads, world_names, AGENTS_SRC, LANES_SRC};
use super::comments::{
    long_doc_summaries, long_inline_comments, missing_book_citations, plan_labels,
};
use super::finding::Finding;
use super::functions::{function_spans, MAX_FN_LINES};
use super::history::history_phrases;
use super::items::top_level_items;
use super::modules::{non_test_lines, MAX_NON_TEST_LINES};
use super::order::{
    constants_after_code, helpers_before_impls, inline_tests_in_mod_rs, items_after_tests,
    items_between_type_and_impls, misplaced_inline_tests,
};
use super::primary::extra_primary_types;
use super::scope::scope_of;
use super::summaries::summaries_without_a_verb;
use super::tree::{has_extension, normalize, rel, repo_root};

/// Top-level folders whose Rust files `lint-file` reads.
pub const LINTED_ROOTS: &[&str] = &["crates/", "xtask/", "examples/"];

/// Each check `lint-file` runs, with what to do about a finding.
pub const CHECK_HINTS: &[(&str, &str)] = &[
    (
        "non_test_code_under_700_lines",
        "split the file along its responsibilities",
    ),
    (
        "functions_under_200_lines",
        "split the function along what it does",
    ),
    (
        "tests_come_last_in_a_file",
        "move the code above the test module",
    ),
    (
        "a_file_holds_at_most_one_inline_test_module_named_tests",
        "keep one inline `#[cfg(test)] mod tests`",
    ),
    (
        "a_folder_mod_rs_holds_no_inline_test_module",
        "move the tests to the folder's `tests.rs`",
    ),
    (
        "constants_come_before_the_code",
        "move the constant above the first type, impl or function",
    ),
    (
        "private_helpers_come_after_the_primary_type_impls",
        "move the helper below the type's last impl",
    ),
    (
        "each_file_holds_one_primary_type",
        "split the file, one primary type per file",
    ),
    (
        "a_type_and_its_impls_are_contiguous",
        "move the item below the type's last impl, or to a file of its own",
    ),
    (
        "no_plan_labels_in_comments",
        "say what the code does; history goes in the commit",
    ),
    (
        "book_citations_resolve",
        "cite an existing page and heading",
    ),
    ("doc_summary_is_short", "one sentence, two lines at most"),
    (
        "inline_comments_are_short",
        "say why in four lines; split a function needing steps",
    ),
    (
        "comments_tell_no_history",
        "state what holds; history goes in the commit message",
    ),
    (
        "fn_summaries_start_with_a_verb",
        "open the summary on a verb in the third person (\"Returns…\")",
    ),
    (
        "lanes_never_name_world",
        "a lane reads Views from the bus, never the World",
    ),
    (
        "agents_never_read_runtime_field",
        "reach the runtime through `resource` or `locked`",
    ),
];

/// Returns the folder holding the workspace `path` belongs to: the nearest
/// ancestor with a `Cargo.toml` and an `xtask/Cargo.toml`.
pub fn find_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file() && dir.join("xtask/Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

/// Tells whether `lint-file` reads the file at `relative` (repo-relative,
/// `/` separators): a `.rs` file under `crates/`, `xtask/` or `examples/`.
pub fn is_linted(relative: &str) -> bool {
    relative.ends_with(".rs") && LINTED_ROOTS.iter().any(|root| relative.starts_with(root))
}

/// Runs every per-file check on `path` (absolute, under `root`), the ones the
/// whole-tree run would apply to it, allowlist aside.
///
/// # Errors
///
/// Fails when the file cannot be read.
pub fn lint_file(root: &Path, path: &Path) -> io::Result<Vec<Finding>> {
    let path = normalize(path);
    let relative = rel(root, &path);
    let text = fs::read_to_string(&path)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let scope = scope_of(root, &path, text);
    let mut findings = Vec::new();
    let mut push = |check: &'static str, key: String, (line, message): (usize, String)| {
        findings.push(Finding {
            path: relative.clone(),
            line,
            check,
            message,
            key,
        });
    };
    let file_key = relative.clone();

    if scope.in_src && !scope.size_exempt {
        let lines = non_test_lines(text);
        if lines > MAX_NON_TEST_LINES {
            let message = format!("{lines} non-test lines, over {MAX_NON_TEST_LINES}");
            push(
                "non_test_code_under_700_lines",
                file_key.clone(),
                (MAX_NON_TEST_LINES + 1, message),
            );
        }
    }
    if scope.in_src && !scope.test_code {
        for span in function_spans(text)
            .into_iter()
            .filter(|span| span.lines > MAX_FN_LINES)
        {
            let message = format!(
                "`fn {}` spans {} lines, over {MAX_FN_LINES}",
                span.name, span.lines
            );
            let key = format!("{relative}::{}", span.name);
            push("functions_under_200_lines", key, (span.line, message));
        }
        let items = top_level_items(text);
        if let Some(found) = items_after_tests(&items) {
            push("tests_come_last_in_a_file", file_key.clone(), found);
        }
        for found in misplaced_inline_tests(&items) {
            push(
                "a_file_holds_at_most_one_inline_test_module_named_tests",
                file_key.clone(),
                found,
            );
        }
        if let Some(found) = inline_tests_in_mod_rs(&relative, &items) {
            push(
                "a_folder_mod_rs_holds_no_inline_test_module",
                file_key.clone(),
                found,
            );
        }
        for found in constants_after_code(&items) {
            push("constants_come_before_the_code", file_key.clone(), found);
        }
        for found in helpers_before_impls(&items) {
            push(
                "private_helpers_come_after_the_primary_type_impls",
                file_key.clone(),
                found,
            );
        }
        if let Some(found) = extra_primary_types(&items) {
            push("each_file_holds_one_primary_type", file_key.clone(), found);
        }
        for found in items_between_type_and_impls(&items) {
            push(
                "a_type_and_its_impls_are_contiguous",
                file_key.clone(),
                found,
            );
        }
        for found in long_doc_summaries(text) {
            push("doc_summary_is_short", file_key.clone(), found);
        }
        for found in long_inline_comments(text) {
            push("inline_comments_are_short", file_key.clone(), found);
        }
        for found in summaries_without_a_verb(text) {
            push("fn_summaries_start_with_a_verb", file_key.clone(), found);
        }
    }
    if scope.comments {
        for (line, comment) in plan_labels(text) {
            push(
                "no_plan_labels_in_comments",
                file_key.clone(),
                (line, format!("plan label: {comment}")),
            );
        }
        for found in missing_book_citations(root, text) {
            push("book_citations_resolve", file_key.clone(), found);
        }
        for found in history_phrases(text) {
            push("comments_tell_no_history", file_key.clone(), found);
        }
    }
    if relative.starts_with(LANES_SRC) {
        for (line, code) in world_names(text) {
            let message = format!("names the World: {code}");
            push("lanes_never_name_world", file_key.clone(), (line, message));
        }
    }
    if relative.starts_with(AGENTS_SRC) {
        for found in runtime_reads(text) {
            push("agents_never_read_runtime_field", file_key.clone(), found);
        }
    }
    findings.sort();
    Ok(findings)
}

/// Runs [`lint_file`] on each linted file of `paths`, leaving out the
/// findings `root`'s allowlist lists; other files are skipped.
///
/// # Errors
///
/// Fails on the first linted file that cannot be read, naming it.
pub fn lint_files(root: &Path, paths: &[PathBuf]) -> io::Result<Vec<Finding>> {
    let (allowed, _) = read_allowlist(root);
    let mut findings = Vec::new();
    for path in paths {
        let path = normalize(path);
        if !is_linted(&rel(root, &path)) || !has_extension(&path, "rs") {
            continue;
        }
        let found = lint_file(root, &path).map_err(|error| {
            io::Error::new(error.kind(), format!("{}: {error}", path.display()))
        })?;
        findings.extend(found.into_iter().filter(|finding| {
            !allowed
                .get(finding.check)
                .is_some_and(|keys| keys.contains(&finding.key))
        }));
    }
    Ok(findings)
}

/// Returns the folder `lint-file` reads its allowlist from for `path`: the
/// workspace holding it, else the one this crate was built in.
pub fn root_for(path: &Path) -> PathBuf {
    find_root(path).unwrap_or_else(repo_root)
}

/// Returns what to do about a finding of `check`.
pub fn hint(check: &str) -> &'static str {
    CHECK_HINTS
        .iter()
        .find(|(name, _)| *name == check)
        .map_or("", |(_, hint)| hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(label: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "khora-lint-runner-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        for (file, text) in files {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create a temp folder");
            }
            fs::write(path, text).expect("write a temp file");
        }
        dir
    }

    const OUT_OF_ORDER: &str = "pub struct A;\nimpl A {}\nconst LATE: u8 = 1;\n";

    #[test]
    fn a_finding_is_printed_as_path_line_check_message() {
        let root = temp_tree(
            "printed",
            &[
                ("crates/a/src/lib.rs", OUT_OF_ORDER),
                ("xtask/Cargo.toml", ""),
                ("Cargo.toml", ""),
            ],
        );
        let found = lint_files(&root, &[root.join("crates/a/src/lib.rs")]);
        let _ = fs::remove_dir_all(&root);

        let found: Vec<String> = found
            .expect("lint")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                "crates/a/src/lib.rs:3: constants_come_before_the_code: `const LATE` after \
                 `struct A` (line 1)"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn an_allowlisted_file_and_a_file_outside_the_linted_roots_report_nothing() {
        let root = temp_tree(
            "allowed",
            &[
                ("crates/a/src/lib.rs", OUT_OF_ORDER),
                ("docs/x.rs", OUT_OF_ORDER),
                ("crates/a/src/notes.md", "used to"),
                (
                    "xtask/tests/layout_allow.txt",
                    "constants_come_before_the_code crates/a/src/lib.rs  # B0\n",
                ),
            ],
        );
        let paths = [
            root.join("crates/a/src/lib.rs"),
            root.join("docs/x.rs"),
            root.join("crates/a/src/notes.md"),
        ];
        let found = lint_files(&root, &paths);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(found.expect("lint"), Vec::new());
    }

    #[test]
    fn a_test_file_skips_the_order_checks_but_not_the_comment_checks() {
        let root = temp_tree(
            "tests",
            &[(
                "crates/a/src/tests.rs",
                "pub struct A;\nimpl A {}\n// This no longer holds.\nconst LATE: u8 = 1;\n",
            )],
        );
        let found = lint_file(&root, &root.join("crates/a/src/tests.rs"));
        let _ = fs::remove_dir_all(&root);

        let checks: Vec<&str> = found
            .expect("lint")
            .iter()
            .map(|finding| finding.check)
            .collect();
        assert_eq!(checks, vec!["comments_tell_no_history"]);
    }

    #[test]
    fn the_workspace_root_is_the_folder_holding_the_xtask_manifest() {
        let root = temp_tree(
            "root",
            &[
                ("Cargo.toml", ""),
                ("xtask/Cargo.toml", ""),
                ("crates/a/src/lib.rs", ""),
            ],
        );
        let found = find_root(&root.join("crates/a/src/lib.rs"));
        let _ = fs::remove_dir_all(&root);

        assert_eq!(found, Some(root));
        assert!(is_linted("xtask/src/main.rs"));
        assert!(!is_linted("docs/src/x.md") && !is_linted("crates/a/src/x.md"));
    }
}
