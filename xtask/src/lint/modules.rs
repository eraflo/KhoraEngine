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

//! Reads `mod` declarations and `#[cfg(test)]` marks: which files are test code.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::tree::{file_name, has_extension, normalize, read_text, walk_files};

/// Largest number of non-test lines a Rust file may hold.
pub const MAX_NON_TEST_LINES: usize = 700;

/// A `mod <name>;` item: a module whose body lives in another file.
pub struct ModDecl {
    /// The module's name.
    pub name: String,
    /// 1-based line of the `mod` item.
    pub line: usize,
    /// Under `#[cfg(test)]` or `#[cfg(all(test, …))]`.
    pub cfg_test: bool,
    /// The file a `#[path = "…"]` attribute names.
    pub path_attr: Option<String>,
}

/// Tells whether a directory between `src` and `path` is named `tests`.
pub fn under_tests_dir(src: &Path, path: &Path) -> bool {
    path.parent()
        .and_then(|dir| dir.strip_prefix(src).ok())
        .is_some_and(|dir| dir.components().any(|c| c.as_os_str() == "tests"))
}

/// Returns the files a declaration may resolve to.
pub fn decl_targets(file: &Path, decl: &ModDecl) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new(""));
    if let Some(path) = &decl.path_attr {
        return vec![normalize(&dir.join(path))];
    }
    let owner = match file_name(file).as_str() {
        "lib.rs" | "main.rs" | "mod.rs" => dir.to_path_buf(),
        _ => dir.join(file.file_stem().unwrap_or_default()),
    };
    vec![
        normalize(&owner.join(format!("{}.rs", decl.name))),
        normalize(&owner.join(&decl.name).join("mod.rs")),
    ]
}

/// Reads the out-of-line module declarations of `text`, with the attributes
/// stacked above them.
pub fn mod_decls(text: &str) -> Vec<ModDecl> {
    let mut decls = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (line_attrs, item) = split_outer_attrs(line);
        attrs.extend(line_attrs);
        if item.is_empty() {
            continue;
        }
        if let Some((name, true)) = mod_item(item) {
            decls.push(ModDecl {
                name,
                line: index + 1,
                cfg_test: attrs.iter().any(|attr| is_cfg_test(attr)),
                path_attr: attrs.iter().find_map(|attr| path_attr(attr)),
            });
        }
        attrs.clear();
    }
    decls
}

/// Splits leading `#[…]` attributes off a line; returns them and the rest.
pub fn split_outer_attrs(line: &str) -> (Vec<String>, &str) {
    let mut attrs = Vec::new();
    let mut rest = line;
    while rest.starts_with("#[") {
        let mut depth = 0usize;
        let mut end = None;
        for (index, ch) in rest.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => {
                attrs.push(rest[..end].to_owned());
                rest = rest[end..].trim_start();
            }
            None => {
                attrs.push(rest.to_owned());
                rest = "";
            }
        }
    }
    (attrs, rest)
}

/// Reads `[pub[(…)]] mod <name>`: returns the name, and whether the item is
/// out-of-line (`;`) rather than inline (`{`).
pub fn mod_item(item: &str) -> Option<(String, bool)> {
    let mut rest = item;
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            rest = &rest[rest.find(')')? + 1..];
        }
        rest = rest.trim_start();
    }
    let rest = rest.strip_prefix("mod ")?.trim_start();
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let after = rest[name.len()..].trim_start();
    Some((name, after.starts_with(';')))
}

/// Tells whether `attr` is `#[cfg(test)]`, or `#[cfg(all(…))]` with `test`
/// among its terms, in any place.
pub fn is_cfg_test(attr: &str) -> bool {
    let compact: String = attr.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact == "#[cfg(test)]" {
        return true;
    }
    let Some(terms) = compact
        .strip_prefix("#[cfg(all(")
        .and_then(|rest| rest.strip_suffix("))]"))
    else {
        return false;
    };
    let mut depth = 0usize;
    let mut start = 0;
    let mut found = false;
    for (at, ch) in terms.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                found |= &terms[start..at] == "test";
                start = at + 1;
            }
            _ => {}
        }
    }
    found || &terms[start..] == "test"
}

/// Returns the string of a `#[path = "…"]` attribute.
fn path_attr(attr: &str) -> Option<String> {
    let inner = attr.strip_prefix("#[")?.trim_start();
    let value = inner.strip_prefix("path")?.trim_start().strip_prefix('=')?;
    let start = value.find('"')? + 1;
    let len = value[start..].find('"')?;
    Some(value[start..start + len].to_owned())
}

/// Tells whether `text` carries the inner attribute `#![cfg(test)]` (or
/// `#![cfg(all(test, …))]`).
pub fn has_inner_cfg_test(text: &str) -> bool {
    text.lines().any(|line| {
        let compact: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        compact == "#![cfg(test)]" || compact.starts_with("#![cfg(all(test,")
    })
}

/// Returns the 0-based line of the `#[cfg(test)]` attribute opening the first
/// inline test module (`#[cfg(test)] … mod <name> {`), if any.
pub fn first_inline_test_module(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut start: Option<usize> = None;
    let mut cfg_test = false;
    for (index, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (attrs, item) = split_outer_attrs(line);
        if !attrs.is_empty() && start.is_none() {
            start = Some(index);
        }
        cfg_test |= attrs.iter().any(|attr| is_cfg_test(attr));
        if item.is_empty() {
            continue;
        }
        if cfg_test && matches!(mod_item(item), Some((_, false))) {
            return start;
        }
        start = None;
        cfg_test = false;
    }
    None
}

/// Counts the lines the size check reads: those above the first inline test
/// module, or the whole file.
pub fn non_test_lines(text: &str) -> usize {
    first_inline_test_module(text).unwrap_or_else(|| text.lines().count())
}

/// Lists the `.rs` files under `src` that are not test code, with their text.
///
/// Test code is a `tests.rs`, a file under a `tests/` folder, a
/// `#![cfg(test)]` file, or one declared by a `#[cfg(test)] mod` (a folder
/// module, with all it holds).
pub fn non_test_rust_files(src: &Path) -> Vec<(PathBuf, String)> {
    let files: Vec<(PathBuf, String)> = walk_files(src)
        .into_iter()
        .filter(|path| has_extension(path, "rs"))
        .map(|path| {
            let text = read_text(&path);
            (normalize(&path), text)
        })
        .collect();
    let mut test_files = BTreeSet::new();
    let mut test_folders = Vec::new();
    for (path, text) in &files {
        for decl in mod_decls(text).iter().filter(|decl| decl.cfg_test) {
            for target in decl_targets(path, decl) {
                if file_name(&target) == "mod.rs" {
                    if let Some(folder) = target.parent() {
                        test_folders.push(folder.to_path_buf());
                    }
                }
                test_files.insert(target);
            }
        }
    }
    files
        .into_iter()
        .filter(|(path, text)| {
            file_name(path) != "tests.rs"
                && !under_tests_dir(src, path)
                && !has_inner_cfg_test(text)
                && !test_files.contains(path)
                && !test_folders.iter().any(|folder| path.starts_with(folder))
        })
        .collect()
}
