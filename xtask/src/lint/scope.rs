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

//! Decides which checks read one file, as the whole-tree runs would.

use std::fs;
use std::path::{Path, PathBuf};

use super::modules::{decl_targets, has_inner_cfg_test, mod_decls, under_tests_dir};
use super::tree::{file_name, has_extension, normalize, read_text};

/// What the whole-tree checks make of one file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    /// The file lies in a Rust `src/` tree the checks scan: `crates/*/src`,
    /// `examples/*/src` or `xtask/src`.
    pub in_src: bool,
    /// The file is test code: `tests.rs`, under a `tests/` folder of its
    /// tree, `#![cfg(test)]`, or declared by a `#[cfg(test)] mod`.
    pub test_code: bool,
    /// The file is named `tests.rs` or lies under a `tests/` folder of its
    /// tree, which the size check leaves out.
    pub size_exempt: bool,
    /// The comment checks read the file: a `src/` tree, `crates/*/tests` or
    /// `xtask/tests`.
    pub comments: bool,
}

/// Returns how the checks read `path` (absolute, under `root`), whose text is
/// `text`.
pub fn scope_of(root: &Path, path: &Path, text: &str) -> Scope {
    let path = normalize(path);
    let Ok(relative) = path.strip_prefix(root) else {
        return Scope::default();
    };
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let (src, tests) = match parts.as_slice() {
        ["crates" | "examples", member, "src", ..] => {
            (Some(root.join(parts[0]).join(member).join("src")), false)
        }
        ["xtask", "src", ..] => (Some(root.join("xtask/src")), false),
        ["crates", _, "tests", ..] | ["xtask", "tests", ..] => (None, true),
        _ => (None, false),
    };
    let Some(src) = src.map(|src| normalize(&src)) else {
        return Scope {
            comments: tests,
            ..Scope::default()
        };
    };
    let size_exempt = file_name(&path) == "tests.rs" || under_tests_dir(&src, &path);
    let test_code = size_exempt || has_inner_cfg_test(text) || declared_as_test(&src, &path);
    Scope {
        in_src: true,
        test_code,
        size_exempt,
        comments: true,
    }
}

/// Tells whether a `#[cfg(test)] mod` in a module file between `src` and
/// `path` declares `path`, or a folder module holding it.
pub fn declared_as_test(src: &Path, path: &Path) -> bool {
    let mut dirs: Vec<PathBuf> = path
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(src))
        .map(Path::to_path_buf)
        .collect();
    dirs.reverse();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for file in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if !file.is_file() || !has_extension(&file, "rs") {
                continue;
            }
            let file = normalize(&file);
            for decl in mod_decls(&read_text(&file))
                .iter()
                .filter(|decl| decl.cfg_test)
            {
                let declares = decl_targets(&file, decl).iter().any(|target| {
                    *target == path
                        || (file_name(target) == "mod.rs"
                            && target
                                .parent()
                                .is_some_and(|folder| path.starts_with(folder)))
                });
                if declares {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(label: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "khora-lint-scope-{label}-{}-{:?}",
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

    #[test]
    fn a_file_declared_by_a_test_module_is_test_code_and_its_sibling_is_not() {
        let root = temp_tree(
            "declared",
            &[
                (
                    "crates/a/src/lib.rs",
                    "mod code;\n#[cfg(test)]\nmod checks;\n",
                ),
                ("crates/a/src/code.rs", "pub fn f() {}\n"),
                ("crates/a/src/checks.rs", "fn t() {}\n"),
                ("crates/a/src/checks/deep.rs", "fn t() {}\n"),
            ],
        );
        let scope = |file: &str| scope_of(&root, &root.join(file), "");
        let checks = scope("crates/a/src/checks.rs");
        let deep = scope("crates/a/src/checks/deep.rs");
        let code = scope("crates/a/src/code.rs");
        let _ = fs::remove_dir_all(&root);

        assert!(checks.test_code && !checks.size_exempt);
        assert!(deep.test_code);
        assert!(code.in_src && !code.test_code && code.comments);
    }

    #[test]
    fn an_integration_test_is_read_by_the_comment_checks_only() {
        let root = PathBuf::from("/repo");
        let scope = scope_of(&root, &root.join("crates/a/tests/it/main.rs"), "");
        assert_eq!(
            scope,
            Scope {
                comments: true,
                ..Scope::default()
            }
        );
        assert_eq!(
            scope_of(&root, &root.join("docs/x.rs"), ""),
            Scope::default()
        );
    }
}
