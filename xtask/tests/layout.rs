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

//! The workspace's layout conventions, written as tests.
//!
//! Each test reads the tree from the repository root (the parent of this
//! crate's manifest directory) and fails with the **complete, sorted** list of
//! violations, so a failing run is the to-do list.
//!
//! Rust checks scan `crates/*/src`, `hub/src`, `xtask/src` and
//! `examples/*/src`. Every walk skips `target/`, `.git/`, `.codegraph/` and
//! `node_modules/`.
//!
//! Every check is a **ratchet**. A check reports its violations keyed by a
//! stable repo-relative path; `xtask/tests/layout_allow.txt` lists the known
//! ones, one per line: `<check-name> <repo-relative path>  # reason`, where the
//! reason names the batch that removes the entry (`B0`–`B6`, or an Ergon
//! spec). A check fails when
//!
//! - a violation's path is not listed (no new debt),
//! - a listed path no longer violates the check (stale: remove the line in the
//!   commit that fixes it — the list only shrinks),
//! - the allowlist itself is malformed (unknown check, missing or batch-less
//!   reason, duplicate line) — reported by every check.
//!
//! A check with no listed entries is strict.
//!
//! Checks that need `git` (`no_tracked_ignored_files`,
//! `no_duplicate_asset_files`) return early without asserting when `git`
//! cannot be run in the repository.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Directory names never walked into, wherever they appear.
const SKIP_DIRS: &[&str] = &["target", ".git", ".codegraph", "node_modules"];

/// Largest number of non-test lines a Rust file may hold.
const MAX_NON_TEST_LINES: usize = 700;

/// Every check, as named in the allowlist (each is its test's name).
const RATCHETED_CHECKS: &[&str] = &[
    "no_file_and_folder_module_pair",
    "no_orphan_rust_files",
    "test_files_follow_convention",
    "non_test_code_under_700_lines",
    "wgsl_only_in_shader_tree",
    "every_agent_folder_has_agent_rs",
    "lanes_mirror_agents",
    "no_tracked_ignored_files",
    "relative_links_resolve",
    "no_duplicate_asset_files",
];

/// Words one of which an allowlist reason must contain: the batch that
/// removes the entry.
const BATCH_WORDS: &[&str] = &["B0", "B1", "B2", "B3", "B4", "B5", "B6", "Ergon"];

/// Repository-relative path of the ratchet allowlist.
const ALLOWLIST: &str = "xtask/tests/layout_allow.txt";

/// The one directory `.wgsl` files may live in.
const SHADER_TREE: &str = "crates/khora-infra/src/graphics/shader/shaders/";

/// Markdown trees that are scratch: neither scanned nor link targets.
const SCRATCH_DOCS: &[&str] = &["docs/plans/", "docs/research/"];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// No `foo.rs` next to a `foo/` directory under any Rust `src/` tree: a module
/// with children is a folder module (`foo/mod.rs`), never a file plus a folder.
/// Keyed by the `foo.rs` file.
#[test]
fn no_file_and_folder_module_pair() {
    let root = repo_root();
    let mut violations = Violations::new();
    for src in rust_src_roots(&root) {
        for dir in walk_dirs(&src) {
            let Some(name) = dir.file_name() else {
                continue;
            };
            let sibling = dir.with_file_name(format!("{}.rs", name.to_string_lossy()));
            if sibling.is_file() {
                add(
                    &mut violations,
                    rel(&root, &sibling),
                    format!("next to {}/", rel(&root, &dir)),
                );
            }
        }
    }
    ratchet(
        "no_file_and_folder_module_pair",
        "a module is both a file and a folder (make it `<name>/mod.rs`)",
        &violations,
    );
}

/// Every `.rs` under a Rust `src/` tree is `lib.rs`, `main.rs`, `mod.rs`, lives
/// under `src/bin/`, or is declared by a `mod <name>;` item (any visibility,
/// any attribute, including `#[cfg(test)]`) in the module file that owns its
/// directory — `foo.rs` or `foo/mod.rs` for a file in `foo/`, `lib.rs` or
/// `main.rs` for a file directly in `src/`. A `#[path = "…"]` attribute
/// declares the file it points at, resolved from the declaring file's
/// directory.
#[test]
fn no_orphan_rust_files() {
    let root = repo_root();
    let files = rust_files(&root);
    let mut declared = BTreeSet::new();
    for file in &files {
        for decl in &file.decls {
            declared.extend(decl_targets(&file.path, decl));
        }
    }
    let mut violations = Violations::new();
    for file in &files {
        if !is_crate_or_folder_root(file) && !declared.contains(&file.path) {
            add(
                &mut violations,
                rel(&root, &file.path),
                "declared by no `mod` item".to_owned(),
            );
        }
    }
    ratchet(
        "no_orphan_rust_files",
        "Rust files no module declares",
        &violations,
    );
}

/// A file that holds only tests is named `tests.rs` or lives under a `tests/`
/// directory; no test module is reached through `#[path = "…"]`; no file is
/// named `*_tests.rs`.
///
/// "Holds only tests" is decided by a simple heuristic, any of:
/// - its parent declares it with `#[cfg(test)] mod <name>;`;
/// - it carries the inner attribute `#![cfg(test)]`;
/// - everything before its first inline `#[cfg(test)] mod <name> { … }` is
///   blank lines, comments, inner attributes and private `use` items.
///
/// `#[cfg(test)]` also matches `#[cfg(all(test, …))]`, never `not(test)`.
///
/// Keyed by the offending file; a `#[path]` finding by the file carrying the
/// attribute.
#[test]
fn test_files_follow_convention() {
    let root = repo_root();
    let files = rust_files(&root);
    let mut violations = Violations::new();
    let mut test_only = BTreeSet::new();

    for file in &files {
        for decl in &file.decls {
            if !decl.cfg_test {
                continue;
            }
            if decl.path_attr.is_some() {
                add(
                    &mut violations,
                    rel(&root, &file.path),
                    format!(
                        "line {}: test module `{}` is reached through #[path]",
                        decl.line, decl.name
                    ),
                );
            }
            test_only.extend(
                decl_targets(&file.path, decl)
                    .into_iter()
                    .filter(|target| target.is_file()),
            );
        }
        if has_inner_cfg_test(&file.text) || preamble_then_tests(&file.text) {
            test_only.insert(file.path.clone());
        }
        if file_name(&file.path).ends_with("_tests.rs") {
            add(
                &mut violations,
                rel(&root, &file.path),
                "named `*_tests.rs` (use `tests.rs` or a `tests/` directory)".to_owned(),
            );
        }
    }

    for file in &files {
        if test_only.contains(&file.path)
            && file_name(&file.path) != "tests.rs"
            && !under_tests_dir(&file.src, &file.path)
        {
            add(
                &mut violations,
                rel(&root, &file.path),
                "holds only tests but is neither `tests.rs` nor under `tests/`".to_owned(),
            );
        }
    }
    ratchet(
        "test_files_follow_convention",
        "test files off the convention",
        &violations,
    );
}

/// A Rust file holds at most 700 non-test lines. Non-test lines are
/// the lines before the first `#[cfg(test)]` attribute that opens an inline
/// module (`#[cfg(test)] mod tests { … }`), or the whole file when there is
/// none — a `#[cfg(test)] mod tests;` declaration does not end the count.
/// Files named `tests.rs` or under a `tests/` directory are exempt.
///
/// Keyed by the file.
#[test]
fn non_test_code_under_700_lines() {
    let root = repo_root();
    let mut violations = Violations::new();
    for file in rust_files(&root) {
        if file_name(&file.path) == "tests.rs" || under_tests_dir(&file.src, &file.path) {
            continue;
        }
        let lines = non_test_lines(&file.text);
        if lines > MAX_NON_TEST_LINES {
            add(
                &mut violations,
                rel(&root, &file.path),
                format!("{lines} non-test lines"),
            );
        }
    }
    ratchet(
        "non_test_code_under_700_lines",
        &format!("files over {MAX_NON_TEST_LINES} non-test lines (split them)"),
        &violations,
    );
}

/// Every `.wgsl` file in the repository lives under
/// `crates/khora-infra/src/graphics/shader/shaders/`. Keyed by the file.
#[test]
fn wgsl_only_in_shader_tree() {
    let root = repo_root();
    let mut violations = Violations::new();
    for path in walk_files(&root) {
        let path = rel(&root, &path);
        if path.ends_with(".wgsl") && !path.starts_with(SHADER_TREE) {
            add(&mut violations, path, "outside the shader tree".to_owned());
        }
    }
    ratchet(
        "wgsl_only_in_shader_tree",
        &format!("WGSL outside {SHADER_TREE}"),
        &violations,
    );
}

/// Each `crates/khora-agents/src/*_agent/` folder holds its agent in
/// `agent.rs`. Keyed by the agent folder.
#[test]
fn every_agent_folder_has_agent_rs() {
    let root = repo_root();
    let agents = root.join("crates/khora-agents/src");
    let mut violations = Violations::new();
    for domain in domain_folders(&agents, "_agent") {
        if !agents.join(format!("{domain}_agent/agent.rs")).is_file() {
            add(
                &mut violations,
                format!("crates/khora-agents/src/{domain}_agent"),
                "has no agent.rs".to_owned(),
            );
        }
    }
    ratchet(
        "every_agent_folder_has_agent_rs",
        "agent folders without agent.rs",
        &violations,
    );
}

/// The agent is the unit: each `khora-agents/src/<domain>_agent/` has a
/// `khora-lanes/src/<domain>_lane/`, and each lane folder has its agent.
/// Keyed by the missing folder.
#[test]
fn lanes_mirror_agents() {
    let root = repo_root();
    let agents = domain_folders(&root.join("crates/khora-agents/src"), "_agent");
    let lanes = domain_folders(&root.join("crates/khora-lanes/src"), "_lane");
    let mut violations = Violations::new();
    for domain in agents.difference(&lanes) {
        add(
            &mut violations,
            format!("crates/khora-lanes/src/{domain}_lane"),
            format!("missing, although crates/khora-agents/src/{domain}_agent exists"),
        );
    }
    for domain in lanes.difference(&agents) {
        add(
            &mut violations,
            format!("crates/khora-agents/src/{domain}_agent"),
            format!("missing, although crates/khora-lanes/src/{domain}_lane exists"),
        );
    }
    ratchet(
        "lanes_mirror_agents",
        "lanes and agents out of step",
        &violations,
    );
}

/// No tracked file is matched by `.gitignore`
/// (`git ls-files -ci --exclude-standard` is empty). Keyed by the file.
#[test]
fn no_tracked_ignored_files() {
    let root = repo_root();
    let Some(listed) = git(&root, &["ls-files", "-z", "-ci", "--exclude-standard"]) else {
        return; // git is not runnable here: nothing to check against.
    };
    let mut violations = Violations::new();
    for path in split_nul(&listed) {
        add(&mut violations, path, "tracked but ignored".to_owned());
    }
    ratchet(
        "no_tracked_ignored_files",
        "tracked files that .gitignore ignores (git rm --cached them)",
        &violations,
    );
}

/// Every relative link in `README.md`, `docs/README.md`, `docs/src/**/*.md`
/// and `.agent/**/*.md` points at an existing file or directory.
///
/// A link is a Markdown `[text](target)` / `![alt](target)`, or an HTML
/// `src="…"` / `href="…"` attribute. Skipped: `scheme://` and `mailto:`
/// targets, pure `#anchor` links, links inside fenced code blocks or inline
/// code spans, and anything under `docs/plans/` or `docs/research/`. The
/// `#anchor` and `?query` parts are stripped and never checked; the rest is
/// resolved from the linking file's directory (a leading `/` from the
/// repository root). Keyed by the linking file.
#[test]
fn relative_links_resolve() {
    let root = repo_root();
    let mut sources = Vec::new();
    for single in ["README.md", "docs/README.md"] {
        let path = root.join(single);
        if path.is_file() {
            sources.push(path);
        }
    }
    for tree in ["docs/src", ".agent"] {
        sources.extend(
            walk_files(&root.join(tree))
                .into_iter()
                .filter(|path| has_extension(path, "md")),
        );
    }

    let mut violations = Violations::new();
    for source in sources {
        let source_rel = rel(&root, &source);
        if is_scratch_doc(&source_rel) {
            continue;
        }
        let text = read_text(&source);
        for (line, target) in markdown_links(&text) {
            let Some(path) = link_path(&target) else {
                continue;
            };
            let resolved = match path.strip_prefix('/') {
                Some(from_root) => normalize(&root.join(from_root)),
                None => normalize(&source.parent().unwrap_or(&root).join(&path)),
            };
            if is_scratch_doc(&rel(&root, &resolved)) {
                continue;
            }
            if !resolved.exists() {
                add(
                    &mut violations,
                    source_rel.clone(),
                    format!("line {line}: {target}"),
                );
            }
        }
    }
    ratchet(
        "relative_links_resolve",
        "relative links that do not resolve",
        &violations,
    );
}

/// No two files tracked by git under an `assets/` directory (a path containing
/// `/assets/`) have identical bytes. In each group of identical files the first
/// path in sorted order is the one kept; the others are keyed.
#[test]
fn no_duplicate_asset_files() {
    let root = repo_root();
    let Some(listed) = git(&root, &["ls-files", "-z"]) else {
        return; // git is not runnable here: nothing to check against.
    };
    let mut by_len: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for path in split_nul(&listed).filter(|path| path.contains("/assets/")) {
        if let Ok(meta) = fs::metadata(root.join(&path)) {
            by_len.entry(meta.len()).or_default().push(path);
        }
    }

    let mut violations = Violations::new();
    for paths in by_len.values().filter(|paths| paths.len() > 1) {
        let mut by_bytes: HashMap<Vec<u8>, Vec<String>> = HashMap::new();
        for path in paths {
            if let Ok(bytes) = fs::read(root.join(path)) {
                by_bytes.entry(bytes).or_default().push(path.clone());
            }
        }
        for mut same in by_bytes.into_values().filter(|same| same.len() > 1) {
            same.sort();
            let (kept, copies) = same.split_at(1);
            for copy in copies {
                add(
                    &mut violations,
                    copy.clone(),
                    format!("same bytes as {}", kept[0]),
                );
            }
        }
    }
    ratchet(
        "no_duplicate_asset_files",
        "byte-identical asset files",
        &violations,
    );
}

// ---------------------------------------------------------------------------
// Tree walking
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("{} has no parent", env!("CARGO_MANIFEST_DIR")))
}

/// `crates/*/src`, `examples/*/src`, `hub/src`, `xtask/src` — those that exist.
fn rust_src_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for group in ["crates", "examples"] {
        for member in child_dirs(&root.join(group)) {
            let src = member.join("src");
            if src.is_dir() {
                roots.push(src);
            }
        }
    }
    for single in ["hub/src", "xtask/src"] {
        let src = root.join(single);
        if src.is_dir() {
            roots.push(src);
        }
    }
    roots.sort();
    roots
}

fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !is_skipped(path))
        .collect();
    dirs.sort();
    dirs
}

fn is_skipped(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| SKIP_DIRS.iter().any(|skip| name == *skip))
}

/// Every file under `dir`, recursively, sorted.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.is_dir() {
                if !is_skipped(&path) {
                    stack.push(path);
                }
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Every directory strictly below `dir`, recursively, sorted.
fn walk_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut stack = child_dirs(dir);
    while let Some(current) = stack.pop() {
        stack.extend(child_dirs(&current));
        dirs.push(current);
    }
    dirs.sort();
    dirs
}

/// Domain names of the direct children of `dir` named `<domain><suffix>`.
fn domain_folders(dir: &Path, suffix: &str) -> BTreeSet<String> {
    child_dirs(dir)
        .iter()
        .filter_map(|path| file_name(path).strip_suffix(suffix).map(str::to_owned))
        .collect()
}

/// Repository-relative path with `/` separators.
fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|found| found == ext)
}

/// Resolves `.` and `..` lexically, without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn read_text(path: &Path) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    text.strip_prefix('\u{feff}')
        .map(str::to_owned)
        .unwrap_or(text)
}

// ---------------------------------------------------------------------------
// git
// ---------------------------------------------------------------------------

/// Stdout of `git <args>` run at the repository root, or `None` when git
/// cannot be started or refuses (no git binary, not a repository).
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn split_nul(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Rust modules
// ---------------------------------------------------------------------------

struct RustFile {
    /// The `src/` tree the file belongs to.
    src: PathBuf,
    path: PathBuf,
    text: String,
    decls: Vec<ModDecl>,
}

/// A `mod <name>;` item: a module whose body lives in another file.
struct ModDecl {
    name: String,
    /// 1-based line of the `mod` item.
    line: usize,
    cfg_test: bool,
    path_attr: Option<String>,
}

fn rust_files(root: &Path) -> Vec<RustFile> {
    let mut files = Vec::new();
    for src in rust_src_roots(root) {
        for path in walk_files(&src) {
            if has_extension(&path, "rs") {
                let text = read_text(&path);
                let decls = mod_decls(&text);
                files.push(RustFile {
                    src: src.clone(),
                    path: normalize(&path),
                    text,
                    decls,
                });
            }
        }
    }
    files
}

/// `lib.rs`, `main.rs`, `mod.rs`, or anything under `src/bin/`.
fn is_crate_or_folder_root(file: &RustFile) -> bool {
    matches!(
        file_name(&file.path).as_str(),
        "lib.rs" | "main.rs" | "mod.rs"
    ) || file.path.starts_with(file.src.join("bin"))
}

/// True when a directory between `src` and `path` is named `tests`.
fn under_tests_dir(src: &Path, path: &Path) -> bool {
    path.parent()
        .and_then(|dir| dir.strip_prefix(src).ok())
        .is_some_and(|dir| dir.components().any(|c| c.as_os_str() == "tests"))
}

/// The files a declaration may resolve to.
fn decl_targets(file: &Path, decl: &ModDecl) -> Vec<PathBuf> {
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

/// Out-of-line module declarations, with the attributes stacked above them.
fn mod_decls(text: &str) -> Vec<ModDecl> {
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
fn split_outer_attrs(line: &str) -> (Vec<String>, &str) {
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

/// `[pub[(…)]] mod <name>` → the name, and whether the item is out-of-line
/// (`;`) rather than inline (`{`).
fn mod_item(item: &str) -> Option<(String, bool)> {
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

/// `#[cfg(test)]` or `#[cfg(all(test, …))]`.
fn is_cfg_test(attr: &str) -> bool {
    let compact: String = attr.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact == "#[cfg(test)]" || compact.starts_with("#[cfg(all(test,")
}

/// The string of a `#[path = "…"]` attribute.
fn path_attr(attr: &str) -> Option<String> {
    let inner = attr.strip_prefix("#[")?.trim_start();
    let value = inner.strip_prefix("path")?.trim_start().strip_prefix('=')?;
    let start = value.find('"')? + 1;
    let len = value[start..].find('"')?;
    Some(value[start..start + len].to_owned())
}

fn has_inner_cfg_test(text: &str) -> bool {
    text.lines().any(|line| {
        let compact: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        compact == "#![cfg(test)]" || compact.starts_with("#![cfg(all(test,")
    })
}

/// 0-based line of the `#[cfg(test)]` attribute opening the first inline test
/// module (`#[cfg(test)] … mod <name> {`), if any.
fn first_inline_test_module(text: &str) -> Option<usize> {
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

/// Lines counted by the size ratchet.
fn non_test_lines(text: &str) -> usize {
    first_inline_test_module(text).unwrap_or_else(|| text.lines().count())
}

/// True when the file opens an inline test module after nothing but blank
/// lines, comments, inner attributes and private `use` items.
fn preamble_then_tests(text: &str) -> bool {
    let Some(first) = first_inline_test_module(text) else {
        return false;
    };
    let mut in_use = false;
    for raw in text.lines().take(first) {
        let line = raw.trim();
        if in_use {
            in_use = !line.ends_with(';');
            continue;
        }
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with("#![")
        {
            continue;
        }
        if line.starts_with("use ") {
            in_use = !line.ends_with(';');
            continue;
        }
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Allowlist
// ---------------------------------------------------------------------------

/// Violations of one check: repo-relative key → what is wrong there.
type Violations = BTreeMap<String, BTreeSet<String>>;

fn add(violations: &mut Violations, key: String, detail: String) {
    violations.entry(key).or_default().insert(detail);
}

/// Compares a check's violations with its allowlist entries and fails with
/// every new violation, every stale entry and every allowlist problem.
fn ratchet(check: &str, what: &str, violations: &Violations) {
    assert!(
        RATCHETED_CHECKS.contains(&check),
        "`{check}` is missing from RATCHETED_CHECKS"
    );
    let (entries, mut problems) = read_allowlist(&repo_root());
    let allowed = entries.get(check).cloned().unwrap_or_default();

    for (key, details) in violations {
        if !allowed.contains(key) {
            let details: Vec<&str> = details.iter().map(String::as_str).collect();
            problems.push(format!("new: {key}: {}", details.join("; ")));
        }
    }
    for key in &allowed {
        if !violations.contains_key(key) {
            problems.push(format!(
                "stale: {key} no longer violates this check (remove its line from {ALLOWLIST})"
            ));
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "{check}: {what} — {} problems (fix a new violation, or list it in {ALLOWLIST} as \
         `{check} <path>  # reason naming the batch`):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// Entries per check, and the problems found while reading the file.
fn read_allowlist(root: &Path) -> (BTreeMap<String, BTreeSet<String>>, Vec<String>) {
    let mut entries: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut problems = Vec::new();
    let Ok(text) = fs::read_to_string(root.join(ALLOWLIST)) else {
        return (entries, problems);
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    for (index, raw) in text.lines().enumerate() {
        let (line, reason) = raw.split_once('#').unwrap_or((raw, ""));
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [check, path] = fields.as_slice() else {
            problems.push(format!(
                "{ALLOWLIST}:{}: expected `<check-name> <path>  # reason`, got `{raw}`",
                index + 1
            ));
            continue;
        };
        if !RATCHETED_CHECKS.contains(check) {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{check}` is not a layout check",
                index + 1
            ));
            continue;
        }
        let names_batch = reason
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| BATCH_WORDS.contains(&word));
        if !names_batch {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{path}` needs a `# reason` naming the batch that removes it \
                 (one of {BATCH_WORDS:?})",
                index + 1
            ));
        }
        if !entries
            .entry((*check).to_owned())
            .or_default()
            .insert((*path).to_owned())
        {
            problems.push(format!(
                "{ALLOWLIST}:{}: `{path}` is listed twice",
                index + 1
            ));
        }
    }
    (entries, problems)
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn is_scratch_doc(rel_path: &str) -> bool {
    SCRATCH_DOCS.iter().any(|dir| rel_path.starts_with(dir))
}

/// `(1-based line, raw target)` of every link outside code.
fn markdown_links(text: &str) -> Vec<(usize, String)> {
    let mut links = Vec::new();
    let mut fence: Option<char> = None;
    for (index, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        let marker = trimmed.chars().next().filter(|ch| *ch == '`' || *ch == '~');
        if let Some(ch) = marker {
            if trimmed.starts_with(&ch.to_string().repeat(3)) {
                fence = match fence {
                    None => Some(ch),
                    Some(open) if open == ch => None,
                    other => other,
                };
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let line = strip_code_spans(raw);
        for target in inline_link_targets(&line)
            .into_iter()
            .chain(html_link_targets(&line))
            .chain(reference_definition_target(&line))
        {
            links.push((index + 1, target));
        }
    }
    links
}

/// The line with every `` `code` `` span blanked out.
fn strip_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
        } else if in_code {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

/// Targets of `[text](target)` on one line.
fn inline_link_targets(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find("](") {
        let after = &rest[open + 2..];
        let mut depth = 1usize;
        let mut end = None;
        for (index, ch) in after.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };
        let inside = after[..end].trim();
        let target = match inside.strip_prefix('<') {
            Some(angled) => angled.split('>').next().unwrap_or(""),
            None => inside.split_whitespace().next().unwrap_or(""),
        };
        if !target.is_empty() {
            targets.push(target.to_owned());
        }
        rest = &after[end..];
    }
    targets
}

/// The target of a reference-style definition, `[label]: target`, indented
/// by at most three spaces as CommonMark allows. Footnotes (`[^x]:`) are not
/// links.
fn reference_definition_target(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line.trim_start().strip_prefix('[')?;
    let (label, after) = rest.split_once("]:")?;
    if label.is_empty() || label.starts_with('^') {
        return None;
    }
    let value = after.trim_start();
    let target = match value.strip_prefix('<') {
        Some(angled) => angled.split('>').next().unwrap_or(""),
        None => value.split_whitespace().next().unwrap_or(""),
    };
    (!target.is_empty()).then(|| target.to_owned())
}

/// Values of HTML `src` and `href` attributes on one line, single- or
/// double-quoted.
fn html_link_targets(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for (attr, quote) in [
        ("src=\"", '"'),
        ("href=\"", '"'),
        ("src='", '\''),
        ("href='", '\''),
    ] {
        let mut rest = line;
        while let Some(at) = rest.find(attr) {
            let preceded_by_space = at == 0
                || rest[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace);
            let value = &rest[at + attr.len()..];
            let Some(len) = value.find(quote) else {
                break;
            };
            if preceded_by_space && !value[..len].is_empty() {
                targets.push(value[..len].to_owned());
            }
            rest = &value[len..];
        }
    }
    targets
}

/// The file part of a relative link target, or `None` for links not checked.
fn link_path(target: &str) -> Option<String> {
    if target.starts_with('#') || target.starts_with("mailto:") || target.contains("://") {
        return None;
    }
    let path = target.split(['#', '?']).next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    Some(path.replace("%20", " "))
}

// ---------------------------------------------------------------------------
// Test-breaker findings (B0/B1, round 1)
// ---------------------------------------------------------------------------

/// A relative link must resolve in a fresh clone, not only in this working
/// tree: its target has to be tracked by git. `relative_links_resolve` checks
/// `exists()` on disk, so a link to a generated wrapper that `.gitignore`
/// excludes (present locally, absent on CI and on GitHub) passes here.
#[test]
fn breaker_link_targets_are_tracked() {
    let root = repo_root();
    let Some(listed) = git(&root, &["ls-files", "-z"]) else {
        return;
    };
    let tracked: BTreeSet<String> = split_nul(&listed).collect();
    let is_tracked = |path: &str| {
        let dir = format!("{path}/");
        tracked.contains(path)
            || tracked
                .range(dir.clone()..)
                .next()
                .is_some_and(|first| first.starts_with(&dir))
    };

    let mut sources = Vec::new();
    for single in ["README.md", "docs/README.md"] {
        let path = root.join(single);
        if path.is_file() {
            sources.push(path);
        }
    }
    for tree in ["docs/src", ".agent"] {
        sources.extend(
            walk_files(&root.join(tree))
                .into_iter()
                .filter(|path| has_extension(path, "md")),
        );
    }

    let mut problems = Vec::new();
    for source in sources {
        let source_rel = rel(&root, &source);
        if is_scratch_doc(&source_rel) {
            continue;
        }
        for (line, target) in markdown_links(&read_text(&source)) {
            let Some(path) = link_path(&target) else {
                continue;
            };
            let resolved = match path.strip_prefix('/') {
                Some(from_root) => normalize(&root.join(from_root)),
                None => normalize(&source.parent().unwrap_or(&root).join(&path)),
            };
            let target_rel = rel(&root, &resolved);
            if target_rel.is_empty() || is_scratch_doc(&target_rel) {
                continue;
            }
            if resolved.exists() && !is_tracked(&target_rel) {
                problems.push(format!(
                    "{source_rel}:{line}: {target} (not tracked by git)"
                ));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "links that resolve only in this working tree:\n  {}",
        problems.join("\n  ")
    );
}

/// The allowlist reader must accept a UTF-8 BOM, as `read_text` does for
/// Markdown: an editor that saves with a BOM would otherwise turn the first
/// line into a malformed entry and fail all ten checks at once.
#[test]
fn breaker_allowlist_tolerates_a_bom() {
    let dir = std::env::temp_dir().join(format!("khora-layout-bom-{}", std::process::id()));
    fs::create_dir_all(dir.join("xtask/tests")).expect("create temp allowlist dir");
    fs::write(
        dir.join(ALLOWLIST),
        "\u{feff}# header comment\nwgsl_only_in_shader_tree a.wgsl  # B4\n",
    )
    .expect("write temp allowlist");
    let (entries, problems) = read_allowlist(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        problems.is_empty(),
        "BOM breaks the allowlist: {problems:?}"
    );
    assert!(entries
        .get("wgsl_only_in_shader_tree")
        .is_some_and(|paths| paths.contains("a.wgsl")));
}

/// A reference-style link definition (`[label]: target`) is a Markdown link;
/// `markdown_links` never sees it, so a broken one slips through.
#[test]
fn breaker_reference_style_links_are_seen() {
    let links = markdown_links("See [the logo][logo].\n\n[logo]: ./missing-logo.png\n");
    assert!(
        links
            .iter()
            .any(|(_, target)| target == "./missing-logo.png"),
        "reference-style link not extracted: {links:?}"
    );
}

/// An HTML attribute with single quotes (`<img src='…'>`) is as much a link
/// as `src="…"`; only the double-quoted form is extracted.
#[test]
fn breaker_single_quoted_html_links_are_seen() {
    let links = markdown_links("<img src='./missing-logo.png' alt='logo'>\n");
    assert!(
        links
            .iter()
            .any(|(_, target)| target == "./missing-logo.png"),
        "single-quoted src not extracted: {links:?}"
    );
}
