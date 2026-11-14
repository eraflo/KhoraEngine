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
//! Rust checks scan `crates/*/src`, `xtask/src` and `examples/*/src`. Every
//! walk skips `target/`, `.git/`, `.codegraph/` and `node_modules/` at any
//! depth; the generated wrappers and tool state at the root (`.claude/`,
//! `.cursor/`, `.gemini/`, `.gitagent/`, `.khora/`, `.impeccable/`,
//! `.dist/`); and any directory holding a `.git` entry — a worktree,
//! submodule or nested clone, whose files are another checkout's.
//!
//! Every check is a **ratchet**. A check reports its violations keyed by a
//! stable repo-relative path (`<path>::<function>` for a function);
//! `xtask/tests/layout_allow.txt` lists the known
//! ones, one per line: `<check-name> <repo-relative path>  # reason`, where the
//! reason names the batch that removes the entry (`B0`–`B6`, `P7`, or an
//! Ergon spec). A check fails when
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
//! cannot be run in the repository; `cited_source_paths_exist` then checks
//! existence only.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Directory names never walked into, wherever they appear.
const SKIP_DIRS: &[&str] = &["target", ".git", ".codegraph", "node_modules"];

/// Directories at the repository root never walked into: generated AI
/// wrappers and tool state, not part of the tree the checks govern.
/// `.agent/` and `.github/` are sources and stay scanned.
const SKIP_ROOT_DIRS: &[&str] = &[
    ".claude",
    ".cursor",
    ".gemini",
    ".codegraph",
    ".gitagent",
    ".khora",
    ".impeccable",
    ".dist",
];

/// Largest number of non-test lines a Rust file may hold.
const MAX_NON_TEST_LINES: usize = 700;

/// Every check, as named in the allowlist (each is its test's name).
const RATCHETED_CHECKS: &[&str] = &[
    "no_file_and_folder_module_pair",
    "no_orphan_rust_files",
    "test_files_follow_convention",
    "non_test_code_under_700_lines",
    "wgsl_only_in_shader_tree",
    "no_tracked_ignored_files",
    "relative_links_resolve",
    "cited_source_paths_exist",
    "no_duplicate_asset_files",
    "one_spelling_of_util",
    "no_generic_module_file_names",
    "docs_pages_are_kebab_case",
    "no_stuttering_file_names",
    "retired_file_names",
    "constants_come_before_the_code",
    "private_helpers_come_after_the_primary_type_impls",
    "each_file_holds_one_primary_type",
    "functions_under_200_lines",
];

/// Module names too generic to say what a file holds: each is shared by
/// unrelated types across the workspace. Banned as a file module
/// (`registry.rs`).
const GENERIC_MODULE_NAMES: &[&str] = &["registry", "context", "service", "helpers", "widgets"];

/// The generic names also banned as a folder module (`service/mod.rs`).
/// `widgets/` is not among them: a folder holding one file per widget is a
/// collection, named for what it collects.
const GENERIC_FOLDER_NAMES: &[&str] = &["registry", "context", "service", "helpers"];

/// Markdown file names under `docs/src` exempt from kebab-case: mdBook
/// requires `SUMMARY.md` by that name.
const DOCS_NAME_EXEMPT: &[&str] = &["SUMMARY.md"];

/// Paths a rename has retired, with the reason. None may exist again.
const RETIRED_FILE_NAMES: &[(&str, &str)] = &[
    // A file is named after its primary type.
    (
        "crates/khora-core/src/utils/timer.rs",
        "holds `Stopwatch`: name the file after it",
    ),
    (
        "crates/khora-core/src/util/timer.rs",
        "holds `Stopwatch`: name the file after it",
    ),
    (
        "crates/khora-lanes/src/render_lane/util/dynamic_uniform_buffer.rs",
        "holds `DynamicUniformRingBuffer`: name the file after it",
    ),
    (
        "crates/khora-core/src/renderer/api/command/encoder.rs",
        "holds `DrawCommand`, not an encoder (the recorder trait is `traits/command_recorder.rs`)",
    ),
    (
        "crates/khora-core/src/asset/uuid.rs",
        "holds `AssetUUID`: name the file after it",
    ),
    (
        "crates/khora-infra/src/ui/taffy/taffy_layout.rs",
        "stutters its folder's name",
    ),
    // Editor and hub use the same words.
    (
        "crates/khora-hub/src/chrome/topbar.rs",
        "the editor's word is title bar: `title_bar.rs`",
    ),
    (
        "crates/khora-hub/src/ui/widgets.rs",
        "holds only `format_ts`: `ui/format.rs`",
    ),
    (
        "crates/khora-editor/src/widgets/chrome.rs",
        "clashes with the `chrome/` folder: `widgets/panel_header.rs`",
    ),
    (
        "crates/khora-editor/src/util.rs",
        "holds only `read_git_branch`: `git.rs`",
    ),
    ("xtask/src/helpers.rs", "split into `term.rs` and `exec.rs`"),
    // A name says what the thing is.
    (
        "crates/khora-io/src/vfs.rs",
        "`AssetIndex` is an asset index: `asset/index.rs` `AssetIndex`",
    ),
    (
        "crates/khora-core/src/renderer/api/scene",
        "GPU frame data, not the scene file format: `api/gpu_scene/`",
    ),
    (
        "crates/khora-core/src/renderer/api/core",
        "shadows the crate name and is a grab-bag: split it",
    ),
    // Shader in one place: `renderer/api/shader/`.
    (
        "crates/khora-core/src/renderer/api/shader_defs.rs",
        "shader code lives in `api/shader/`",
    ),
    (
        "crates/khora-core/src/renderer/api/resource/shader_source.rs",
        "shader code lives in `api/shader/`",
    ),
    // Lights in one place: `renderer/light/`.
    (
        "crates/khora-core/src/renderer/light.rs",
        "lights live in the `renderer/light/` folder",
    ),
];

/// Words one of which an allowlist reason must contain: the batch that
/// removes the entry.
const BATCH_WORDS: &[&str] = &["B0", "B1", "B2", "B3", "B4", "B5", "B6", "P7", "Ergon"];

/// Repository-relative path of the ratchet allowlist.
const ALLOWLIST: &str = "xtask/tests/layout_allow.txt";

/// The one directory `.wgsl` files may live in.
const SHADER_TREE: &str = "crates/khora-infra/src/graphics/shader/shaders/";

/// Markdown trees that are scratch: neither scanned nor link targets.
const SCRATCH_DOCS: &[&str] = &["docs/plans/", "docs/research/"];

/// Top-level directories whose paths a doc may cite in inline code.
const CITED_ROOTS: &[&str] = &["crates", "xtask", "examples"];

/// Extensions of the files a cited source path may name.
const CITED_EXTENSIONS: &[&str] = &["rs", "wgsl", "toml", "md", "json", "ron"];

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
            if !resolved.exists() && !renders_to(&root, &source, &resolved) {
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

/// Every repository source path a doc cites in inline code exists. Catches the
/// drift `relative_links_resolve` cannot see when a file moves: a
/// `` `crates/khora-control/src/scheduler.rs` `` that is prose, not a link.
///
/// Scanned: the files `relative_links_resolve` scans, minus the dated logs
/// `.agent/*/knowledge/MEMORY.md`. Only inline code spans outside fenced code
/// blocks are read. A span is a cited path when, with no whitespace in it:
///
/// - it starts with `crates/`, `xtask/`, `examples/`, or
///   `khora-<name>/` where `crates/khora-<name>/` exists (resolved as
///   `crates/khora-<name>/…`);
/// - once everything from its first `:` or `#` is dropped (`:123`,
///   `:12-34`, `::Symbol`, `#anchor`), it ends in `/` (a directory) or in a
///   file name with one of the extensions `.rs`, `.wgsl`, `.toml`, `.md`,
///   `.json`, `.ron`.
///
/// Brace groups are expanded (`{physics/rapier,audio/cpal}` → two paths, one
/// level of nesting); a group without a comma is a placeholder (`{domain}`)
/// and skips the span, as does any span containing `*`, `<`, `>`, `…` or
/// `...`. Bare file names (`` `service.rs` ``) and directories without a
/// trailing `/` are out of scope: nothing says where they live.
///
/// A cited path must exist in the working tree and be known to git — tracked,
/// or untracked but not ignored (a file a split has just created), or a
/// directory holding such a file — so a citation of a gitignored generated
/// file fails. When `git` cannot run, existence alone is checked.
/// Keyed by the citing Markdown file.
#[test]
fn cited_source_paths_exist() {
    let root = repo_root();
    let crates = crate_dir_names(&root);
    let known = git(
        &root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .map(|listed| split_nul(&listed).collect::<BTreeSet<String>>());

    let mut violations = Violations::new();
    for source in markdown_sources(&root) {
        let source_rel = rel(&root, &source);
        if is_scratch_doc(&source_rel) || is_dated_log(&source_rel) {
            continue;
        }
        for (line, span, path) in cited_source_paths(&read_text(&source), &crates) {
            let on_disk = root.join(path.trim_end_matches('/'));
            let exists = if path.ends_with('/') {
                on_disk.is_dir()
            } else {
                on_disk.is_file()
            };
            let problem = if !exists {
                "does not exist"
            } else if known
                .as_ref()
                .is_some_and(|known| !is_known_to_git(known, path.trim_end_matches('/')))
            {
                "ignored by git"
            } else {
                continue;
            };
            add(
                &mut violations,
                source_rel.clone(),
                format!("line {line}: `{span}` → {path} {problem}"),
            );
        }
    }
    ratchet(
        "cited_source_paths_exist",
        "source paths cited in inline code that do not exist",
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
// Naming
// ---------------------------------------------------------------------------

/// One spelling per folder concept: the helper folder is `util/` (singular,
/// like `flow/` and `lexer/`), never `utils/` — as a folder or as a
/// `utils.rs` file module — under any Rust `src/` tree. Keyed by the folder
/// or file.
#[test]
fn one_spelling_of_util() {
    let root = repo_root();
    let mut violations = Violations::new();
    for src in rust_src_roots(&root) {
        for dir in walk_dirs(&src) {
            if file_name(&dir) == "utils" {
                add(
                    &mut violations,
                    rel(&root, &dir),
                    "spelled `utils/` (use `util/`)".to_owned(),
                );
            }
        }
        for file in walk_files(&src) {
            if file_name(&file) == "utils.rs" {
                add(
                    &mut violations,
                    rel(&root, &file),
                    "spelled `utils.rs` (use `util`)".to_owned(),
                );
            }
        }
    }
    ratchet(
        "one_spelling_of_util",
        "`utils` where the concept is spelled `util`",
        &violations,
    );
}

/// No module under a Rust `src/` tree carries a name that says nothing about
/// what it holds:
///
/// - no file module named `registry.rs`, `context.rs`, `service.rs`,
///   `helpers.rs` or `widgets.rs` (`GENERIC_MODULE_NAMES`);
/// - no folder module (a folder holding `mod.rs`) named `registry/`,
///   `context/`, `service/` or `helpers/` (`GENERIC_FOLDER_NAMES`);
/// - no file or folder whose name starts with `mod_`.
///
/// Keyed by the file, or by the folder.
#[test]
fn no_generic_module_file_names() {
    let root = repo_root();
    let mut violations = Violations::new();
    for src in rust_src_roots(&root) {
        for file in walk_files(&src) {
            if !has_extension(&file, "rs") {
                continue;
            }
            let name = file_name(&file);
            let stem = name.trim_end_matches(".rs");
            if GENERIC_MODULE_NAMES.contains(&stem) {
                add(
                    &mut violations,
                    rel(&root, &file),
                    format!("generic name `{name}` (name it after the type it holds)"),
                );
            }
            if name.starts_with("mod_") {
                add(
                    &mut violations,
                    rel(&root, &file),
                    format!("`mod_` prefix on `{name}`"),
                );
            }
        }
        for dir in walk_dirs(&src) {
            let name = file_name(&dir);
            if GENERIC_FOLDER_NAMES.contains(&name.as_str()) && dir.join("mod.rs").is_file() {
                add(
                    &mut violations,
                    rel(&root, &dir),
                    format!("generic folder module `{name}/` (name it after what it holds)"),
                );
            }
            if name.starts_with("mod_") {
                add(
                    &mut violations,
                    rel(&root, &dir),
                    format!("`mod_` prefix on `{name}/`"),
                );
            }
        }
    }
    ratchet(
        "no_generic_module_file_names",
        "modules with a generic or `mod_`-prefixed name",
        &violations,
    );
}

/// Every Markdown page under `docs/src` has a kebab-case file name: lowercase
/// ASCII letters and digits, words joined by `-`, never `_`. `SUMMARY.md` is
/// exempt (mdBook requires it). Keyed by the page.
#[test]
fn docs_pages_are_kebab_case() {
    let root = repo_root();
    let mut violations = Violations::new();
    for page in walk_files(&root.join("docs/src")) {
        if !has_extension(&page, "md") {
            continue;
        }
        let name = file_name(&page);
        if DOCS_NAME_EXEMPT.contains(&name.as_str()) {
            continue;
        }
        if !is_kebab_case(name.trim_end_matches(".md")) {
            add(
                &mut violations,
                rel(&root, &page),
                format!("`{name}` is not kebab-case"),
            );
        }
    }
    ratchet(
        "docs_pages_are_kebab_case",
        "docs pages whose file name is not kebab-case",
        &violations,
    );
}

/// No Rust file repeats its folder's name: `<dir>/<dir>_*.rs` and
/// `<dir>/<dir>.rs` are out (`ui/taffy/taffy_layout.rs`, which holds
/// `TaffyLayoutSystem`), since the folder already says it. Scans every folder
/// below a Rust `src/` tree; `src/` itself is not a module name.
///
/// A file named after a type it declares is exempt, because "a file is named
/// after its primary type" comes first: `physics/physics_debug_data.rs` holds
/// `PhysicsDebugData`, `asset/asset_uuid.rs` would hold `AssetUUID`. "Named after"
/// means the file stem is the type's snake_case name, acronyms kept whole
/// (`AssetUUID` → `asset_uuid`). Keyed by the file.
#[test]
fn no_stuttering_file_names() {
    let root = repo_root();
    let mut violations = Violations::new();
    for src in rust_src_roots(&root) {
        for dir in walk_dirs(&src) {
            let folder = file_name(&dir);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            let mut stutters: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && has_extension(path, "rs"))
                .filter(|path| {
                    let name = file_name(path);
                    let stem = name.trim_end_matches(".rs");
                    stem == folder || stem.starts_with(&format!("{folder}_"))
                })
                .filter(|path| {
                    let name = file_name(path);
                    let stem = name.trim_end_matches(".rs");
                    !declared_type_names(&read_text(path))
                        .iter()
                        .any(|ty| snake_case(ty) == stem)
                })
                .collect();
            stutters.sort();
            for file in stutters {
                add(
                    &mut violations,
                    rel(&root, &file),
                    format!("repeats its folder's name `{folder}`"),
                );
            }
        }
    }
    ratchet(
        "no_stuttering_file_names",
        "file names that repeat their folder's name",
        &violations,
    );
}

/// No path a rename has retired exists again (`RETIRED_FILE_NAMES`): a file
/// named after something other than its primary type, a word the editor and
/// the hub spell differently, a name that misstates what the thing is. Keyed
/// by the retired path.
#[test]
fn retired_file_names() {
    let root = repo_root();
    let mut violations = Violations::new();
    for (path, reason) in RETIRED_FILE_NAMES {
        if root.join(path).exists() {
            add(&mut violations, (*path).to_owned(), (*reason).to_owned());
        }
    }
    ratchet(
        "retired_file_names",
        "retired file names that exist again",
        &violations,
    );
}

/// Names of the `struct`, `enum`, `union`, `trait` and `type` items a file
/// declares at the start of a line (any visibility, any indentation).
fn declared_type_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim_start();
        if let Some(after) = rest.strip_prefix("pub") {
            rest = after.trim_start();
            if rest.starts_with('(') {
                let Some(close) = rest.find(')') else {
                    continue;
                };
                rest = rest[close + 1..].trim_start();
            }
        }
        for keyword in ["struct ", "enum ", "union ", "trait ", "type "] {
            if let Some(after) = rest.strip_prefix(keyword) {
                let name: String = after
                    .trim_start()
                    .chars()
                    .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// `CamelCase` → `snake_case`, keeping an acronym whole: a `_` goes before an
/// uppercase letter that follows a lowercase letter or digit, or that starts
/// a new word after an acronym (`AssetUUID` → `asset_uuid`, `HTTPServer` →
/// `http_server`).
fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && index > 0 {
            let prev = chars[index - 1];
            let next_is_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
            if prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next_is_lower)
            {
                out.push('_');
            }
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// Lowercase ASCII letters and digits in words joined by single `-`.
fn is_kebab_case(stem: &str) -> bool {
    !stem.is_empty()
        && stem.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        })
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

/// `crates/*/src`, `examples/*/src`, `xtask/src` — those that exist.
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
    let xtask = root.join("xtask/src");
    if xtask.is_dir() {
        roots.push(xtask);
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

/// True for a directory the walkers must not enter:
///
/// - one named in `SKIP_DIRS` (`target/`, `.git/`, `.codegraph/`,
///   `node_modules/`), at any depth;
/// - one named in `SKIP_ROOT_DIRS`, directly under the repository root
///   (generated wrappers and tool state: `.claude/`, `.cursor/`, `.gemini/`,
///   `.codegraph/`, `.gitagent/`, `.khora/`, `.impeccable/`, `.dist/`);
/// - one holding a `.git` entry, file or directory — another checkout (a git
///   worktree, a submodule, a nested clone) whose files are copies, not this
///   tree's. Only directories below the walk's start are tested, so the
///   repository root, which holds `.git/` itself, is still walked.
fn is_skipped(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    SKIP_DIRS.iter().any(|skip| name == *skip)
        || (path.parent() == Some(repo_root().as_path())
            && SKIP_ROOT_DIRS.iter().any(|skip| name == *skip))
        || path.join(".git").exists()
}

/// Every file under `dir`, recursively, sorted. Directories `is_skipped`
/// rejects are not entered.
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

/// Every directory strictly below `dir`, recursively, sorted. Directories
/// `is_skipped` rejects are neither listed nor entered.
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

/// `#[cfg(test)]`, or `#[cfg(all(…))]` with `test` among its terms, in any
/// place.
fn is_cfg_test(attr: &str) -> bool {
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

/// Whether `resolved`, a link from `source` that names no file, is a page the
/// book renders: inside `docs/src`, mdBook writes `x.md` as `x.html`, and a
/// link in raw HTML (a landing block, a card) must name the `.html` it will
/// be, since mdBook rewrites only Markdown links.
fn renders_to(root: &Path, source: &Path, resolved: &Path) -> bool {
    let book = root.join("docs").join("src");
    source.starts_with(&book)
        && resolved.starts_with(&book)
        && resolved.extension().is_some_and(|ext| ext == "html")
        && resolved.with_extension("md").is_file()
}

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

/// `README.md`, `docs/README.md`, `docs/src/**/*.md` and `.agent/**/*.md`,
/// scratch trees included (callers filter them).
fn markdown_sources(root: &Path) -> Vec<PathBuf> {
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
    sources
}

/// `.agent/<profile>/knowledge/MEMORY.md`: a dated log, whose old entries
/// rightly name paths that have since moved.
fn is_dated_log(rel_path: &str) -> bool {
    matches!(
        rel_path.split('/').collect::<Vec<_>>().as_slice(),
        [".agent", _, "knowledge", "MEMORY.md"]
    )
}

/// Names of the directories directly under `crates/`.
fn crate_dir_names(root: &Path) -> BTreeSet<String> {
    child_dirs(&root.join("crates"))
        .iter()
        .map(|path| file_name(path))
        .collect()
}

/// True when `path` is in `known`, or is a directory holding a file that is.
fn is_known_to_git(known: &BTreeSet<String>, path: &str) -> bool {
    let dir = format!("{path}/");
    known.contains(path)
        || known
            .range(dir.clone()..)
            .next()
            .is_some_and(|first| first.starts_with(&dir))
}

/// `(1-based line, span content)` of every inline code span outside fenced
/// code blocks. A span opens on a run of backticks and closes on the next run
/// of the same length on the same line; an unmatched run is literal text.
fn inline_code_spans(text: &str) -> Vec<(usize, String)> {
    let mut spans = Vec::new();
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
        let bytes = raw.as_bytes();
        let run_end = |from: usize| {
            from + bytes[from..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count()
        };
        let mut at = 0;
        while let Some(offset) = raw[at..].find('`') {
            let open = at + offset;
            let content = run_end(open);
            let len = content - open;
            let mut search = content;
            let mut close = None;
            while let Some(offset) = raw[search..].find('`') {
                let start = search + offset;
                let end = run_end(start);
                if end - start == len {
                    close = Some(start);
                    break;
                }
                search = end;
            }
            match close {
                Some(close) => {
                    spans.push((index + 1, raw[content..close].trim().to_owned()));
                    at = close + len;
                }
                None => at = content,
            }
        }
    }
    spans
}

/// `(1-based line, span, repo-relative path)` of every source path cited in
/// an inline code span; see `cited_source_paths_exist` for what counts.
fn cited_source_paths(text: &str, crates: &BTreeSet<String>) -> Vec<(usize, String, String)> {
    let mut cited = Vec::new();
    for (line, span) in inline_code_spans(text) {
        let skipped = span.is_empty()
            || span.contains("...")
            || span
                .chars()
                .any(|ch| ch.is_whitespace() || matches!(ch, '*' | '<' | '>' | '…'));
        if skipped {
            continue;
        }
        let path = span.split([':', '#']).next().unwrap_or("");
        let Some(expanded) = expand_braces(path) else {
            continue;
        };
        for candidate in expanded {
            if let Some(path) = repo_source_path(&candidate, crates) {
                cited.push((line, span.clone(), path));
            }
        }
    }
    cited
}

/// Every alternative of `a{b,c}d{e,f}`, or `None` when a brace group is
/// nested, unbalanced, or has no comma (a placeholder such as `{domain}`).
fn expand_braces(path: &str) -> Option<Vec<String>> {
    let Some(open) = path.find('{') else {
        return (!path.contains('}')).then(|| vec![path.to_owned()]);
    };
    let close = open + path[open..].find('}')?;
    let inner = &path[open + 1..close];
    if inner.contains('{') || !inner.contains(',') || path[..open].contains('}') {
        return None;
    }
    let head = &path[..open];
    let tails = expand_braces(&path[close + 1..])?;
    Some(
        inner
            .split(',')
            .flat_map(|alt| tails.iter().map(move |tail| format!("{head}{alt}{tail}")))
            .collect(),
    )
}

/// The repo-relative path `candidate` names, when it is a cited source path.
fn repo_source_path(candidate: &str, crates: &BTreeSet<String>) -> Option<String> {
    let candidate = candidate.strip_prefix("./").unwrap_or(candidate);
    let (first, _) = candidate.split_once('/')?;
    let path = if CITED_ROOTS.contains(&first) {
        candidate.to_owned()
    } else if first.starts_with("khora-") && crates.contains(first) {
        format!("crates/{candidate}")
    } else {
        return None;
    };
    let last = path.rsplit('/').next().unwrap_or("");
    let names_file = Path::new(last)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| CITED_EXTENSIONS.contains(&ext));
    (path.ends_with('/') || names_file).then_some(path)
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
// Links a fresh clone must resolve, and allow-list parsing
// ---------------------------------------------------------------------------

/// A relative link must resolve in a fresh clone, not only in this working
/// tree: its target has to be tracked by git. `relative_links_resolve` checks
/// `exists()` on disk, so a link to a generated wrapper that `.gitignore`
/// excludes (present locally, absent on CI and on GitHub) passes here.
#[test]
fn link_targets_are_tracked() {
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
fn allowlist_tolerates_a_bom() {
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
fn reference_style_links_are_seen() {
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
fn single_quoted_html_links_are_seen() {
    let links = markdown_links("<img src='./missing-logo.png' alt='logo'>\n");
    assert!(
        links
            .iter()
            .any(|(_, target)| target == "./missing-logo.png"),
        "single-quoted src not extracted: {links:?}"
    );
}

// ---------------------------------------------------------------------------
// Helper tests
// ---------------------------------------------------------------------------

/// The citation extractor sees a plain path, a `:line` suffix, a brace group
/// and a `khora-x/` short form; it ignores fenced code, glob and placeholder
/// spans, bare file names, and `khora-` names that are not crate folders.
#[test]
fn cited_source_paths_extractor() {
    let crates: BTreeSet<String> = ["khora-core", "khora-infra"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let text = "\
Plain `crates/khora-core/src/lib.rs` here.
Line `crates/khora-core/src/math/vector.rs:123` and range `examples/sandbox/src/main.rs:4-9`.
Braces `crates/khora-infra/src/{physics/rapier,audio/cpal}/` too.
Short `khora-core/src/lane/bus.rs#anchor`, double ``xtask/Cargo.toml``.
```rust
// `crates/khora-core/src/fenced.rs`
```
~~~
`crates/khora-core/src/tilde_fenced.rs`
~~~
Glob `crates/*/src/lib.rs`, angle `crates/<name>/src/`, ellipsis `crates/…/x.rs`.
Placeholder `crates/khora-lanes/src/{domain}_lane/`, bare `service.rs`.
Not a crate `khora-render/src/lib.rs`, no extension `crates/khora-core`.
Command `cargo test -p xtask`, text `crates/khora-core/src/lib.rs is here`.
";
    let cited: Vec<(usize, String)> = cited_source_paths(text, &crates)
        .into_iter()
        .map(|(line, _, path)| (line, path))
        .collect();
    let expected: Vec<(usize, String)> = [
        (1, "crates/khora-core/src/lib.rs"),
        (2, "crates/khora-core/src/math/vector.rs"),
        (2, "examples/sandbox/src/main.rs"),
        (3, "crates/khora-infra/src/physics/rapier/"),
        (3, "crates/khora-infra/src/audio/cpal/"),
        (4, "crates/khora-core/src/lane/bus.rs"),
        (4, "xtask/Cargo.toml"),
    ]
    .into_iter()
    .map(|(line, path)| (line, path.to_owned()))
    .collect();
    assert_eq!(cited, expected);
}

/// The walkers never enter another checkout: a directory holding a `.git`
/// file (a worktree) or a `.git/` directory (a submodule, a nested clone) is
/// skipped with everything under it, while an ordinary sibling is visited.
#[test]
fn walkers_skip_nested_checkouts() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-nested-git-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let setup = || -> std::io::Result<()> {
        fs::create_dir_all(dir.join("normal/deep"))?;
        fs::write(dir.join("normal/deep/kept.wgsl"), "")?;
        fs::create_dir_all(dir.join("worktree/sub"))?;
        fs::write(dir.join("worktree/.git"), "gitdir: elsewhere\n")?;
        fs::write(dir.join("worktree/sub/copy.wgsl"), "")?;
        fs::create_dir_all(dir.join("submodule/.git"))?;
        fs::write(dir.join("submodule/copy.wgsl"), "")?;
        Ok(())
    };
    let setup = setup();
    let files: Vec<String> = walk_files(&dir).iter().map(|p| rel(&dir, p)).collect();
    let dirs: Vec<String> = walk_dirs(&dir).iter().map(|p| rel(&dir, p)).collect();
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(files, vec!["normal/deep/kept.wgsl".to_owned()]);
    assert_eq!(dirs, vec!["normal".to_owned(), "normal/deep".to_owned()]);
}

/// Type names convert to file stems with acronyms kept whole.
#[test]
fn snake_case_keeps_acronyms_whole() {
    for (ty, stem) in [
        ("AssetUUID", "asset_uuid"),
        ("TaffyLayoutSystem", "taffy_layout_system"),
        ("PhysicsDebugData", "physics_debug_data"),
        ("HTTPServer", "http_server"),
        ("Vec3", "vec3"),
        ("Mat4x4", "mat4x4"),
        ("Stopwatch", "stopwatch"),
    ] {
        assert_eq!(snake_case(ty), stem, "{ty}");
    }
}

/// Every type-like item is found, whatever its visibility; functions and
/// words inside other items are not types.
#[test]
fn declared_type_names_finds_items() {
    let text = "\
pub struct Alpha {
pub(crate) enum Beta {
    trait Gamma: Send {
pub type Delta = u32;
union Epsilon {
pub fn struct_like() {}
// struct Commented
let structure = 1;
";
    let names = declared_type_names(text);
    assert_eq!(names, vec!["Alpha", "Beta", "Gamma", "Delta", "Epsilon"]);
}

/// Kebab-case is lowercase words of letters and digits joined by single `-`.
#[test]
fn kebab_case_matcher() {
    for good in ["open-questions", "roadmap", "ergon-02-types", "a1-b2"] {
        assert!(is_kebab_case(good), "`{good}` should be kebab-case");
    }
    for bad in [
        "open_questions",
        "Open-questions",
        "SUMMARY",
        "open--questions",
        "-open",
        "open-",
        "open questions",
        "",
    ] {
        assert!(!is_kebab_case(bad), "`{bad}` should not be kebab-case");
    }
}

/// Only `.agent/<profile>/knowledge/MEMORY.md` is a dated log.
#[test]
fn dated_log_is_only_the_knowledge_memory() {
    assert!(is_dated_log(".agent/engine/knowledge/MEMORY.md"));
    assert!(!is_dated_log(".agent/engine/knowledge/decisions.md"));
    assert!(!is_dated_log(".agent/engine/MEMORY.md"));
    assert!(!is_dated_log("docs/src/knowledge/MEMORY.md"));
}

/// A book page's `.html` link resolves when the book renders that page from a
/// `.md` beside it, and only then, and only inside the book.
#[test]
fn a_book_link_to_a_rendered_page_resolves() {
    let root = repo_root();
    let book = root.join("docs").join("src");
    let home = book.join("home.md");
    assert!(renders_to(
        &root,
        &home,
        &book.join("concepts").join("saa.html")
    ));
    assert!(!renders_to(
        &root,
        &home,
        &book.join("concepts").join("no-such-page.html")
    ));
    assert!(!renders_to(
        &root,
        &root.join("README.md"),
        &book.join("concepts").join("saa.html")
    ));
    assert!(!renders_to(
        &root,
        &home,
        &book.join("concepts").join("saa.md")
    ));
}

// ---------------------------------------------------------------------------
// The agent and lane contract
// ---------------------------------------------------------------------------

/// Lanes consume Views from the `LaneBus`; the `World` is the Flows' and the
/// DataSystems' to read. So no Rust file under `crates/khora-lanes/src` names
/// `World` as a word outside comments — not in a `use`, not in a signature,
/// not in a downcast. `RenderWorld` and the like are other words. Keyed by
/// file and line.
#[test]
fn lanes_never_name_world() {
    let root = repo_root();
    let mut problems = Vec::new();
    for path in walk_files(&root.join("crates/khora-lanes/src")) {
        if !has_extension(&path, "rs") {
            continue;
        }
        let code = strip_rust_comments(&read_text(&path));
        for (index, line) in code.lines().enumerate() {
            if names_word(line, "World") {
                problems.push(format!(
                    "{}:{}: {}",
                    rel(&root, &path),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "lanes name the World — a lane reads Views from the bus, the World is the Flows' \
         and the DataSystems':\n  {}",
        problems.join("\n  ")
    );
}

/// An agent reaches the runtime through `EngineContext::resource` and
/// `::locked`, which refuse what its `contention()` did not declare; reading
/// the context's `runtime` field directly bypasses that check. So no file
/// under `crates/khora-agents/src` reads `.runtime` on a binding of type
/// `EngineContext` (a parameter `x: &mut EngineContext`, `x: &EngineContext`,
/// or a `let x = EngineContext::…`). Keyed by file and line.
#[test]
fn agents_never_read_runtime_field() {
    let root = repo_root();
    let mut problems = Vec::new();
    for path in walk_files(&root.join("crates/khora-agents/src")) {
        if !has_extension(&path, "rs") {
            continue;
        }
        let code = strip_rust_comments(&read_text(&path));
        for line in runtime_field_reads(&code) {
            problems.push(format!("{}:{}", rel(&root, &path), line));
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "agents read EngineContext.runtime directly instead of through `resource` / `locked`:\n  {}",
        problems.join("\n  ")
    );
}

/// `EngineContext.runtime` is private to `khora-core`: `resource` and `locked`
/// are the only doors, so a `contention()` declaration cannot be bypassed from
/// another crate.
#[test]
fn engine_context_runtime_field_is_private() {
    let root = repo_root();
    let path = root.join("crates/khora-core/src/engine_context.rs");
    let code = strip_rust_comments(&read_text(&path));
    let public: Vec<String> = code
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim_start();
            line.strip_prefix("pub ")
                .is_some_and(|rest| rest.trim_start().starts_with("runtime:"))
        })
        .map(|(index, line)| format!("{}:{}: {}", rel(&root, &path), index + 1, line.trim()))
        .collect();
    assert!(
        public.is_empty(),
        "EngineContext.runtime is public; agents must reach the runtime through `resource` \
         and `locked` only:\n  {}",
        public.join("\n  ")
    );
}

/// The modules of `crates/khora-core/src/lane/plumbing/`: how views reach a
/// lane and how its outputs and staged items leave it. The folder holds these
/// and nothing else; anything new about lanes is contract.
const LANE_PLUMBING: &[&str] = &[
    "bus",
    "context_keys",
    "deck",
    "host_views",
    "lease",
    "reserve",
    "staging",
];

/// The modules `crates/khora-core/src/lane/contract/` holds at least: what a
/// lane is, declares and promises. The rest of the lane folder.
const LANE_CONTRACT: &[&str] = &[
    "compile_check",
    "cost",
    "erased",
    "error",
    "family",
    "guard",
    "io",
    "lane",
    "level",
];

/// `crates/khora-core/src/lane/` is split in two: `contract/` (what a lane
/// is) and `plumbing/` (the bus, the deck and what carries items between
/// them). The folder holds `mod.rs` and those two folders, nothing else;
/// `plumbing/` holds exactly the plumbing modules, `contract/` the contract
/// modules and none of the plumbing ones. A module may be a file (`bus.rs`) or
/// a folder (`bus/`). Strict: not in the ratchet.
#[test]
fn the_lane_folder_holds_the_contract_and_the_plumbing() {
    let root = repo_root();
    let lane = root.join("crates/khora-core/src/lane");
    let problems = lane_folder_problems(&lane);
    assert!(
        problems.is_empty(),
        "crates/khora-core/src/lane/ is not split into `contract/` and `plumbing/`:\n  {}",
        problems.join("\n  ")
    );
}

/// The entries directly under `dir`, a folder's name ending in `/`, sorted.
/// Empty when `dir` cannot be read.
fn folder_entries(dir: &Path) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let name = file_name(&path);
            if path.is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect()
}

/// The module a folder entry declares: `bus.rs` and `bus/` are `bus`; any
/// other file is not a module.
fn entry_module(entry: &str) -> Option<&str> {
    entry
        .strip_suffix('/')
        .or_else(|| entry.strip_suffix(".rs"))
}

/// What keeps `lane` (a `lane/` folder) from being split into `contract/`
/// and `plumbing/`, sorted. Each line names the entry relative to `lane`.
fn lane_folder_problems(lane: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    if !lane.is_dir() {
        problems.push(format!("{}: no such folder", lane.display()));
        return problems;
    }

    let expected: BTreeSet<String> = ["mod.rs", "contract/", "plumbing/"]
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect();
    let found = folder_entries(lane);
    for entry in found.difference(&expected) {
        problems.push(format!(
            "lane/{entry}: the lane folder holds only `mod.rs`, `contract/` and `plumbing/`"
        ));
    }
    for entry in expected.difference(&found) {
        problems.push(format!("lane/{entry}: missing"));
    }

    let halves = [
        ("contract", LANE_CONTRACT, LANE_PLUMBING, false),
        ("plumbing", LANE_PLUMBING, LANE_CONTRACT, true),
    ];
    for (half, own, other, closed) in halves {
        let dir = lane.join(half);
        if !dir.is_dir() {
            continue;
        }
        let entries = folder_entries(&dir);
        if !entries.contains("mod.rs") {
            problems.push(format!("lane/{half}/mod.rs: missing"));
        }
        let modules: BTreeSet<&str> = entries.iter().filter_map(|e| entry_module(e)).collect();
        for module in own {
            if !modules.contains(module) {
                problems.push(format!(
                    "lane/{half}/{module}.rs: missing (`{module}` belongs to `{half}/`)"
                ));
            }
        }
        for module in other {
            if modules.contains(module) {
                problems.push(format!(
                    "lane/{half}/{module}: `{module}` belongs to the other half, not `{half}/`"
                ));
            }
        }
        if closed {
            for entry in &entries {
                let known = entry == "mod.rs"
                    || entry_module(entry).is_some_and(|module| own.contains(&module));
                if !known && !entry_module(entry).is_some_and(|m| other.contains(&m)) {
                    problems.push(format!(
                        "lane/{half}/{entry}: `{half}/` holds only {}; the rest is contract",
                        own.join(", ")
                    ));
                }
            }
        }
    }
    problems.sort();
    problems
}

/// The lane folder check accepts the split layout, with a module as a file or
/// a folder, and names what is wrong in the flat layout, in a plumbing module
/// left in the contract, and in a new module put in the plumbing.
#[test]
fn lane_folder_check_names_what_breaks_the_split() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-lane-folder-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let write_tree = |base: &Path, files: &[&str]| -> std::io::Result<()> {
        for file in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, "")?;
        }
        Ok(())
    };
    let split: Vec<String> = std::iter::once("mod.rs".to_owned())
        .chain(["contract/mod.rs".to_owned(), "plumbing/mod.rs".to_owned()])
        .chain(LANE_CONTRACT.iter().map(|m| {
            if *m == "io" {
                "contract/io/mod.rs".to_owned()
            } else {
                format!("contract/{m}.rs")
            }
        }))
        .chain(LANE_PLUMBING.iter().map(|m| format!("plumbing/{m}.rs")))
        .collect();
    let split: Vec<&str> = split.iter().map(String::as_str).collect();
    let flat: Vec<String> = std::iter::once("mod.rs".to_owned())
        .chain(["contract.rs".to_owned(), "io/mod.rs".to_owned()])
        .chain(
            LANE_CONTRACT
                .iter()
                .chain(LANE_PLUMBING)
                .filter(|m| !matches!(**m, "io" | "lane"))
                .map(|m| format!("{m}.rs")),
        )
        .collect();
    let flat: Vec<&str> = flat.iter().map(String::as_str).collect();

    let setup = || -> std::io::Result<()> {
        write_tree(&dir.join("split/lane"), &split)?;
        write_tree(&dir.join("folder_module/lane"), &split)?;
        fs::remove_file(dir.join("folder_module/lane/contract/guard.rs"))?;
        write_tree(&dir.join("folder_module/lane"), &["contract/guard/mod.rs"])?;
        write_tree(&dir.join("flat/lane"), &flat)?;
        write_tree(&dir.join("misplaced/lane"), &split)?;
        fs::remove_file(dir.join("misplaced/lane/plumbing/bus.rs"))?;
        write_tree(&dir.join("misplaced/lane"), &["contract/bus.rs"])?;
        write_tree(&dir.join("new_plumbing/lane"), &split)?;
        write_tree(&dir.join("new_plumbing/lane"), &["plumbing/route.rs"])?;
        Ok(())
    };
    let setup = setup();
    let split_problems = lane_folder_problems(&dir.join("split/lane"));
    let folder_module_problems = lane_folder_problems(&dir.join("folder_module/lane"));
    let flat_problems = lane_folder_problems(&dir.join("flat/lane"));
    let misplaced_problems = lane_folder_problems(&dir.join("misplaced/lane"));
    let new_plumbing_problems = lane_folder_problems(&dir.join("new_plumbing/lane"));
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(split_problems.is_empty(), "{split_problems:#?}");
    assert!(
        folder_module_problems.is_empty(),
        "{folder_module_problems:#?}"
    );
    assert!(
        flat_problems.iter().any(|p| p.starts_with("lane/bus.rs:"))
            && flat_problems.iter().any(|p| p.starts_with("lane/io/:"))
            && flat_problems
                .iter()
                .any(|p| p.starts_with("lane/contract/:"))
            && flat_problems
                .iter()
                .any(|p| p.starts_with("lane/plumbing/:")),
        "{flat_problems:#?}"
    );
    assert!(
        misplaced_problems
            .iter()
            .any(|p| p.starts_with("lane/contract/bus:"))
            && misplaced_problems
                .iter()
                .any(|p| p.starts_with("lane/plumbing/bus.rs: missing")),
        "{misplaced_problems:#?}"
    );
    assert_eq!(new_plumbing_problems.len(), 1, "{new_plumbing_problems:#?}");
    assert!(
        new_plumbing_problems[0].starts_with("lane/plumbing/route.rs:"),
        "{new_plumbing_problems:#?}"
    );
}

// ---------------------------------------------------------------------------
// Families
// ---------------------------------------------------------------------------

/// Where the families live: one folder each, directly under this one.
const LANES_SRC: &str = "crates/khora-lanes/src";

/// Each family folder under `crates/khora-lanes/src/`, and the domain its
/// `<domain>_lane/` (khora-lanes) and `<domain>_agent/` (khora-agents)
/// folders were named by before the family moved into its own folder.
const FAMILY_FOLDERS: &[(&str, &str)] = &[
    ("audio", "audio"),
    ("loading", "loading"),
    ("overlay", "overlay"),
    ("physics", "physics"),
    ("render", "render"),
    ("script", "script"),
    ("shadows", "shadow"),
    ("skybox", "skybox"),
    ("ui", "ui"),
];

/// The family folder that groups several families, one sub-folder each.
const OVERLAY: &str = "overlay";

/// The families `overlay/` holds, one sub-folder each, and nothing else.
const OVERLAY_FAMILIES: &[&str] = &["gizmo", "grid", "wireframe"];

/// Folders directly under `crates/khora-lanes/src/` that hold no family
/// (shared helpers, say). A folder there is a family unless it is named here.
const NOT_FAMILY_FOLDERS: &[&str] = &[];

/// The files every family folder holds: its module, its family and outputs
/// (`family.rs`), and its one `register_agent!` (`registration.rs`).
const FAMILY_FOLDER_FILES: &[&str] = &["mod.rs", "family.rs", "registration.rs"];

/// The module where `register_agent!` is defined; its body calls itself.
const REGISTER_AGENT_DEFINITION: &str = "crates/khora-core/src/agent/registration.rs";

/// A family is read in one folder. Every folder directly under
/// `crates/khora-lanes/src/` is a family folder (`FAMILY_FOLDERS`) holding
/// `mod.rs`, `family.rs`, `registration.rs` (with exactly one
/// `register_agent!` call outside comments and tests) and at least one lane,
/// unless it is named in `NOT_FAMILY_FOLDERS`. `overlay/` groups three
/// families: it holds `mod.rs` and the family folders `grid/`, `wireframe/`
/// and `gizmo/`, nothing else. Strict: not in the ratchet.
#[test]
fn every_family_folder_holds_its_family_and_registration() {
    let root = repo_root();
    let problems = family_folder_problems(&root.join(LANES_SRC));
    assert!(
        problems.is_empty(),
        "family folders under {LANES_SRC}/ that do not hold their family and registration:\n  {}",
        problems.join("\n  ")
    );
}

/// A family registers where it is read. Outside test code, a
/// `register_agent!` call (comments do not count) appears only in a
/// `registration.rs` under `crates/khora-lanes/src/`, or in the macro's own
/// definition in khora-core. Test files and inline test modules may build
/// registrations of their own. Strict: not in the ratchet.
#[test]
fn registrations_live_in_their_family_folder() {
    let root = repo_root();
    let mut problems = Vec::new();
    for src in rust_src_roots(&root) {
        if !src.starts_with(root.join("crates")) {
            continue;
        }
        for path in walk_files(&src) {
            if !has_extension(&path, "rs")
                || file_name(&path) == "tests.rs"
                || under_tests_dir(&src, &path)
            {
                continue;
            }
            let path_rel = rel(&root, &path);
            if registration_site_is_allowed(&path_rel) {
                continue;
            }
            for line in register_agent_calls(&read_text(&path)) {
                problems.push(format!("{path_rel}:{line}"));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "`register_agent!` outside a family folder's `registration.rs` \
         ({LANES_SRC}/<family>/registration.rs):\n  {}",
        problems.join("\n  ")
    );
}

/// A family moves whole. For each family folder under
/// `crates/khora-lanes/src/`, the folders it moved out of —
/// `crates/khora-lanes/src/<domain>_lane/` and
/// `crates/khora-agents/src/<domain>_agent/`, `<domain>` read from
/// `FAMILY_FOLDERS` — no longer exist.
#[test]
fn a_moved_family_leaves_no_old_folder() {
    let root = repo_root();
    let mut problems = Vec::new();
    for (family, domain) in FAMILY_FOLDERS {
        if !root.join(LANES_SRC).join(family).is_dir() {
            continue;
        }
        for old in [
            format!("{LANES_SRC}/{domain}_lane"),
            format!("crates/khora-agents/src/{domain}_agent"),
        ] {
            if root.join(&old).exists() {
                problems.push(format!(
                    "{old}/: still exists, although {LANES_SRC}/{family}/ holds the family"
                ));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "families moved into their folder but left their old one behind:\n  {}",
        problems.join("\n  ")
    );
}

/// What keeps the folders directly under `lanes_src` from being family
/// folders, sorted. Each line names the entry relative to `lanes_src`.
fn family_folder_problems(lanes_src: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for dir in child_dirs(lanes_src) {
        let name = file_name(&dir);
        if NOT_FAMILY_FOLDERS.contains(&name.as_str()) {
            continue;
        }
        if name == OVERLAY {
            if !dir.join("mod.rs").is_file() {
                problems.push(format!("{name}/mod.rs: missing"));
            }
            let found: BTreeSet<String> = child_dirs(&dir).iter().map(|d| file_name(d)).collect();
            for family in OVERLAY_FAMILIES {
                if !found.contains(*family) {
                    problems.push(format!("{name}/{family}/: missing"));
                }
            }
            for sub in &found {
                if OVERLAY_FAMILIES.contains(&sub.as_str()) {
                    family_problems(&dir.join(sub), &format!("{name}/{sub}"), &mut problems);
                } else {
                    problems.push(format!(
                        "{name}/{sub}/: `{name}/` holds only the family folders {}",
                        OVERLAY_FAMILIES.join(", ")
                    ));
                }
            }
        } else if FAMILY_FOLDERS.iter().any(|(family, _)| *family == name) {
            family_problems(&dir, &name, &mut problems);
        } else {
            problems.push(format!(
                "{name}/: not a family (FAMILY_FOLDERS) and not named in NOT_FAMILY_FOLDERS"
            ));
        }
    }
    problems.sort();
    problems
}

/// What `dir`, a family folder shown as `label`, lacks.
fn family_problems(dir: &Path, label: &str, problems: &mut Vec<String>) {
    for file in FAMILY_FOLDER_FILES {
        if !dir.join(file).is_file() {
            problems.push(format!("{label}/{file}: missing"));
        }
    }
    let registration = dir.join("registration.rs");
    if registration.is_file() {
        let calls = register_agent_calls(&read_text(&registration)).len();
        if calls != 1 {
            problems.push(format!(
                "{label}/registration.rs: {calls} `register_agent!` calls; a family registers one"
            ));
        }
    }
    let lanes = folder_entries(dir)
        .iter()
        .filter_map(|entry| entry_module(entry))
        .filter(|module| !matches!(*module, "mod" | "family" | "registration" | "tests"))
        .count();
    if lanes == 0 {
        problems.push(format!("{label}/: holds no lane"));
    }
}

/// Whether a production `register_agent!` call may stand in the file at
/// `path` (repository-relative, `/`-separated).
fn registration_site_is_allowed(path: &str) -> bool {
    let in_family_folder = path
        .strip_prefix(LANES_SRC)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.strip_suffix("/registration.rs"))
        .is_some_and(|folder| !folder.is_empty());
    in_family_folder || path == REGISTER_AGENT_DEFINITION
}

/// The 1-based lines of `text` that call `register_agent!` — the name then,
/// past any whitespace, `{`, `(` or `[` — comments excluded and the inline
/// test module (from its `#[cfg(test)]` on) too. A mention in prose, a
/// string or a comment is not a call.
fn register_agent_calls(text: &str) -> Vec<usize> {
    const MACRO: &str = "register_agent!";
    strip_rust_comments(text)
        .lines()
        .take(non_test_lines(text))
        .enumerate()
        .filter(|(_, line)| {
            line.match_indices(MACRO).any(|(at, _)| {
                line[at + MACRO.len()..]
                    .trim_start()
                    .starts_with(['{', '(', '['])
            })
        })
        .map(|(index, _)| index + 1)
        .collect()
}

/// The family folder check accepts a complete family, the grouped overlay and
/// a folder not moved yet, and names what a family folder lacks, a folder
/// that is no family, an overlay family missing, and a registration that
/// registers nothing or two families.
#[test]
fn family_folder_check_names_what_a_family_folder_lacks() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-family-folder-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    const ONE: &str = "inventory::submit! { RegisteredAgent(khora_core::register_agent! { }) }\n";
    const TWO: &str = "khora_core::register_agent! { }\nkhora_core::register_agent! { }\n";
    const MENTION: &str = "// `register_agent!` is called below.\n";
    let write_tree = |base: &Path, files: &[(&str, &str)]| -> std::io::Result<()> {
        for (file, text) in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    };
    let family = |prefix: &str| -> Vec<(String, &'static str)> {
        vec![
            (format!("{prefix}/mod.rs"), ""),
            (format!("{prefix}/family.rs"), ""),
            (format!("{prefix}/registration.rs"), ONE),
            (format!("{prefix}/lane.rs"), ""),
        ]
    };
    let mut good: Vec<(String, &str)> = family("audio");
    good.extend(family("render"));
    good.push(("render/forward_plus/mod.rs".to_owned(), ""));
    good.push(("overlay/mod.rs".to_owned(), ""));
    for sub in OVERLAY_FAMILIES {
        good.extend(family(&format!("overlay/{sub}")));
    }
    let good: Vec<(&str, &str)> = good.iter().map(|(p, t)| (p.as_str(), *t)).collect();

    let bad: Vec<(&str, &str)> = vec![
        // No family.rs, and a registration that only mentions the macro.
        ("audio/mod.rs", ""),
        ("audio/registration.rs", MENTION),
        ("audio/spatial_mixing.rs", ""),
        // No lane.
        ("physics/mod.rs", ""),
        ("physics/family.rs", ""),
        ("physics/registration.rs", ONE),
        // Two families in one registration.
        ("skybox/mod.rs", ""),
        ("skybox/family.rs", ""),
        ("skybox/registration.rs", TWO),
        ("skybox/lane.rs", ""),
        // A folder that is no family and is not listed.
        ("util/mod.rs", ""),
        // Overlay with one family of three, and a stray folder.
        ("overlay/mod.rs", ""),
        ("overlay/grid/mod.rs", ""),
        ("overlay/grid/family.rs", ""),
        ("overlay/grid/registration.rs", ONE),
        ("overlay/grid/lane.rs", ""),
        ("overlay/shared/mod.rs", ""),
        // A lane folder of the old layout.
        ("ui_lane/mod.rs", ""),
    ];

    let setup = || -> std::io::Result<()> {
        write_tree(&dir.join("good"), &good)?;
        write_tree(&dir.join("bad"), &bad)?;
        Ok(())
    };
    let setup = setup();
    let good_problems = family_folder_problems(&dir.join("good"));
    let bad_problems = family_folder_problems(&dir.join("bad"));
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(good_problems.is_empty(), "{good_problems:#?}");
    for expected in [
        "audio/family.rs: missing",
        "audio/registration.rs: 0 `register_agent!` calls",
        "physics/: holds no lane",
        "skybox/registration.rs: 2 `register_agent!` calls",
        "util/: not a family",
        "overlay/gizmo/: missing",
        "overlay/wireframe/: missing",
        "overlay/shared/:",
        "ui_lane/: not a family",
    ] {
        assert!(
            bad_problems.iter().any(|p| p.starts_with(expected)),
            "expected a problem starting `{expected}`:\n{bad_problems:#?}"
        );
    }
    assert_eq!(bad_problems.len(), 9, "{bad_problems:#?}");
}

/// The registration rule allows a family folder's `registration.rs` (nested
/// families too) and the macro's definition; it refuses any other file. It counts calls, not mentions in comments, and
/// not calls in the inline test module.
#[test]
fn registration_rule_reads_calls_in_family_folders_only() {
    for allowed in [
        "crates/khora-lanes/src/audio/registration.rs",
        "crates/khora-lanes/src/overlay/grid/registration.rs",
        "crates/khora-core/src/agent/registration.rs",
    ] {
        assert!(registration_site_is_allowed(allowed), "{allowed}");
    }
    for refused in [
        "crates/khora-lanes/src/registration.rs",
        "crates/khora-lanes/src/audio/mod.rs",
        "crates/khora-lanes/src/audio/family.rs",
        "crates/khora-agents/src/registered_agent.rs",
        "crates/khora-agents/src/lane_agent/agent.rs",
        "crates/khora-agents/src/ui_agent/agent.rs",
        "crates/khora-agents/src/ui_agent/mod.rs",
        "crates/khora-agents/src/families/audio/registration.rs",
        "crates/khora-sdk/src/lib.rs",
    ] {
        assert!(!registration_site_is_allowed(refused), "{refused}");
    }

    let text = "\
//! Registers with `register_agent!`.
/// ```
/// register_agent! { }
/// ```
static A: R = khora_core::register_agent! {
};
/* register_agent! { } */
const HELP: &str = \"families of lanes (`register_agent!`), see the docs\";
#[cfg(test)]
mod tests {
    static B: R = khora_core::register_agent! { };
}
";
    assert_eq!(register_agent_calls(text), vec![5]);
}

// ---------------------------------------------------------------------------
// khora-agents, and the tests of each family
// ---------------------------------------------------------------------------

/// The source tree of khora-agents.
const KHORA_AGENTS_SRC: &str = "crates/khora-agents/src";

/// Everything `crates/khora-agents/src/` holds: the crate root, the boot of
/// the registered families (`registered_agent.rs`) and the one agent
/// (`lane_agent/`). The families themselves live in khora-lanes.
const KHORA_AGENTS_SRC_ENTRIES: &[&str] = &["lib.rs", "registered_agent.rs", "lane_agent/"];

/// khora-agents holds the `LaneAgent` and the boot of the registered
/// families, nothing else: `crates/khora-agents/src/` holds exactly `lib.rs`,
/// `registered_agent.rs` and the folder `lane_agent/`. Strict: not in the
/// ratchet.
#[test]
fn khora_agents_holds_only_the_lane_agent_and_its_boot() {
    let root = repo_root();
    let problems = exact_entries_problems(&root.join(KHORA_AGENTS_SRC), KHORA_AGENTS_SRC_ENTRIES);
    assert!(
        problems.is_empty(),
        "{KHORA_AGENTS_SRC}/ holds more or less than the LaneAgent and its boot \
         ({}):\n  {}",
        KHORA_AGENTS_SRC_ENTRIES.join(", "),
        problems.join("\n  ")
    );
}

/// What keeps `dir` from holding exactly `expected` (folders spelled with a
/// trailing `/`), sorted. Each line names the entry relative to `dir`.
fn exact_entries_problems(dir: &Path, expected: &[&str]) -> Vec<String> {
    if !dir.is_dir() {
        return vec![format!("{}: no such folder", dir.display())];
    }
    let expected: BTreeSet<String> = expected.iter().map(|e| (*e).to_owned()).collect();
    let found = folder_entries(dir);
    let mut problems = Vec::new();
    for entry in found.difference(&expected) {
        problems.push(format!("{entry}: not one of {expected:?}"));
    }
    for entry in expected.difference(&found) {
        problems.push(format!("{entry}: missing"));
    }
    problems.sort();
    problems
}

/// The khora-agents check accepts the crate as it is meant to be, and names
/// the agent folder under its old name, a family's agent folder left behind,
/// a stray file, and a `lane_agent` written as a file instead of a folder.
#[test]
fn khora_agents_check_names_what_is_not_the_lane_agent() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-agents-src-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let write_tree = |base: &Path, files: &[&str]| -> std::io::Result<()> {
        for file in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, "")?;
        }
        Ok(())
    };
    let setup = || -> std::io::Result<()> {
        write_tree(
            &dir.join("good"),
            &[
                "lib.rs",
                "registered_agent.rs",
                "lane_agent/mod.rs",
                "lane_agent/choice.rs",
            ],
        )?;
        write_tree(
            &dir.join("old"),
            &[
                "lib.rs",
                "registered_agent.rs",
                "agent/mod.rs",
                "ui_agent/mod.rs",
                "util.rs",
            ],
        )?;
        write_tree(
            &dir.join("file"),
            &["lib.rs", "registered_agent.rs", "lane_agent.rs"],
        )?;
        Ok(())
    };
    let setup = setup();
    let good = exact_entries_problems(&dir.join("good"), KHORA_AGENTS_SRC_ENTRIES);
    let old = exact_entries_problems(&dir.join("old"), KHORA_AGENTS_SRC_ENTRIES);
    let file = exact_entries_problems(&dir.join("file"), KHORA_AGENTS_SRC_ENTRIES);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(good.is_empty(), "{good:#?}");
    for expected in ["agent/:", "ui_agent/:", "util.rs:", "lane_agent/: missing"] {
        assert!(
            old.iter().any(|p| p.starts_with(expected)),
            "expected a problem starting `{expected}`:\n{old:#?}"
        );
    }
    assert_eq!(old.len(), 4, "{old:#?}");
    assert!(
        file.iter().any(|p| p.starts_with("lane_agent.rs:"))
            && file.iter().any(|p| p.starts_with("lane_agent/: missing")),
        "{file:#?}"
    );
    assert_eq!(file.len(), 2, "{file:#?}");
}

/// An integration-test tree whose tests are grouped by family.
struct FamilyTestTree {
    /// The crate's `tests/it/`, repository-relative.
    it: &'static str,
    /// The folder directly under `it` that holds one folder per family, or
    /// `""` when the family folders sit directly under `it`.
    families: &'static str,
    /// The entries directly under `it` that pin no single family (a folder
    /// spelled with a trailing `/`): the crate's own surface, tests across
    /// families, shared helpers. `main.rs` is always allowed.
    no_family: &'static [&'static str],
}

/// The two crates whose integration tests are grouped by family. khora-agents
/// keeps the family tests that need the `LaneAgent`, the scheduler or a GPU
/// under `families/<family>/`; khora-lanes keeps its lane tests in
/// `<family>/`, directly under `tests/it/`.
const FAMILY_TEST_TREES: &[FamilyTestTree] = &[
    FamilyTestTree {
        it: "crates/khora-agents/tests/it",
        families: "families",
        no_family: &[
            // What every family offers GORNA, side by side.
            "agent_negotiation_snapshot_test.rs",
            // The asset service's pack loader: no lane, no family.
            "asset_loading_test.rs",
            // What every shipped agent declares to the scheduler.
            "contention_test.rs",
            // The render, shadow and audio flows of one tick together.
            "frame_e2e_test.rs",
            // ECS maintenance: no lane, no family.
            "garbage_collector_test.rs",
            // The `LaneAgent` itself, whatever its family.
            "lane_agent/",
            // Driving any agent through GORNA's protocol (shared helper).
            "protocol.rs",
            // The crate's public surface.
            "public_paths.rs",
        ],
    },
    FamilyTestTree {
        it: "crates/khora-lanes/tests/it",
        families: "",
        no_family: &[
            // The crate's public surface.
            "public_paths.rs",
        ],
    },
];

/// The family an entry under a test tree is named after, if any: its name
/// (without `/` or `.rs`) is a family's folder name or its domain
/// (`FAMILY_FOLDERS`), that name in the plural, or starts with one of them
/// followed by `_` (`audio_family.rs`, `overlay_families/`,
/// `shadows_through_the_frame_graph.rs`, `loading_lane.rs`). The family's
/// folder name is returned; a family `overlay/` groups (`OVERLAY_FAMILIES`)
/// is read as `overlay`.
fn family_named_by(entry: &str) -> Option<&'static str> {
    let stem = entry_module(entry).unwrap_or(entry);
    let named = |name: &str| {
        stem == name || stem == format!("{name}s") || stem.starts_with(&format!("{name}_"))
    };
    FAMILY_FOLDERS
        .iter()
        .find_map(|(family, domain)| (named(family) || named(domain)).then_some(*family))
        .or_else(|| OVERLAY_FAMILIES.iter().any(|f| named(f)).then_some(OVERLAY))
}

/// A family's tests are read in one folder. In each `FamilyTestTree`, every
/// entry directly under `tests/it/` is one of:
///
/// - `main.rs`;
/// - a family folder, named after a family of `FAMILY_FOLDERS` (`audio/`,
///   `loading/`, `overlay/`, `physics/`, `render/`, `script/`, `shadows/`,
///   `skybox/`, `ui/`) — directly under `tests/it/` in khora-lanes, under
///   `tests/it/families/` in khora-agents, whose `families/` holds `mod.rs`
///   and family folders only;
/// - an entry the tree lists as pinning no single family (`no_family`),
///   which is never itself named after a family (`family_named_by`).
///
/// Anything else is a test of one family left outside its folder. Strict:
/// not in the ratchet.
#[test]
fn family_tests_live_in_their_family_folder() {
    let root = repo_root();
    let mut problems = Vec::new();
    for tree in FAMILY_TEST_TREES {
        problems.extend(family_test_problems(
            &root.join(tree.it),
            tree.it,
            tree.families,
            tree.no_family,
        ));
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "tests outside their family's folder (families: {}):\n  {}",
        family_names(),
        problems.join("\n  ")
    );
}

/// What keeps the test tree `it` (shown as `label`) grouped by family,
/// sorted. Each line names the entry as `label/<entry>`.
fn family_test_problems(it: &Path, label: &str, families: &str, no_family: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    if !it.is_dir() {
        problems.push(format!("{label}/: no such folder"));
        return problems;
    }
    let home = if families.is_empty() {
        label.to_owned()
    } else {
        format!("{label}/{families}")
    };
    let is_family_folder = |entry: &str| {
        entry
            .strip_suffix('/')
            .is_some_and(|name| FAMILY_FOLDERS.iter().any(|(family, _)| *family == name))
    };

    for entry in folder_entries(it) {
        if entry == "main.rs" {
            continue;
        }
        if !families.is_empty() && entry == format!("{families}/") {
            for inner in folder_entries(&it.join(families)) {
                if inner != "mod.rs" && !is_family_folder(&inner) {
                    problems.push(format!(
                        "{home}/{inner}: `{families}/` holds `mod.rs` and one folder per family \
                         ({}), nothing else",
                        family_names()
                    ));
                }
            }
            continue;
        }
        if families.is_empty() && is_family_folder(&entry) {
            continue;
        }
        if no_family.contains(&entry.as_str()) {
            if let Some(family) = family_named_by(&entry) {
                problems.push(format!(
                    "{label}/{entry}: listed as pinning no single family, but named after \
                     the `{family}` family: its tests go under {home}/{family}/"
                ));
            }
            continue;
        }
        match family_named_by(&entry) {
            Some(family) => problems.push(format!(
                "{label}/{entry}: a test of the `{family}` family: move it under \
                 {home}/{family}/"
            )),
            None => problems.push(format!(
                "{label}/{entry}: neither a family folder under {home}/ nor listed among \
                 the tests that pin no single family"
            )),
        }
    }
    problems.sort();
    problems
}

/// The family folder names, comma-separated.
fn family_names() -> String {
    FAMILY_FOLDERS
        .iter()
        .map(|(family, _)| *family)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The family test check accepts a tree grouped by family — under
/// `families/` or directly under `tests/it/` — and names a family test left
/// at the root (by its family when its name says it), a family folder put at
/// the root where `families/` is expected, a stray entry in `families/`, a
/// family's tests as one file, and a no-family list that names a family.
#[test]
fn family_test_check_names_what_sits_outside_a_family_folder() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-family-tests-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let write_tree = |base: &Path, files: &[&str]| -> std::io::Result<()> {
        for file in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, "")?;
        }
        Ok(())
    };
    let agents_no_family: &[&str] = &["lane_agent/", "protocol.rs"];
    let lanes_no_family: &[&str] = &["public_paths.rs"];
    let setup = || -> std::io::Result<()> {
        write_tree(
            &dir.join("agents_good"),
            &[
                "main.rs",
                "protocol.rs",
                "lane_agent/mod.rs",
                "families/mod.rs",
                "families/audio/mod.rs",
                "families/overlay/mod.rs",
                "families/script/behavior_budget.rs",
            ],
        )?;
        write_tree(
            &dir.join("agents_bad"),
            &[
                "main.rs",
                "protocol.rs",
                "lane_agent/mod.rs",
                "families/mod.rs",
                "families/physics/mod.rs",
                "families/fixtures.rs",
                "families/spatial_mixing/mod.rs",
                "audio_family.rs",
                "overlay_families/mod.rs",
                "shadows_through_the_frame_graph.rs",
                "physics/mod.rs",
                "behavior_budget.rs",
            ],
        )?;
        write_tree(
            &dir.join("lanes_good"),
            &[
                "main.rs",
                "public_paths.rs",
                "audio/mod.rs",
                "loading/mod.rs",
                "script/watchdog.rs",
            ],
        )?;
        write_tree(
            &dir.join("lanes_bad"),
            &[
                "main.rs",
                "public_paths.rs",
                "loading_lane.rs",
                "overlays/mod.rs",
                "spatial_mixing/mod.rs",
                "script_levels.rs",
                "watchdog.rs",
                "ui.rs",
            ],
        )?;
        Ok(())
    };
    let setup = setup();
    let agents_good =
        family_test_problems(&dir.join("agents_good"), "a", "families", agents_no_family);
    let agents_bad =
        family_test_problems(&dir.join("agents_bad"), "a", "families", agents_no_family);
    let lanes_good = family_test_problems(&dir.join("lanes_good"), "l", "", lanes_no_family);
    let lanes_bad = family_test_problems(&dir.join("lanes_bad"), "l", "", lanes_no_family);
    let listing_a_family = family_test_problems(
        &dir.join("agents_bad"),
        "a",
        "families",
        &[
            "lane_agent/",
            "protocol.rs",
            "shadows_through_the_frame_graph.rs",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(agents_good.is_empty(), "{agents_good:#?}");
    assert!(lanes_good.is_empty(), "{lanes_good:#?}");

    for expected in [
        "a/families/fixtures.rs: `families/` holds",
        "a/families/spatial_mixing/: `families/` holds",
        "a/audio_family.rs: a test of the `audio` family: move it under a/families/audio/",
        "a/overlay_families/: a test of the `overlay` family: move it under a/families/overlay/",
        "a/shadows_through_the_frame_graph.rs: a test of the `shadows` family",
        "a/physics/: a test of the `physics` family: move it under a/families/physics/",
        "a/behavior_budget.rs: neither a family folder",
    ] {
        assert!(
            agents_bad.iter().any(|p| p.starts_with(expected)),
            "expected a problem starting `{expected}`:\n{agents_bad:#?}"
        );
    }
    assert_eq!(agents_bad.len(), 7, "{agents_bad:#?}");

    for expected in [
        "l/loading_lane.rs: a test of the `loading` family: move it under l/loading/",
        "l/overlays/: a test of the `overlay` family: move it under l/overlay/",
        "l/spatial_mixing/: neither a family folder",
        "l/script_levels.rs: a test of the `script` family: move it under l/script/",
        "l/watchdog.rs: neither a family folder",
        "l/ui.rs: a test of the `ui` family: move it under l/ui/",
    ] {
        assert!(
            lanes_bad.iter().any(|p| p.starts_with(expected)),
            "expected a problem starting `{expected}`:\n{lanes_bad:#?}"
        );
    }
    assert_eq!(lanes_bad.len(), 6, "{lanes_bad:#?}");

    assert!(
        listing_a_family.iter().any(|p| p.starts_with(
            "a/shadows_through_the_frame_graph.rs: listed as pinning no single family, \
             but named after the `shadows` family"
        )),
        "{listing_a_family:#?}"
    );
}

/// A name is read as a family's by its whole first word: `script_levels.rs`
/// and `shadow_maps/` are, `behavior_budget.rs`, `scripted.rs` and
/// `uniforms.rs` are not.
#[test]
fn family_names_are_read_by_their_whole_first_word() {
    for (entry, family) in [
        ("audio_family.rs", "audio"),
        ("audio/", "audio"),
        ("loading_lane.rs", "loading"),
        ("overlay_families/", "overlay"),
        ("overlays/", "overlay"),
        ("physics_agent_tests.rs", "physics"),
        ("physics_lane_contacts_test.rs", "physics"),
        ("shadows_through_the_frame_graph.rs", "shadows"),
        ("shadow_maps/", "shadows"),
        ("script_levels.rs", "script"),
        ("ui.rs", "ui"),
    ] {
        assert_eq!(family_named_by(entry), Some(family), "{entry}");
    }
    for entry in [
        "behavior_budget.rs",
        "scripted.rs",
        "uniforms.rs",
        "spatial_mixing/",
        "authored_physics.rs",
        "lane_agent/",
        "public_paths.rs",
        "frame_e2e_test.rs",
    ] {
        assert_eq!(family_named_by(entry), None, "{entry}");
    }
}

/// The three families `overlay/` groups are families too: a name read by the
/// whole first word as `grid`, `wireframe` or `gizmo` is the `overlay`
/// family's, so such a test listed as pinning no single family is caught.
#[test]
fn the_overlay_families_are_read_by_their_own_names() {
    for entry in [
        "grid_lane.rs",
        "grid/",
        "wireframe_frames.rs",
        "gizmos/",
        "gizmo_hits.rs",
    ] {
        assert_eq!(family_named_by(entry), Some(OVERLAY), "{entry}");
    }
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-overlay-family-names-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let setup = (|| -> std::io::Result<()> {
        fs::create_dir_all(dir.join("overlay"))?;
        for file in [
            "main.rs",
            "public_paths.rs",
            "grid_lane.rs",
            "overlay/mod.rs",
        ] {
            fs::write(dir.join(file), "")?;
        }
        Ok(())
    })();
    let problems = family_test_problems(&dir, "l", "", &["public_paths.rs", "grid_lane.rs"]);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");
    assert!(
        problems.iter().any(|p| p.starts_with(
            "l/grid_lane.rs: listed as pinning no single family, but named after the \
             `overlay` family"
        )),
        "{problems:#?}"
    );
}

// ---------------------------------------------------------------------------
// Residencies
// ---------------------------------------------------------------------------

/// The source tree of khora-data.
const KHORA_DATA_SRC: &str = "crates/khora-data/src";

/// The folder under `KHORA_DATA_SRC` that holds every residency.
const RESIDENCY: &str = "residency";

/// What `residency/` holds besides the residencies: its module, which defines
/// `register_residency!`, and the tests of the residencies.
const RESIDENCY_FOLDER_FILES: &[&str] = &["mod.rs", "tests.rs"];

/// The residencies the engine ships, one file each under `residency/`.
const RESIDENCY_FILES: &[&str] = &["image_on_gpu.rs", "mesh_on_gpu.rs", "material_on_gpu.rs"];

/// A residency is read in one folder. In khora-data's production code, a
/// `register_residency!` call and an `impl … Residency for …` (comments do
/// not count) appear only in a file under `crates/khora-data/src/residency/`,
/// whose `mod.rs` also defines the macro. Test files and inline test modules
/// may build residencies of their own. Strict: not in the ratchet.
#[test]
fn residencies_live_in_residency() {
    let root = repo_root();
    let problems = residency_site_problems(&root.join(KHORA_DATA_SRC));
    assert!(
        problems.is_empty(),
        "residencies made outside {KHORA_DATA_SRC}/{RESIDENCY}/ (a `register_residency!` \
         call or an `impl Residency for`):\n  {}",
        problems.join("\n  ")
    );
}

/// One file per residency. `crates/khora-data/src/residency/` holds `mod.rs`,
/// `tests.rs`, and otherwise only files named for a residency,
/// `<what>_on_<device>.rs`, each registering exactly one (one
/// `register_residency!` call outside comments and tests). The engine's
/// three — `image_on_gpu.rs`, `mesh_on_gpu.rs` and `material_on_gpu.rs` — are
/// among them. A new residency adds its file; no helper and no sub-folder sits
/// beside them. Strict: not in the ratchet.
#[test]
fn the_residency_folder_holds_one_file_per_residency() {
    let root = repo_root();
    let problems = residency_folder_problems(&root.join(KHORA_DATA_SRC).join(RESIDENCY));
    assert!(
        problems.is_empty(),
        "{KHORA_DATA_SRC}/{RESIDENCY}/ does not hold one file per residency:\n  {}",
        problems.join("\n  ")
    );
}

/// What the material projection is made of, each as the text that declares
/// it: it moved out of `gpu/projection.rs` into the material residency.
const MATERIAL_PROJECTION_ITEMS: &[&str] = &[
    "fn sync_materials",
    "fn sampler",
    "struct MaterialProjector",
    "fn alpha_mask_cutoff",
    "fn material_textures_ready",
    "fn material_variant",
];

/// The names of the material projection, which `gpu/projection.rs` no longer
/// spells in code, its tests included: the material tests moved with them.
const MATERIAL_PROJECTION_NAMES: &[&str] = &[
    "sync_materials",
    "MaterialProjector",
    "alpha_mask_cutoff",
    "material_textures_ready",
    "material_variant",
];

/// The material residency holds the material projection. Its file,
/// `crates/khora-data/src/residency/material_on_gpu.rs`, declares
/// `ProjectionRegistry::sync_materials` and `sampler`, `MaterialProjector`,
/// `alpha_mask_cutoff`, `material_textures_ready` and `material_variant`;
/// `crates/khora-data/src/gpu/projection.rs` keeps the registry and the mesh
/// half, and spells none of them in code (comments do not count), tests
/// included.
#[test]
fn the_material_projection_lives_in_its_residency() {
    let root = repo_root();
    let data_src = root.join(KHORA_DATA_SRC);
    let mut problems = Vec::new();
    let residency = data_src.join(RESIDENCY).join("material_on_gpu.rs");
    if residency.is_file() {
        let code = strip_rust_comments(&read_text(&residency));
        for item in MATERIAL_PROJECTION_ITEMS {
            if !code.lines().any(|line| names_word(line, item)) {
                problems.push(format!(
                    "{RESIDENCY}/material_on_gpu.rs: does not declare `{item}`"
                ));
            }
        }
    } else {
        problems.push(format!("{RESIDENCY}/material_on_gpu.rs: missing"));
    }
    let projection = data_src.join("gpu/projection.rs");
    if projection.is_file() {
        let code = strip_rust_comments(&read_text(&projection));
        for (index, line) in code.lines().enumerate() {
            for name in MATERIAL_PROJECTION_NAMES {
                if names_word(line, name) {
                    problems.push(format!("gpu/projection.rs:{}: `{name}`", index + 1));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the material projection is not in its residency \
         ({KHORA_DATA_SRC}/{RESIDENCY}/material_on_gpu.rs):\n  {}",
        problems.join("\n  ")
    );
}

/// The production residency sites under `data_src` outside its `residency/`
/// folder, each as `<path relative to data_src>:<line>`, sorted.
fn residency_site_problems(data_src: &Path) -> Vec<String> {
    let home = data_src.join(RESIDENCY);
    let mut problems = Vec::new();
    for path in walk_files(data_src) {
        if !has_extension(&path, "rs")
            || path.starts_with(&home)
            || file_name(&path) == "tests.rs"
            || under_tests_dir(data_src, &path)
        {
            continue;
        }
        let text = read_text(&path);
        if has_inner_cfg_test(&text) {
            continue;
        }
        for line in residency_sites(&text) {
            problems.push(format!("{}:{line}", rel(data_src, &path)));
        }
    }
    problems.sort();
    problems
}

/// What keeps `dir`, a `residency/` folder, from holding one file per
/// residency, sorted. Each line names the entry relative to `dir`.
fn residency_folder_problems(dir: &Path) -> Vec<String> {
    if !dir.is_dir() {
        return vec![format!("{}: no such folder", dir.display())];
    }
    let entries = folder_entries(dir);
    let mut problems = Vec::new();
    for required in RESIDENCY_FOLDER_FILES.iter().chain(RESIDENCY_FILES) {
        if !entries.contains(*required) {
            problems.push(format!("{required}: missing"));
        }
    }
    for entry in &entries {
        if RESIDENCY_FOLDER_FILES.contains(&entry.as_str()) {
            continue;
        }
        let named_for_a_residency = entry
            .strip_suffix(".rs")
            .and_then(|stem| stem.split_once("_on_"))
            .is_some_and(|(what, device)| !what.is_empty() && !device.is_empty());
        if !named_for_a_residency {
            problems.push(format!(
                "{entry}: not a residency (`<what>_on_<device>.rs`) nor one of {}",
                RESIDENCY_FOLDER_FILES.join(", ")
            ));
            continue;
        }
        let calls = register_residency_calls(&read_text(&dir.join(entry))).len();
        if calls != 1 {
            problems.push(format!(
                "{entry}: {calls} `register_residency!` calls; a residency registers one"
            ));
        }
    }
    problems.sort();
    problems
}

/// The 1-based lines of `text` that make a residency: a `register_residency!`
/// call or an `impl` of the `Residency` trait. Comments are excluded, and the
/// inline test module (from its `#[cfg(test)]` on) too.
fn residency_sites(text: &str) -> Vec<usize> {
    let code = strip_rust_comments(text);
    let lines: Vec<&str> = code.lines().take(non_test_lines(text)).collect();
    // An impl header rustfmt wraps (`for` or the bound on the next line) is
    // read whole: from its line up to the one that opens its body.
    let header = |start: usize| {
        let mut joined = String::new();
        for line in &lines[start..] {
            joined.push(' ');
            joined.push_str(line.trim());
            if line.contains('{') || line.contains(';') {
                break;
            }
        }
        joined
    };
    let impls = (0..lines.len())
        .filter(|&index| {
            lines[index]
                .split(|c: char| !is_ident_char(c))
                .any(|word| word == "impl")
                && implements_residency(&header(index))
        })
        .map(|index| index + 1);
    let mut lines: Vec<usize> = register_residency_calls(text)
        .into_iter()
        .chain(impls)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The 1-based lines of `text` that call `register_residency!` — the name,
/// not the tail of a longer one, then, past any whitespace, `(`, `{` or `[` —
/// comments excluded and the inline test module (from its `#[cfg(test)]` on)
/// too. A mention in prose or a comment is not a call.
fn register_residency_calls(text: &str) -> Vec<usize> {
    const MACRO: &str = "register_residency!";
    strip_rust_comments(text)
        .lines()
        .take(non_test_lines(text))
        .enumerate()
        .filter(|(_, line)| {
            line.match_indices(MACRO).any(|(at, _)| {
                !line[..at].chars().next_back().is_some_and(is_ident_char)
                    && line[at + MACRO.len()..]
                        .trim_start()
                        .starts_with(['(', '{', '['])
            })
        })
        .map(|(index, _)| index + 1)
        .collect()
}

/// Whether `line` implements the `Residency` trait: the word `impl`, then the
/// whole word `Residency` (bare or by path) followed by the word `for`.
/// `impl ResidencyError for …` and `impl Display for ResidencyReport` do not.
fn implements_residency(line: &str) -> bool {
    let Some(start) = line.match_indices("impl").find_map(|(at, word)| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        (!before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)).then_some(at)
    }) else {
        return false;
    };
    let rest = &line[start..];
    rest.match_indices("Residency").any(|(at, word)| {
        let before = rest[..at].chars().next_back();
        let after = &rest[at + word.len()..];
        !before.is_some_and(is_ident_char)
            && after.starts_with(char::is_whitespace)
            && after
                .trim_start()
                .strip_prefix("for")
                .is_some_and(|tail| !tail.chars().next().is_some_and(is_ident_char))
    })
}

/// The residency-site check accepts residencies made under `residency/`, the
/// macro's definition and its doc example, test files and inline test
/// modules, prose and other traits; it names each file outside `residency/`
/// that registers or implements a residency, with its line.
#[test]
fn residency_check_names_a_residency_made_outside_its_folder() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-residency-sites-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    const MACRO_DEFINITION: &str = "\
/// ```ignore
/// register_residency!(name = ImageOnGpu::NAME, run = image_on_gpu_system);
/// ```
#[macro_export]
macro_rules! register_residency {
    (name = $name:expr, run = $run:path $(,)?) => {
        $crate::register_residency!(name = $name, run = $run, after = []);
    };
}
";
    let write_tree = |base: &Path, files: &[(&str, &str)]| -> std::io::Result<()> {
        for (file, text) in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    };
    let good: &[(&str, &str)] = &[
        ("residency/mod.rs", MACRO_DEFINITION),
        (
            "residency/mesh_on_gpu.rs",
            "impl Residency for MeshOnGpu {}\nfn run() {}\ncrate::register_residency!(name = MeshOnGpu::NAME, run = run);\n",
        ),
        (
            "residency/material_on_gpu.rs",
            "crate::register_residency!(\n    name = \"gpu_material_sync\",\n    run = run,\n    after = [\"gpu_mesh_sync\"],\n);\n",
        ),
        (
            "ecs/systems/mod.rs",
            "//! Residencies register with `register_residency!(…)`.\n\
             /* register_residency!(name = A, run = b); impl Residency for A {} */\n\
             const HELP: &str = \"see `register_residency!`\";\n\
             pub struct ResidencyReport;\n\
             impl Display for ResidencyReport {}\n\
             impl ResidencyError for Refusal {}\n\
             fn implements_residency() {}\n",
        ),
        (
            "ui/tests.rs",
            "impl Residency for Fake {}\nregister_residency!(name = \"fake\", run = run);\n",
        ),
        (
            "gpu/projection.rs",
            "pub struct ProjectionRegistry;\n#[cfg(test)]\nmod tests {\n    impl Residency for Fake {}\n    register_residency!(name = \"fake\", run = run);\n}\n",
        ),
        ("ecs/tests/fixture.rs", "impl Residency for Fixture {}\n"),
    ];
    let bad: &[(&str, &str)] = &[
        ("residency/mod.rs", MACRO_DEFINITION),
        (
            "ecs/systems/mesh_upload.rs",
            "use x;\ncrate::register_residency!(name = MeshOnGpu::NAME, run = run);\n",
        ),
        (
            "gpu/projection.rs",
            "pub struct ProjectionRegistry;\nimpl<'a> khora_core::residency::Residency for MaterialOnGpu<'a> {}\n",
        ),
        ("ui/atlas.rs", "pub struct Atlas;\n\nimpl Residency for Atlas {\n}\n"),
    ];
    let setup = || -> std::io::Result<()> {
        write_tree(&dir.join("good"), good)?;
        write_tree(&dir.join("bad"), bad)?;
        Ok(())
    };
    let setup = setup();
    let good_problems = residency_site_problems(&dir.join("good"));
    let bad_problems = residency_site_problems(&dir.join("bad"));
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(good_problems.is_empty(), "{good_problems:#?}");
    assert_eq!(
        bad_problems,
        vec![
            "ecs/systems/mesh_upload.rs:2".to_owned(),
            "gpu/projection.rs:2".to_owned(),
            "ui/atlas.rs:3".to_owned(),
        ]
    );
}

/// An `impl` of `Residency` whose header is longer than a line is written by
/// rustfmt with `for` on the next line; the residency-site check still names
/// it, on the line of its `impl`.
#[test]
fn residency_check_names_an_impl_whose_header_rustfmt_splits() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-residency-split-impl-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let setup = (|| -> std::io::Result<()> {
        fs::create_dir_all(dir.join("residency"))?;
        fs::create_dir_all(dir.join("gpu"))?;
        fs::write(dir.join("residency/mod.rs"), "mod mesh_on_gpu;\n")?;
        fs::write(
            dir.join("gpu/projection.rs"),
            "pub struct ProjectionRegistry;\n\
             impl<'a> khora_core::residency::Residency\n    \
             for MaterialProjectionOnTheGraphicsDevice<'a, SomeParameter>\n\
             {\n}\n",
        )
    })();
    let problems = residency_site_problems(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(problems, vec!["gpu/projection.rs:2".to_owned()]);
}

/// The residency-folder check accepts the engine's residencies and a new one,
/// and names a residency missing, a residency file that registers none or two,
/// a helper file and a sub-folder.
#[test]
fn residency_folder_check_names_what_is_not_one_file_per_residency() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-residency-folder-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    const ONE: &str =
        "impl Residency for X {}\ncrate::register_residency!(name = X::NAME, run = run);\n";
    const TWO: &str = "crate::register_residency!(name = \"a\", run = a);\ncrate::register_residency!(name = \"b\", run = b);\n";
    const NONE: &str =
        "//! Registers with `register_residency!`.\nimpl Residency for MeshOnGpu {}\n";
    const MODULE: &str = "macro_rules! register_residency {\n    () => { $crate::register_residency!(name = n, run = r); };\n}\n";
    let write_tree = |base: &Path, files: &[(&str, &str)]| -> std::io::Result<()> {
        for (file, text) in files {
            let path = base.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    };
    let good: &[(&str, &str)] = &[
        ("mod.rs", MODULE),
        (
            "tests.rs",
            "register_residency!(name = \"fake\", run = run);\n",
        ),
        ("image_on_gpu.rs", ONE),
        ("mesh_on_gpu.rs", ONE),
        ("material_on_gpu.rs", ONE),
        ("clip_on_cpu.rs", ONE),
    ];
    let bad: &[(&str, &str)] = &[
        ("mod.rs", MODULE),
        ("tests.rs", ""),
        ("image_on_gpu.rs", ONE),
        ("mesh_on_gpu.rs", NONE),
        ("atlas_on_gpu.rs", TWO),
        ("helpers.rs", ""),
        ("material/mod.rs", ONE),
    ];
    let setup = || -> std::io::Result<()> {
        write_tree(&dir.join("good"), good)?;
        write_tree(&dir.join("bad"), bad)?;
        Ok(())
    };
    let setup = setup();
    let good_problems = residency_folder_problems(&dir.join("good"));
    let bad_problems = residency_folder_problems(&dir.join("bad"));
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert!(good_problems.is_empty(), "{good_problems:#?}");
    for expected in [
        "atlas_on_gpu.rs: 2 `register_residency!` calls",
        "helpers.rs: not a residency",
        "material/: not a residency",
        "material_on_gpu.rs: missing",
        "mesh_on_gpu.rs: 0 `register_residency!` calls",
    ] {
        assert!(
            bad_problems.iter().any(|p| p.starts_with(expected)),
            "expected a problem starting `{expected}`:\n{bad_problems:#?}"
        );
    }
    assert_eq!(bad_problems.len(), 5, "{bad_problems:#?}");
}

/// `text` with every `//` and `/* */` comment blanked out (doc comments
/// included), keeping string and character literals and every newline, so
/// line numbers survive.
fn strip_rust_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                out.push('"');
                i += 1;
            }
        } else if c == 'r'
            && (next == Some('"') || next == Some('#'))
            && !chars
                .get(i.wrapping_sub(1))
                .is_some_and(|p| p.is_alphanumeric() || *p == '_')
        {
            // A raw string: r"…", r#"…"#, …
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            let closing: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body_start = j + 1;
            let rest: String = chars[body_start..].iter().collect();
            let end = rest.find(&closing).map_or(chars.len(), |at| {
                body_start + rest[..at].chars().count() + closing.chars().count()
            });
            out.extend(chars[i..end].iter());
            i = end;
        } else if c == '\'' {
            // A character literal ('x', '\n', '\''); otherwise a lifetime.
            let literal_end = match (next, chars.get(i + 2), chars.get(i + 3)) {
                // The escaped character sits at `i + 2`; the closing quote follows.
                (Some('\\'), _, _) => chars
                    .get(i + 3..)
                    .and_then(|rest| rest.iter().position(|&ch| ch == '\''))
                    .map(|at| i + 3 + at + 1),
                (Some(_), Some('\''), _) => Some(i + 3),
                _ => None,
            };
            match literal_end {
                Some(end) if end <= chars.len() => {
                    out.extend(chars[i..end].iter());
                    i = end;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `line` holds `word` as a whole identifier.
fn names_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

/// The 1-based lines of `code` that read the `runtime` field of a binding
/// holding an `EngineContext`.
fn runtime_field_reads(code: &str) -> Vec<usize> {
    let mut bindings = BTreeSet::new();
    for (at, _) in code.match_indices("EngineContext") {
        let before = code[..at].trim_end();
        // `name: &mut EngineContext` / `name: &EngineContext`
        let typed = before
            .strip_suffix("&mut")
            .or_else(|| before.strip_suffix('&'))
            .map(str::trim_end)
            .and_then(|b| b.strip_suffix(':'));
        // `let [mut] name = EngineContext::…`
        let built = before.strip_suffix('=');
        if let Some(prefix) = typed.or(built) {
            let name: String = prefix
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| is_ident_char(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if !name.is_empty() && name != "self" {
                bindings.insert(name);
            }
        }
    }

    let mut lines = BTreeSet::new();
    for (index, line) in code.lines().enumerate() {
        for name in &bindings {
            let access = format!("{name}.runtime");
            let reads = line.match_indices(&access).any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + access.len()..].chars().next();
                !before.is_some_and(|c| is_ident_char(c) || c == '.')
                    && !after.is_some_and(|c| is_ident_char(c) || c == '(')
            });
            if reads {
                lines.insert(index + 1);
            }
        }
    }
    lines.into_iter().collect()
}

/// The comment stripper keeps code and literals, blanks comments, and keeps
/// every line where it was.
#[test]
fn comment_stripping_keeps_code_and_lines() {
    let text = "use a::World; // World\n/// World\nlet s = \"// not a comment\"; /* World\n World */ let c = '\"';\nfn f<'a>() {}\n";
    let code = strip_rust_comments(text);
    let lines: Vec<&str> = code.lines().collect();

    assert_eq!(lines.len(), 5, "every line survives: {code:?}");
    assert!(
        names_word(lines[0], "World"),
        "code is kept: {:?}",
        lines[0]
    );
    assert!(!lines[0].contains("// World"), "a line comment is blanked");
    assert!(!names_word(lines[1], "World"), "a doc comment is blanked");
    assert!(
        lines[2].contains("\"// not a comment\""),
        "a string is kept"
    );
    assert!(!names_word(lines[2], "World") && !names_word(lines[3], "World"));
    assert!(lines[3].contains("let c = '\"';"), "a char literal is kept");
    assert!(
        lines[4].contains("fn f<'a>() {}"),
        "a lifetime is not a literal"
    );
    assert!(!names_word("RenderWorld and WorldAccess", "World"));
}

/// The runtime-field detector sees a read through a parameter or a built
/// context, and ignores an agent's own `runtime` field and other types.
#[test]
fn runtime_field_reads_are_seen() {
    let code = "\
fn execute(&mut self, context: &mut EngineContext<'_>) {
    let a = context.runtime.resources.get::<X>();
    let b = self.runtime.clone();
    let c = lane_ctx.runtime.clone();
}
fn other(ctx: &EngineContext<'_>) {
    let d = Arc::clone(&ctx.runtime);
}
fn built() {
    let mut engine_ctx = EngineContext::for_agent(a, b, c, d, e, f);
    let e = engine_ctx.runtime.services.get::<Y>();
}
";
    assert_eq!(runtime_field_reads(code), vec![2, 7, 11]);
}

// ---------------------------------------------------------------------------
// Public type names
// ---------------------------------------------------------------------------

/// Members of `crates/` whose type names are not the engine's: the example
/// game, whose types are a game's own.
const TYPE_NAMES_NOT_SCANNED: &[&str] = &["sandbox"];

/// Public type names two items may share, each with why (a type re-declared
/// per backend, say). An entry with no reason, or whose name is no longer
/// declared twice, fails the check.
const TYPE_NAMES_SHARED: &[(&str, &str)] = &[
    ("UiBorder", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
    ("UiColor", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
    ("UiImage", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
    ("UiNode", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
    ("UiText", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
    ("UiTransform", "khora-data's UI component mirrors khora-core's UI value type of the same name, converted with `From`"),
];

/// A public type name means one thing. Across the non-test code of the
/// workspace's crates (`crates/*/src`, the example game `sandbox` aside), a
/// `pub struct`, `pub enum`, `pub union`, `pub trait` (`pub unsafe trait`
/// too) or `pub type` declares each name once, at any indentation; comments
/// do not count. Not counted either:
///
/// - `pub(crate)`, `pub(super)` and `pub(in …)` items;
/// - test code: a `tests.rs`, a file under a `tests/` folder, a file carrying
///   `#![cfg(test)]` or declared by `#[cfg(test)] mod` (a folder module with
///   all it holds), the inline test module and what follows it, and an item
///   under `#[cfg(test)]`;
/// - what sits beside `src/` (`tests/`, `benches/`, `examples/`) and `xtask`;
/// - a name a macro builds (`pub struct $name`).
///
/// Strict: a name two items share on purpose goes in `TYPE_NAMES_SHARED`
/// with why.
#[test]
fn no_public_type_name_is_declared_twice() {
    let root = repo_root();
    let problems = type_name_problems(&root, TYPE_NAMES_SHARED);
    assert!(
        problems.is_empty(),
        "public type names declared more than once in the workspace's non-test code \
         (rename one side, or list the name in TYPE_NAMES_SHARED with why):\n  {}",
        problems.join("\n  ")
    );
}

/// The public type names declared more than once under `root/crates/*/src`
/// and not in `shared`, each as `<Name>: declared <n> times: <path>:<line>, …`
/// (paths relative to `root`), and every `shared` entry with no reason or no
/// longer declared twice. Sorted.
fn type_name_problems(root: &Path, shared: &[(&str, &str)]) -> Vec<String> {
    let mut sites: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for member in child_dirs(&root.join("crates")) {
        if TYPE_NAMES_NOT_SCANNED.contains(&file_name(&member).as_str()) {
            continue;
        }
        let src = member.join("src");
        if !src.is_dir() {
            continue;
        }
        for (path, text) in non_test_rust_files(&src) {
            for (line, name) in public_type_declarations(&text) {
                sites
                    .entry(name)
                    .or_default()
                    .push(format!("{}:{line}", rel(root, &path)));
            }
        }
    }
    let mut problems = Vec::new();
    for (name, reason) in shared {
        if reason.trim().is_empty() {
            problems.push(format!("{name}: listed as shared without a reason"));
        }
        let count = sites.get(*name).map_or(0, Vec::len);
        if count < 2 {
            problems.push(format!(
                "{name}: listed as shared, but declared {count} time(s): remove it from the list"
            ));
        }
    }
    for (name, places) in &mut sites {
        if places.len() > 1 && !shared.iter().any(|(listed, _)| listed == name) {
            places.sort();
            problems.push(format!(
                "{name}: declared {} times: {}",
                places.len(),
                places.join(", ")
            ));
        }
    }
    problems.sort();
    problems
}

/// The `.rs` files under `src` that are not test code, with their text:
/// neither `tests.rs`, nor under a `tests/` folder, nor `#![cfg(test)]`, nor
/// declared by a `#[cfg(test)] mod` (a folder module, with all it holds).
fn non_test_rust_files(src: &Path) -> Vec<(PathBuf, String)> {
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

/// The public types `text` declares outside its tests, each as its 1-based
/// line and its name: a `pub struct`, `pub enum`, `pub union`, `pub trait`
/// (`pub unsafe trait`) or `pub type`, at any indentation. Comments are
/// excluded, the inline test module and what follows it too, and an item
/// whose attributes (spread over lines or not) hold `#[cfg(test)]`.
fn public_type_declarations(text: &str) -> Vec<(usize, String)> {
    let code = strip_rust_comments(text);
    let mut found = Vec::new();
    // The attributes stacked above the next item, and the brackets still open
    // in one that spans lines.
    let mut attrs = String::new();
    let mut open = 0usize;
    let in_tests = inline_test_module_lines(&code);
    for (index, raw) in code.lines().enumerate() {
        if in_tests.contains(&index) {
            attrs.clear();
            continue;
        }
        let mut rest = raw.trim();
        loop {
            if open > 0 {
                let mut consumed = rest.len();
                for (at, ch) in rest.char_indices() {
                    match ch {
                        '[' => open += 1,
                        ']' => {
                            open -= 1;
                            if open == 0 {
                                consumed = at + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                attrs.push_str(&rest[..consumed]);
                rest = rest[consumed..].trim_start();
                if open > 0 {
                    break;
                }
            } else if let Some(after) = rest.strip_prefix("#[") {
                attrs.push_str("#[");
                open = 1;
                rest = after;
            } else {
                break;
            }
        }
        if open > 0 || rest.is_empty() {
            continue;
        }
        let compact: String = attrs.chars().filter(|ch| !ch.is_whitespace()).collect();
        let under_cfg_test =
            compact.contains("#[cfg(test)]") || compact.contains("#[cfg(all(test,");
        attrs.clear();
        if under_cfg_test {
            continue;
        }
        if let Some(name) = public_type_name(rest) {
            found.push((index + 1, name));
        }
    }
    found
}

/// The 0-based lines of `code` (comments already blanked) inside an inline
/// test module: from a `mod name {` under `#[cfg(test)]` to the brace that
/// closes it. What follows the module is code again. Braces in string and
/// character literals are not counted.
fn inline_test_module_lines(code: &str) -> BTreeSet<usize> {
    let mut lines = BTreeSet::new();
    let mut cfg_test = false;
    // The brace depth inside the test module being skipped.
    let mut depth: Option<usize> = None;
    for (index, raw) in code.lines().enumerate() {
        let line = raw.trim();
        if depth.is_none() {
            let compact: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
            if compact.starts_with("#[cfg(test)]") || compact.starts_with("#[cfg(all(test,") {
                cfg_test = true;
            }
            let item = line
                .rsplit_once(']')
                .map_or(line, |(_, after)| after)
                .trim_start();
            let item = item.strip_prefix("pub ").unwrap_or(item).trim_start();
            if cfg_test && item.starts_with("mod ") && item.contains('{') {
                depth = Some(0);
            } else if !line.is_empty() && !line.starts_with("#[") {
                cfg_test = false;
            }
        }
        let Some(open) = depth.as_mut() else {
            continue;
        };
        lines.insert(index);
        let mut in_string = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' if in_string => {
                    chars.next();
                }
                '"' => in_string = !in_string,
                '\'' if !in_string => {
                    // A character literal (`'{'`, `'\''`), not a lifetime.
                    let mut ahead = chars.clone();
                    let first = ahead.next();
                    let closes = if first == Some('\\') {
                        ahead.next();
                        ahead.next() == Some('\'')
                    } else {
                        ahead.next() == Some('\'')
                    };
                    if closes {
                        chars = ahead;
                    }
                }
                '{' if !in_string => *open += 1,
                '}' if !in_string => {
                    *open -= 1;
                    if *open == 0 {
                        depth = None;
                        cfg_test = false;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    lines
}

/// The name an item declares when it is a `pub` (not `pub(…)`) `struct`,
/// `enum`, `union`, `trait` (`unsafe trait` too) or `type`; `None` for any
/// other item, and for a name a macro builds (`$name`).
fn public_type_name(item: &str) -> Option<String> {
    let after_pub = item.strip_prefix("pub")?;
    if !after_pub.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = after_pub.trim_start();
    if let Some(after) = rest.strip_prefix("unsafe") {
        if after.starts_with(char::is_whitespace) {
            rest = after.trim_start();
        }
    }
    for keyword in ["struct", "enum", "union", "trait", "type"] {
        let Some(after) = rest.strip_prefix(keyword) else {
            continue;
        };
        if !after.starts_with(char::is_whitespace) {
            continue;
        }
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|ch| is_ident_char(*ch))
            .collect();
        return (!name.is_empty() && !name.starts_with(|ch: char| ch.is_ascii_digit()))
            .then_some(name);
    }
    None
}

/// The type-name check names each public type name declared twice across
/// crates, with every site, and reads past what is not a public declaration
/// in non-test code: a comment, a string, a `pub(crate)` or `pub(super)`
/// item, an item under `#[cfg(test)]` (its attributes spread over lines or
/// not), the inline test module, a `tests.rs`, a `tests/` folder, a
/// `#![cfg(test)]` file, a `#[cfg(test)] mod` folder, what sits beside `src/`,
/// the example game, a macro's `$name`. A shared name listed with a reason
/// passes; one listed without a reason, or declared once, is named.
#[test]
fn type_name_check_names_each_public_name_declared_twice() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-type-names-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    const ALPHA_LIB: &str = "\
//! Alpha: `pub struct Commented;`
pub struct Shared;
pub(crate) struct Private;
/* pub struct Commented; */
#[cfg(test)]
pub struct OnlyInTests;
#[derive(
    Debug,
    Clone,
)]
pub enum Run {
    A,
}
const NOTE: &str = \"a pub struct in a string\";
#[cfg(test)]
#[derive(
    Debug,
)]
pub struct OnlyInTests;
#[cfg(not(test))]
pub struct NotInTests;
macro_rules! make {
    ($name:ident) => {
        pub struct $name;
    };
}
pub unsafe trait Unsafe {}
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests {
    pub struct Shared;
    pub struct OnlyInTests;
}
";
    const BETA_LIB: &str = "\
pub struct Shared;
pub(super) enum Private {}
pub mod inner {
    pub type Run = u8;
}
pub struct NotInTests;
pub struct Lonely;
";
    let files: &[(&str, &str)] = &[
        ("crates/alpha/src/lib.rs", ALPHA_LIB),
        (
            "crates/alpha/src/shared.rs",
            "pub struct Allowed;\npub struct Twin;\n",
        ),
        ("crates/alpha/src/tests.rs", "pub struct Shared;\n"),
        ("crates/alpha/src/fixtures/mod.rs", "pub struct Shared;\n"),
        ("crates/alpha/src/fixtures/more.rs", "pub struct Shared;\n"),
        ("crates/beta/src/lib.rs", BETA_LIB),
        (
            "crates/beta/src/more.rs",
            "pub trait Unsafe {}\npub struct Allowed;\npub struct Twin;\n",
        ),
        ("crates/beta/src/tests/mod.rs", "pub struct Shared;\n"),
        (
            "crates/beta/src/generated.rs",
            "#![cfg(test)]\npub struct Shared;\n",
        ),
        ("crates/beta/tests/it.rs", "pub struct Shared;\n"),
        ("crates/beta/benches/bench.rs", "pub struct Shared;\n"),
        ("crates/sandbox/src/main.rs", "pub struct Shared;\n"),
    ];
    let setup = (|| -> std::io::Result<()> {
        for (file, text) in files {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    })();
    let shared: &[(&str, &str)] = &[
        ("Allowed", "re-declared per backend"),
        ("Twin", ""),
        ("Lonely", "was declared twice"),
    ];
    let problems = type_name_problems(&dir, shared);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "Lonely: listed as shared, but declared 1 time(s): remove it from the list".to_owned(),
            "NotInTests: declared 2 times: crates/alpha/src/lib.rs:21, crates/beta/src/lib.rs:6"
                .to_owned(),
            "Run: declared 2 times: crates/alpha/src/lib.rs:11, crates/beta/src/lib.rs:4"
                .to_owned(),
            "Shared: declared 2 times: crates/alpha/src/lib.rs:2, crates/beta/src/lib.rs:1"
                .to_owned(),
            "Twin: listed as shared without a reason".to_owned(),
            "Unsafe: declared 2 times: crates/alpha/src/lib.rs:27, crates/beta/src/more.rs:1"
                .to_owned(),
        ]
    );
}

/// Code after an inline test module is still the crate's code: a public type
/// declared below `#[cfg(test)] mod … { … }` counts like one above it.
/// `khora-core`'s `resource/texture.rs` declares `TextureId`,
/// `TextureViewId` and `SamplerId` below its `cpu_texture_tests` module.
#[test]
fn type_name_check_reads_the_code_after_an_inline_test_module() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-type-names-after-tests-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    const ALPHA_LIB: &str = "\
pub struct Above;
#[cfg(test)]
mod cpu_texture_tests {
    pub struct InTests;
}
impl Above {}
pub struct Twin;
";
    let files: &[(&str, &str)] = &[
        ("crates/alpha/src/lib.rs", ALPHA_LIB),
        (
            "crates/beta/src/lib.rs",
            "pub struct Twin;\npub struct InTests;\n",
        ),
    ];
    let setup = (|| -> std::io::Result<()> {
        for (file, text) in files {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    })();
    let problems = type_name_problems(&dir, &[]);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "Twin: declared 2 times: crates/alpha/src/lib.rs:7, crates/beta/src/lib.rs:1"
                .to_owned()
        ]
    );
}

// ---------------------------------------------------------------------------
// In-file order
// ---------------------------------------------------------------------------
//
// A file reads in one order: module doc, imports, constants, the primary
// type, its impls, private helpers, tests. The checks below read a file's
// top-level items (`top_level_items`) and name what is out of that order.
// They scan the non-test files of every Rust `src/` tree (`crates/*/src`,
// `examples/*/src`, `xtask/src`): a `tests.rs`, a file under a `tests/`
// folder, a `#![cfg(test)]` file and a file declared by `#[cfg(test)] mod`
// are test code, and are not read.

/// What a top-level item is, as far as the in-file order checks need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    /// `use`, and `extern crate`.
    Use,
    Const,
    Static,
    /// `type Name = …;`
    TypeAlias,
    Struct,
    Enum,
    Union,
    Trait,
    Impl,
    Fn,
    Mod,
    /// A macro call (`inventory::submit! { … }`) or a `macro_rules!`.
    Macro,
    /// Anything else (an `extern "C" { … }` block).
    Other,
}

impl ItemKind {
    /// The keyword a report names the item by.
    fn word(self) -> &'static str {
        match self {
            ItemKind::Use => "use",
            ItemKind::Const => "const",
            ItemKind::Static => "static",
            ItemKind::TypeAlias => "type",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Union => "union",
            ItemKind::Trait => "trait",
            ItemKind::Impl => "impl",
            ItemKind::Fn => "fn",
            ItemKind::Mod => "mod",
            ItemKind::Macro => "macro",
            ItemKind::Other => "item",
        }
    }
}

/// A top-level item of a Rust file.
#[derive(Debug)]
struct TopItem {
    /// 1-based line of the item itself, below its attributes.
    line: usize,
    kind: ItemKind,
    /// The name the item declares; for an `impl`, the name of the type it is
    /// for (`impl<T> Trait for Foo<T>` → `Foo`); `_` for `const _`; empty when
    /// the item names nothing (`use`, a macro call).
    name: String,
    /// No visibility at all: not `pub`, nor `pub(crate)`, `pub(super)`, ….
    private: bool,
    /// Under `#[cfg(test)]` or `#[cfg(all(test, …))]`.
    cfg_test: bool,
    /// A `mod` whose body is written in place (`mod x { … }`), not in a file
    /// of its own (`mod x;`).
    inline: bool,
    /// The item's code, its outer attributes included, with comments and the
    /// contents of literals blanked.
    code: String,
}

impl TopItem {
    /// `fn name`, `struct Name`, `impl Name`, … as a report shows it.
    fn label(&self) -> String {
        if self.name.is_empty() {
            self.kind.word().to_owned()
        } else {
            format!("{} {}", self.kind.word(), self.name)
        }
    }

    /// A test module: an inline `#[cfg(test)] mod x { … }`, or the
    /// declaration `#[cfg(test)] mod tests;` of the file holding the tests.
    fn is_test_module(&self) -> bool {
        self.kind == ItemKind::Mod && self.cfg_test && (self.inline || self.name == "tests")
    }

    /// An inline `#[cfg(test)] mod x { … }`.
    fn is_inline_test_module(&self) -> bool {
        self.kind == ItemKind::Mod && self.cfg_test && self.inline
    }
}

/// Each non-test Rust file of `root`'s Rust `src/` trees, as its path
/// relative to `root` and its top-level items. Sorted by path.
fn order_scanned_files(root: &Path) -> Vec<(String, Vec<TopItem>)> {
    let mut files = Vec::new();
    for src in rust_src_roots(root) {
        for (path, text) in non_test_rust_files(&src) {
            files.push((rel(root, &path), top_level_items(&text)));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// The top-level items of a Rust file, in order.
///
/// Comments and the contents of string and character literals are blanked
/// first, so a brace, a `;` or a line at column 0 inside one is not read as
/// code. An item runs from its first token to the `;` that ends it at bracket
/// depth 0 or, for an item with a body (`fn`, `impl`, `struct … { }`,
/// `mod … { }`, a macro call in braces), to the brace that closes that body.
/// A `const`, `static`, `type` or `use` always runs to its `;`, so
/// `const X: Foo = Foo { … };` is one item. Inner attributes (`#![…]`) are
/// skipped; outer attributes belong to the item below them.
fn top_level_items(text: &str) -> Vec<TopItem> {
    let code = blank_literals(&strip_rust_comments(text));
    let chars: Vec<char> = code.chars().collect();
    let newlines = |range: &[char]| range.iter().filter(|&&ch| ch == '\n').count();
    let mut items = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == ';' {
            if c == '\n' {
                line += 1;
            }
            i += 1;
            continue;
        }
        if c == '#' {
            let inner = chars.get(i + 1) == Some(&'!');
            let open = if inner { i + 2 } else { i + 1 };
            if chars.get(open) == Some(&'[') {
                let end = closing_bracket(&chars, open);
                line += newlines(&chars[i..end]);
                if !inner {
                    attrs.push(chars[i..end].iter().collect());
                }
                i = end;
                continue;
            }
        }

        // An item starts here. Its header runs to the first `{` or `;` outside
        // parentheses and brackets.
        let mut depth = 0usize;
        let mut stop = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(i) {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '{' | ';' if depth == 0 => {
                    stop = j;
                    break;
                }
                _ => {}
            }
        }
        let header: String = chars[i..stop].iter().collect();
        let (kind, name, private) = classify_item(&header);
        let to_semicolon = matches!(
            kind,
            ItemKind::Const | ItemKind::Static | ItemKind::TypeAlias | ItemKind::Use
        );
        let mut depth = 0usize;
        let mut end = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(i) {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && ch == '}' && !to_semicolon {
                        end = j + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
        }
        items.push(TopItem {
            line,
            kind,
            name,
            private,
            cfg_test: attrs.iter().any(|attr| is_cfg_test(attr)),
            inline: kind == ItemKind::Mod && chars.get(stop) == Some(&'{'),
            code: attrs.concat() + &chars[i..end].iter().collect::<String>(),
        });
        attrs.clear();
        line += newlines(&chars[i..end]);
        i = end;
    }
    items
}

/// The index just past the `]` closing the `[` at `open`, or the end.
fn closing_bracket(chars: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (at, &ch) in chars.iter().enumerate().skip(open) {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    chars.len()
}

/// `code` (comments already blanked) with the contents of every string
/// literal (`"…"`, `r#"…"#`, `b"…"`, `br"…"`) and character literal blanked,
/// quotes and newlines kept, so line numbers survive.
fn blank_literals(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let blank = |ch: char| if ch == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let after_ident = i > 0 && is_ident_char(chars[i - 1]);
        let raw_quote_search = match (c, chars.get(i + 1)) {
            ('r', _) if !after_ident => Some(i + 1),
            ('b', Some('r')) if !after_ident => Some(i + 2),
            _ => None,
        };
        if let Some(mut j) = raw_quote_search {
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                out.extend(&chars[i..=j]);
                let mut k = j + 1;
                while k < chars.len()
                    && !(chars[k] == '"' && (1..=hashes).all(|h| chars.get(k + h) == Some(&'#')))
                {
                    out.push(blank(chars[k]));
                    k += 1;
                }
                let end = (k + 1 + hashes).min(chars.len());
                out.extend(&chars[k.min(chars.len())..end]);
                i = end;
                continue;
            }
        }
        if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(' ');
                    out.push(blank(chars[i + 1]));
                    i += 2;
                    continue;
                }
                out.push(blank(chars[i]));
                i += 1;
            }
            if i < chars.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        if c == '\'' {
            // A character literal ('x', '\n', '\''); otherwise a lifetime.
            let end = match (chars.get(i + 1), chars.get(i + 2)) {
                (Some('\\'), _) => chars
                    .get(i + 3..)
                    .and_then(|rest| rest.iter().position(|&ch| ch == '\''))
                    .map(|at| i + 3 + at + 1),
                (Some(_), Some('\'')) => Some(i + 3),
                _ => None,
            };
            if let Some(end) = end.filter(|&end| end <= chars.len()) {
                out.push('\'');
                out.extend(std::iter::repeat_n(' ', end - i - 2));
                out.push('\'');
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The leading identifier of `text` (empty when it starts otherwise).
fn leading_ident(text: &str) -> &str {
    let len = text
        .char_indices()
        .find(|&(_, ch)| !is_ident_char(ch))
        .map_or(text.len(), |(at, _)| at);
    &text[..len]
}

/// What an item's header (its text up to its body or its `;`) declares: its
/// kind, its name, and whether it has no visibility at all.
fn classify_item(header: &str) -> (ItemKind, String, bool) {
    let header = header.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rest = header.as_str();
    let private = leading_ident(rest) != "pub";
    if !private {
        rest = rest["pub".len()..].trim_start();
        if rest.starts_with('(') {
            rest = rest.find(')').map_or("", |at| &rest[at + 1..]).trim_start();
        }
    }
    loop {
        let word = leading_ident(rest);
        let after = rest[word.len()..].trim_start();
        let next = leading_ident(after);
        let kind = match word {
            "default" | "async" | "unsafe" | "safe" => {
                rest = after;
                continue;
            }
            "const" if matches!(next, "fn" | "unsafe" | "async" | "extern") => {
                rest = after;
                continue;
            }
            "extern" if next == "crate" => return (ItemKind::Use, String::new(), private),
            "extern" => {
                // `extern "C" fn …` is a function; `extern "C" { … }` a block.
                let after_abi = match after.strip_prefix('"') {
                    Some(abi) => abi.find('"').map_or("", |at| &abi[at + 1..]).trim_start(),
                    None => after,
                };
                if leading_ident(after_abi).is_empty() {
                    return (ItemKind::Other, String::new(), private);
                }
                rest = after_abi;
                continue;
            }
            "impl" => return (ItemKind::Impl, impl_self_type(after), private),
            "const" => ItemKind::Const,
            "static" => ItemKind::Static,
            "type" => ItemKind::TypeAlias,
            "struct" => ItemKind::Struct,
            "enum" => ItemKind::Enum,
            "union" => ItemKind::Union,
            "trait" => ItemKind::Trait,
            "fn" => ItemKind::Fn,
            "mod" => ItemKind::Mod,
            "use" => return (ItemKind::Use, String::new(), private),
            "macro_rules" => return (ItemKind::Macro, String::new(), private),
            _ if !word.is_empty() && (after.starts_with('!') || after.starts_with("::")) => {
                return (ItemKind::Macro, String::new(), private);
            }
            _ => return (ItemKind::Other, String::new(), private),
        };
        let name = if kind == ItemKind::Static && next == "mut" {
            leading_ident(after["mut".len()..].trim_start())
        } else {
            next
        };
        return (kind, name.to_owned(), private);
    }
}

/// The name of the type an `impl` is for, from the header text after `impl`:
/// `<T> Trait for Foo<T>` → `Foo`, `Foo` → `Foo`, `<T> Trait for &mut
/// a::Bar` → `Bar`.
fn impl_self_type(after_impl: &str) -> String {
    let mut rest = after_impl.trim_start();
    // Skip the impl's own generics; `->` inside them is no closing angle.
    if rest.starts_with('<') {
        let mut depth = 0i32;
        for (at, ch) in rest.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' if !rest[..at].ends_with('-') => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                rest = &rest[at + 1..];
                break;
            }
        }
    }
    // The type after ` for ` at angle depth 0, if this is a trait impl.
    let mut depth = 0i32;
    for (at, ch) in rest.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if !rest[..at].ends_with('-') => depth -= 1,
            _ => {}
        }
        if depth == 0 && rest[at..].starts_with(" where ") {
            rest = &rest[..at];
            break;
        }
        if depth == 0 && rest[at..].starts_with(" for ") {
            rest = &rest[at + " for ".len()..];
            break;
        }
    }
    let mut ty = rest.trim_start();
    loop {
        let before = ty;
        ty = ty.trim_start_matches(['&', '!']).trim_start();
        if let Some(lifetime) = ty.strip_prefix('\'') {
            ty = lifetime[leading_ident(lifetime).len()..].trim_start();
        }
        for word in ["mut", "dyn"] {
            if leading_ident(ty) == word {
                ty = ty[word.len()..].trim_start();
            }
        }
        if ty == before {
            break;
        }
    }
    let path: String = ty
        .chars()
        .take_while(|&ch| is_ident_char(ch) || ch == ':')
        .collect();
    path.rsplit("::").next().unwrap_or_default().to_owned()
}

/// Every non-test file under `root` in which a top-level item follows the
/// test module (an inline `#[cfg(test)] mod x { … }`, or the declaration
/// `#[cfg(test)] mod tests;`), each as `<path>: …`. Sorted.
fn tests_last_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, items) in order_scanned_files(root) {
        let Some(at) = items.iter().position(TopItem::is_test_module) else {
            continue;
        };
        let after: Vec<String> = items[at + 1..]
            .iter()
            .map(|item| format!("`{}` (line {})", item.label(), item.line))
            .collect();
        if !after.is_empty() {
            problems.push(format!(
                "{path}: {} after the test module `{}` (line {}): {}",
                after.len(),
                items[at].name,
                items[at].line,
                after.join(", ")
            ));
        }
    }
    problems
}

/// Every non-test file under `root` holding more than one inline test module
/// (`#[cfg(test)] mod x { … }` at the top level), or one not named `tests`,
/// each as `<path>: …`. A `#[cfg(test)] mod x;` declaration names a file of
/// its own and is not counted. Sorted.
fn inline_test_module_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, items) in order_scanned_files(root) {
        let inline: Vec<&TopItem> = items
            .iter()
            .filter(|item| item.is_inline_test_module())
            .collect();
        let mut found = Vec::new();
        if inline.len() > 1 {
            let lines: Vec<String> = inline.iter().map(|item| item.line.to_string()).collect();
            found.push(format!(
                "{} inline test modules (lines {})",
                inline.len(),
                lines.join(", ")
            ));
        }
        for item in inline.iter().filter(|item| item.name != "tests") {
            found.push(format!(
                "inline test module `{}` (line {}) is not named `tests`",
                item.name, item.line
            ));
        }
        if !found.is_empty() {
            problems.push(format!("{path}: {}", found.join("; ")));
        }
    }
    problems
}

/// Every non-test folder `mod.rs` under `root` holding an inline test module,
/// each as `<path>: …`. Sorted.
fn mod_rs_inline_test_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, items) in order_scanned_files(root) {
        if !path.ends_with("/mod.rs") {
            continue;
        }
        let inline: Vec<String> = items
            .iter()
            .filter(|item| item.is_inline_test_module())
            .map(|item| format!("`{}` (line {})", item.name, item.line))
            .collect();
        if !inline.is_empty() {
            problems.push(format!(
                "{path}: inline test module {} (move it to `tests.rs`, declared \
                 `#[cfg(test)] mod tests;`)",
                inline.join(", ")
            ));
        }
    }
    problems
}

/// Per non-test file under `root`: each `const` or `static` declared after
/// the file's first type, `impl` or function (`struct`, `enum`, `union`,
/// `trait`, `impl`, `fn`). Items under `#[cfg(test)]` are not read; neither
/// is `const _` (a compile-time assertion about the type above it). A `type`
/// alias is neither code nor a constant here.
fn constants_first_violations(root: &Path) -> Violations {
    let mut violations = Violations::new();
    for (path, items) in order_scanned_files(root) {
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
            continue;
        };
        for item in &items[first + 1..] {
            if matches!(item.kind, ItemKind::Const | ItemKind::Static) && item.name != "_" {
                add(
                    &mut violations,
                    path.clone(),
                    format!(
                        "line {}: `{}` after `{}` (line {})",
                        item.line,
                        item.label(),
                        items[first].label(),
                        items[first].line
                    ),
                );
            }
        }
    }
    violations
}

/// Per non-test file under `root`: each private free function (no `pub` of
/// any kind) declared between the file's first `struct` or `enum` (taken as
/// its primary type) and the last `impl` of that type (`impl Foo`,
/// `impl Trait for Foo`, generics aside). Items under `#[cfg(test)]` are not
/// read. A file whose first type has no `impl` has nothing to check.
fn helpers_after_impls_violations(root: &Path) -> Violations {
    let mut violations = Violations::new();
    for (path, items) in order_scanned_files(root) {
        let items: Vec<&TopItem> = items.iter().filter(|item| !item.cfg_test).collect();
        let Some(primary) = items
            .iter()
            .position(|item| matches!(item.kind, ItemKind::Struct | ItemKind::Enum))
        else {
            continue;
        };
        let name = &items[primary].name;
        let Some(last_impl) = items
            .iter()
            .rposition(|item| item.kind == ItemKind::Impl && &item.name == name)
        else {
            continue;
        };
        for item in items.iter().take(last_impl).skip(primary + 1) {
            if item.kind == ItemKind::Fn && item.private {
                add(
                    &mut violations,
                    path.clone(),
                    format!(
                        "line {}: `{}` between `{}` (line {}) and its last impl (line {})",
                        item.line,
                        item.label(),
                        items[primary].label(),
                        items[primary].line,
                        items[last_impl].line
                    ),
                );
            }
        }
    }
    violations
}

/// Per non-test file under `root`: the file's **families**, when it holds
/// more than one — one detail string, `<n> primary types: `struct Foo` (line
/// N), `enum Bar` (line M)`, each family by its representative, in line
/// order (see [`type_families`]).
///
/// A file's top-level items are joined into families by naming: a type and
/// its impls are one node, and any item (another type, a free `fn`, a
/// `const`, a `static`, an alias, a macro call) that names a type as a whole
/// word joins it — comments and literal contents blanked, `use` and `mod`
/// items and `#[cfg(test)]` ones aside, an enum's variant names not read as
/// types. A family is a group holding a type with a visibility; one is the
/// file's primary type with what serves it.
fn primary_type_violations(root: &Path) -> Violations {
    let mut violations = Violations::new();
    for (path, items) in order_scanned_files(root) {
        let families = type_families(&items);
        if families.len() > 1 {
            let listed: Vec<String> = families
                .iter()
                .map(|ty| format!("`{}` (line {})", ty.label(), ty.line))
                .collect();
            add(
                &mut violations,
                path,
                format!("{} primary types: {}", families.len(), listed.join(", ")),
            );
        }
    }
    violations
}

/// The families of a file's top-level `items`, each by its representative,
/// in line order. Every item but a `use`, a `mod` and a `#[cfg(test)]` one is
/// a node, a type's impls joined to it; an item naming a type joins it. A
/// family is a connected group holding a non-private type; its
/// representative is its first non-private type that no other type of the
/// group names, else its first non-private type.
fn type_families(items: &[TopItem]) -> Vec<&TopItem> {
    let is_type = |item: &TopItem| {
        matches!(
            item.kind,
            ItemKind::Struct | ItemKind::Enum | ItemKind::Union | ItemKind::Trait
        )
    };
    let items: Vec<&TopItem> = items
        .iter()
        .filter(|item| !item.cfg_test && !matches!(item.kind, ItemKind::Use | ItemKind::Mod))
        .collect();
    let enums: Vec<&str> = (items.iter())
        .filter(|item| item.kind == ItemKind::Enum)
        .map(|item| item.name.as_str())
        .collect();
    // Each item's node: a type's own; an impl joins the type it is for, when
    // the file declares it; any other item a node of its own.
    let types: Vec<usize> = (0..items.len()).filter(|&i| is_type(items[i])).collect();
    let node_of = |i: usize| -> usize {
        if items[i].kind == ItemKind::Impl {
            if let Some(&ty) = types.iter().find(|&&ty| items[ty].name == items[i].name) {
                return ty;
            }
        }
        i
    };
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find(parent: &mut [usize], mut at: usize) -> usize {
        while parent[at] != at {
            parent[at] = parent[parent[at]];
            at = parent[at];
        }
        at
    }
    // `named[ty]`: whether another type of the file (its declaration or its
    // impls) names `ty`.
    let mut named = vec![false; items.len()];
    for (i, item) in items.iter().enumerate() {
        let node = node_of(i);
        let code = if item.kind == ItemKind::Enum {
            blank_variant_names(&item.code)
        } else {
            item.code.clone()
        };
        for &ty in &types {
            if ty == node || !names_type(&code, &items[ty].name, &enums) {
                continue;
            }
            let (a, b) = (find(&mut parent, node), find(&mut parent, ty));
            parent[a] = b;
            if is_type(items[node]) {
                named[ty] = true;
            }
        }
    }
    let mut families: Vec<&TopItem> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for &ty in &types {
        let group = find(&mut parent, ty);
        if seen.contains(&group) {
            continue;
        }
        let members: Vec<usize> = (types.iter().copied())
            .filter(|&other| !items[other].private && find(&mut parent, other) == group)
            .collect();
        let Some(&first) = members.first() else {
            continue;
        };
        seen.push(group);
        let representative = (members.iter().copied())
            .find(|&other| !named[other])
            .unwrap_or(first);
        families.push(items[representative]);
    }
    families.sort_by_key(|item| item.line);
    families
}

/// Whether `code` names the type `word`: as a whole identifier, not as a
/// variant of an enum of `enums` (`Kind::Word`).
fn names_type(code: &str, word: &str, enums: &[&str]) -> bool {
    code.match_indices(word).any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + word.len()..].chars().next();
        let whole = !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char);
        let variant = (code[..at].strip_suffix("::"))
            .is_some_and(|path| enums.iter().any(|name| names_suffix(path, name)));
        whole && !variant
    })
}

/// Whether `path` ends with the whole identifier `name`.
fn names_suffix(path: &str, name: &str) -> bool {
    path.strip_suffix(name)
        .is_some_and(|rest| !rest.chars().next_back().is_some_and(is_ident_char))
}

/// An enum's `code` with the name of each of its variants blanked, where it
/// is declared: what a variant is called names no type.
fn blank_variant_names(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let Some(open) = chars.iter().position(|&ch| ch == '{') else {
        return code.to_owned();
    };
    let mut out = chars.clone();
    let mut depth = 0usize;
    let mut expect_name = true;
    let mut i = open + 1;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '#' if depth == 0 && chars.get(i + 1) == Some(&'[') => {
                i = closing_bracket(&chars, i + 1);
                continue;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '}' if depth == 0 => break,
            '}' => depth -= 1,
            ',' if depth == 0 => expect_name = true,
            _ if expect_name && depth == 0 && is_ident_char(ch) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                out[start..i].iter_mut().for_each(|blank| *blank = ' ');
                expect_name = false;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Tests come last in a file: after the test module — an inline
/// `#[cfg(test)] mod tests { … }`, or `#[cfg(test)] mod tests;` declaring the
/// file that holds them — nothing but blank lines and comments. Strict.
#[test]
fn tests_come_last_in_a_file() {
    let problems = tests_last_problems(&repo_root());
    assert!(
        problems.is_empty(),
        "{} files hold code after their test module (move the code above it):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// A file holds at most one inline test module, named `tests`. A
/// `#[cfg(test)] mod <name>;` declaration is not an inline module: it points
/// at a file of its own, which `test_files_follow_convention` governs.
/// Strict.
#[test]
fn a_file_holds_at_most_one_inline_test_module_named_tests() {
    let problems = inline_test_module_problems(&repo_root());
    assert!(
        problems.is_empty(),
        "{} files hold several inline test modules, or one not named `tests`:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// A folder `mod.rs` holds no inline test module: its tests go in the
/// folder's `tests.rs` (or `tests/`), declared `#[cfg(test)] mod tests;`.
/// Strict.
#[test]
fn a_folder_mod_rs_holds_no_inline_test_module() {
    let problems = mod_rs_inline_test_problems(&repo_root());
    assert!(
        problems.is_empty(),
        "{} folder `mod.rs` files hold an inline test module:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// Constants come before the code: no top-level `const` or `static` after
/// the file's first `struct`, `enum`, `union`, `trait`, `impl` or `fn`
/// (`const _` and items under `#[cfg(test)]` aside). Keyed by the file.
#[test]
fn constants_come_before_the_code() {
    ratchet(
        "constants_come_before_the_code",
        "constants declared after the code (move them above the first type, impl or function)",
        &constants_first_violations(&repo_root()),
    );
}

/// Private helpers come after the primary type's impls: no private free
/// function between the file's first `struct` or `enum` and the last `impl`
/// of that type. Keyed by the file.
#[test]
fn private_helpers_come_after_the_primary_type_impls() {
    ratchet(
        "private_helpers_come_after_the_primary_type_impls",
        "private helpers between the primary type and its impls (move them below the last impl)",
        &helpers_after_impls_violations(&repo_root()),
    );
}

/// **A file holds one primary type.** Of a file's public types (`struct`,
/// `enum`, `union`, `trait`, any `pub`), at most one is a root: named by no
/// other item of the file than its own declaration and impls. The others are
/// its field and payload types, the types its methods return, the traits it
/// implements. Keyed by the file.
#[test]
fn each_file_holds_one_primary_type() {
    ratchet(
        "each_file_holds_one_primary_type",
        "files holding more than one primary type (split them, one file per type)",
        &primary_type_violations(&repo_root()),
    );
}

/// Writes `files` (paths relative to `base`) under `base`.
fn write_order_tree(base: &Path, files: &[(&str, &str)]) -> std::io::Result<()> {
    for (file, text) in files {
        let path = base.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}

/// A fresh, empty directory under the system temp folder for one self-test.
fn order_temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-order-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// A violation map as `(path, details)` pairs, for comparing in a test.
fn violation_list(violations: Violations) -> Vec<(String, Vec<String>)> {
    violations
        .into_iter()
        .map(|(path, details)| (path, details.into_iter().collect()))
        .collect()
}

/// The item reader keeps a file's top-level items whole: a body with
/// braces, a `const` whose value is a struct literal, a multi-line attribute,
/// a raw string holding a column-0 `fn` and a brace, a char literal brace, a
/// `where` clause, a macro call, an `extern` block; it names an `impl`'s type
/// and tells an inline module from a declared one.
#[test]
fn top_level_items_reads_whole_items() {
    const TEXT: &str = "\
//! Doc `fn hidden() {}`.
#![allow(dead_code)]
use std::fmt;

pub(crate) const A: Foo = Foo { a: '{' };
static mut B: u8 = 0;
const _: () = ();
#[derive(
    Debug,
)]
pub struct Foo {
    a: char,
}
const RAW: &str = r#\"
fn not_an_item() {
\"#;
impl<T: Fn() -> u8> fmt::Display for &'static Foo<T>
where
    T: Copy,
{
    fn fmt(&self) {}
}
inventory::submit! { Foo { a: '}' } }
pub const unsafe fn helper() {}
extern \"C\" {
    fn ext();
}
#[cfg(test)]
mod fixtures;
#[cfg(all(test, unix))]
pub mod tests {
    fn inner() {}
}
";
    let seen: Vec<(usize, ItemKind, String, bool, bool, bool)> = top_level_items(TEXT)
        .into_iter()
        .map(|item| {
            (
                item.line,
                item.kind,
                item.name,
                item.private,
                item.cfg_test,
                item.inline,
            )
        })
        .collect();
    let expected: Vec<(usize, ItemKind, String, bool, bool, bool)> = [
        (3, ItemKind::Use, "", true, false, false),
        (5, ItemKind::Const, "A", false, false, false),
        (6, ItemKind::Static, "B", true, false, false),
        (7, ItemKind::Const, "_", true, false, false),
        (11, ItemKind::Struct, "Foo", false, false, false),
        (14, ItemKind::Const, "RAW", true, false, false),
        (17, ItemKind::Impl, "Foo", true, false, false),
        (23, ItemKind::Macro, "", true, false, false),
        (24, ItemKind::Fn, "helper", false, false, false),
        (25, ItemKind::Other, "", true, false, false),
        (29, ItemKind::Mod, "fixtures", true, true, false),
        (31, ItemKind::Mod, "tests", false, true, true),
    ]
    .into_iter()
    .map(|(line, kind, name, private, cfg_test, inline)| {
        (line, kind, name.to_owned(), private, cfg_test, inline)
    })
    .collect();
    assert_eq!(seen, expected);

    assert_eq!(impl_self_type("Foo"), "Foo");
    assert_eq!(impl_self_type("<T> a::b::Bar<T> where T: X"), "Bar");
    assert_eq!(
        impl_self_type("<F> X for F where F: for<'a> Fn(&'a u8)"),
        "F"
    );
    assert_eq!(impl_self_type("<'a> From<&'a str> for &'a mut Baz"), "Baz");
}

/// The tests-last check names a file with code after its test module,
/// inline or declared `mod tests;`, and passes a file whose test module is
/// last (a comment may follow), a file declaring a test file of another name
/// (`#[cfg(test)] mod fixtures;`) above its code, and a test file.
#[test]
fn tests_last_check_names_code_after_the_test_module() {
    let dir = order_temp_dir("tests-last");
    let files: &[(&str, &str)] = &[
        (
            "crates/alpha/src/lib.rs",
            "mod good;\nmod late;\nmod declared;\nmod fixture_decl;\n#[cfg(test)]\nmod tests;\n",
        ),
        ("crates/alpha/src/tests.rs", "#[test]\nfn t() {}\n"),
        (
            "crates/alpha/src/good.rs",
            "pub struct Good;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n// trailing comment\n",
        ),
        (
            "crates/alpha/src/late.rs",
            "pub struct Late;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nimpl Late {}\n\
             pub fn after() {}\n",
        ),
        (
            "crates/alpha/src/declared.rs",
            "#[cfg(test)]\nmod tests;\npub struct Declared;\n",
        ),
        ("crates/alpha/src/declared/tests.rs", "fn t() {}\n"),
        (
            "crates/alpha/src/fixture_decl.rs",
            "#[cfg(test)]\nmod fixtures;\npub struct Fixtured;\n",
        ),
        (
            "crates/alpha/src/fixture_decl/fixtures.rs",
            "pub fn fixture() {}\nimpl X {}\n#[cfg(test)]\nmod tests {\n}\nfn after() {}\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let problems = tests_last_problems(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "crates/alpha/src/declared.rs: 1 after the test module `tests` (line 2): \
             `struct Declared` (line 3)"
                .to_owned(),
            "crates/alpha/src/late.rs: 2 after the test module `tests` (line 3): \
             `impl Late` (line 6), `fn after` (line 7)"
                .to_owned(),
        ]
    );
}

/// The inline-test-module check names a file with two inline test modules
/// and a file whose one inline test module is not named `tests`; it passes a
/// file with `mod tests { … }` beside `#[cfg(test)] mod fixtures;`, a
/// non-test inline module, and an inline module inside a test file.
#[test]
fn inline_test_module_check_names_extra_and_misnamed_modules() {
    let dir = order_temp_dir("inline-tests");
    let files: &[(&str, &str)] = &[
        (
            "crates/alpha/src/lib.rs",
            "mod one;\nmod two;\nmod misnamed;\n#[cfg(test)]\nmod tests;\n",
        ),
        (
            "crates/alpha/src/tests.rs",
            "#[cfg(test)]\nmod nested_in_a_test_file {}\n#[cfg(test)]\nmod again {}\n",
        ),
        (
            "crates/alpha/src/one.rs",
            "pub mod inner {\n    pub fn f() {}\n}\n#[cfg(test)]\nmod fixtures;\n#[cfg(test)]\n\
             mod tests {\n}\n",
        ),
        (
            "crates/alpha/src/one/fixtures.rs",
            "#[cfg(test)]\nmod a {}\n#[cfg(test)]\nmod b {}\n",
        ),
        (
            "crates/alpha/src/two.rs",
            "pub struct Two;\n#[cfg(test)]\nmod tests {\n}\n#[cfg(test)]\nmod more_tests {\n}\n",
        ),
        (
            "crates/alpha/src/misnamed.rs",
            "pub struct Misnamed;\n#[cfg(test)]\nmod misnamed_tests {\n    #[test]\n    fn t() {}\n}\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let problems = inline_test_module_problems(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "crates/alpha/src/misnamed.rs: inline test module `misnamed_tests` (line 3) is not \
             named `tests`"
                .to_owned(),
            "crates/alpha/src/two.rs: 2 inline test modules (lines 3, 6); inline test module \
             `more_tests` (line 6) is not named `tests`"
                .to_owned(),
        ]
    );
}

/// The folder-`mod.rs` check names a `mod.rs` holding `mod tests { … }`; it
/// passes a `mod.rs` declaring `#[cfg(test)] mod tests;`, a file module with
/// inline tests, and a `mod.rs` under a `tests/` folder.
#[test]
fn mod_rs_check_names_an_inline_test_module_in_a_folder_module() {
    let dir = order_temp_dir("mod-rs-tests");
    let files: &[(&str, &str)] = &[
        (
            "crates/alpha/src/lib.rs",
            "mod inline;\nmod declared;\nmod file;\n#[cfg(test)]\nmod tests;\n",
        ),
        (
            "crates/alpha/src/tests/mod.rs",
            "#[cfg(test)]\nmod tests {\n}\n",
        ),
        (
            "crates/alpha/src/inline/mod.rs",
            "pub struct Inline;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
        ),
        (
            "crates/alpha/src/declared/mod.rs",
            "pub struct Declared;\n#[cfg(test)]\nmod tests;\n",
        ),
        ("crates/alpha/src/declared/tests.rs", "fn t() {}\n"),
        (
            "crates/alpha/src/file.rs",
            "pub struct File;\n#[cfg(test)]\nmod tests {\n}\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let problems = mod_rs_inline_test_problems(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "crates/alpha/src/inline/mod.rs: inline test module `tests` (line 3) (move it to \
             `tests.rs`, declared `#[cfg(test)] mod tests;`)"
                .to_owned()
        ]
    );
}

/// A test module is one whatever the place of `test` in its `cfg(all(…))`:
/// `#[cfg(all(not(miri), test))]` compiles only under test as
/// `#[cfg(all(test, not(miri)))]` does, so code after it is named, and a
/// folder `mod.rs` holding it inline is named.
#[test]
fn a_test_module_is_read_whatever_the_place_of_test_in_its_cfg_all() {
    let dir = order_temp_dir("cfg-all-order");
    let files: &[(&str, &str)] = &[
        ("crates/alpha/src/lib.rs", "mod late;\nmod folder;\n"),
        (
            "crates/alpha/src/late.rs",
            "pub struct Late;\n#[cfg(all(not(miri), test))]\nmod tests {\n    fn t() {}\n}\n\
             pub fn after() {}\n",
        ),
        (
            "crates/alpha/src/folder/mod.rs",
            "pub struct Folder;\n#[cfg(all(unix, test))]\nmod tests {\n    fn t() {}\n}\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let last = tests_last_problems(&dir);
    let mod_rs = mod_rs_inline_test_problems(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        last,
        vec![
            "crates/alpha/src/late.rs: 1 after the test module `tests` (line 3): \
             `fn after` (line 6)"
                .to_owned()
        ]
    );
    assert_eq!(mod_rs.len(), 1, "{mod_rs:#?}");
}

/// The constants check names a `const` and a `static` declared after the
/// first type, `impl` or function; it passes constants above the code,
/// `const _`, a `const` under `#[cfg(test)]`, a `const fn`, a `type` alias,
/// a constant inside an `impl` or a test module, and a file of constants and
/// macros only.
#[test]
fn constants_check_names_a_constant_after_the_code() {
    let dir = order_temp_dir("constants");
    const GOOD: &str = "\
use std::fmt;
const A: u8 = 1;
pub static B: u8 = 2;
type Alias = u8;
pub struct Good;
impl Good {
    const C: u8 = 3;
}
const _: () = ();
pub const fn make() -> u8 {
    1
}
type Late = u8;
#[cfg(test)]
const T: u8 = 4;
#[cfg(test)]
mod tests {
    const D: u8 = 5;
}
";
    let files: &[(&str, &str)] = &[
        (
            "crates/alpha/src/lib.rs",
            "mod good;\nmod bad;\nmod helper_first;\nmod only_consts;\n",
        ),
        ("crates/alpha/src/good.rs", GOOD),
        (
            "crates/alpha/src/bad.rs",
            "pub struct Bad;\nimpl Bad {}\npub const LATE: u8 = 1;\nstatic mut COUNT: u8 = 0;\n",
        ),
        (
            "crates/alpha/src/helper_first.rs",
            "fn helper() {}\nconst AFTER: u8 = 1;\n",
        ),
        (
            "crates/alpha/src/only_consts.rs",
            "inventory::submit! { X }\nconst A: u8 = 1;\nmacro_rules! m {\n    () => {};\n}\n\
             static B: u8 = 2;\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let violations = constants_first_violations(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        violation_list(violations),
        vec![
            (
                "crates/alpha/src/bad.rs".to_owned(),
                vec![
                    "line 3: `const LATE` after `struct Bad` (line 1)".to_owned(),
                    "line 4: `static COUNT` after `struct Bad` (line 1)".to_owned(),
                ]
            ),
            (
                "crates/alpha/src/helper_first.rs".to_owned(),
                vec!["line 2: `const AFTER` after `fn helper` (line 1)".to_owned()]
            ),
        ]
    );
}

/// The helpers check names a private free function (a `const fn` too)
/// between the primary type and its last `impl`, inherent or trait, generic
/// or not; it passes a helper below the last impl, a `pub` or `pub(crate)`
/// function between, a helper between a second type's impls, a
/// `#[cfg(test)]` helper, and a file whose primary type has no impl.
#[test]
fn helpers_check_names_a_helper_between_the_type_and_its_impls() {
    let dir = order_temp_dir("helpers");
    const GOOD: &str = "\
pub struct Good<T>(T);
impl<T> Good<T> {}
pub fn api() {}
pub(crate) fn shared() {}
#[cfg(test)]
fn test_helper() {}
impl<T: Clone> Clone for Good<T> {
    fn clone(&self) -> Self {
        todo!()
    }
}
struct Other;
impl Other {}
fn between_others() {}
impl Default for Other {
    fn default() -> Self {
        Other
    }
}
fn helper() {}
";
    const BAD: &str = "\
pub enum Bad {
    A,
}
fn early() {}
impl Bad {}
const fn also() {}
impl std::fmt::Debug for Bad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}
fn after() {}
";
    let files: &[(&str, &str)] = &[
        (
            "crates/alpha/src/lib.rs",
            "mod good;\nmod bad;\nmod no_impl;\n",
        ),
        ("crates/alpha/src/good.rs", GOOD),
        ("crates/alpha/src/bad.rs", BAD),
        (
            "crates/alpha/src/no_impl.rs",
            "pub struct NoImpl;\nfn helper() {}\nimpl Other {}\n",
        ),
    ];
    let setup = write_order_tree(&dir, files);
    let violations = helpers_after_impls_violations(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        violation_list(violations),
        vec![(
            "crates/alpha/src/bad.rs".to_owned(),
            vec![
                "line 4: `fn early` between `enum Bad` (line 1) and its last impl (line 7)"
                    .to_owned(),
                "line 6: `fn also` between `enum Bad` (line 1) and its last impl (line 7)"
                    .to_owned(),
            ]
        )]
    );
}

/// The primary-type check's violations on a fresh tree of `files` (paths
/// relative to it), as `(path, details)` pairs.
fn primary_type_list(label: &str, files: &[(&str, &str)]) -> Vec<(String, Vec<String>)> {
    let dir = order_temp_dir(label);
    let setup = write_order_tree(&dir, files);
    let violations = primary_type_violations(&dir);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");
    violation_list(violations)
}

/// One expected primary-type violation: `path` holding `roots` (each
/// `` `struct Foo` (line N) ``, in line order).
fn primary_type_violation(path: &str, roots: &[&str]) -> (String, Vec<String>) {
    (
        path.to_owned(),
        vec![format!(
            "{} primary types: {}",
            roots.len(),
            roots.join(", ")
        )],
    )
}

/// **A type that names its field and payload types is the file's one
/// root.** `Offer` holds `Vec<OfferRow>`, `OfferRow` holds a `Basis`, a
/// variant of `Basis` carries a `Unit`: one root, `Offer`, so no violation.
/// A file of two unrelated types beside it is named, so the check did run.
#[test]
fn primary_type_check_passes_a_type_naming_its_field_and_payload_types() {
    const OFFER: &str = "\
pub struct Offer {
    pub rows: Vec<OfferRow>,
}
pub struct OfferRow {
    pub basis: Basis,
}
pub enum Basis {
    Time(Unit),
    Memory,
}
pub struct Unit;
";
    let list = primary_type_list(
        "primary-fields",
        &[
            ("crates/alpha/src/lib.rs", "mod offer;\nmod control;\n"),
            ("crates/alpha/src/offer.rs", OFFER),
            (
                "crates/alpha/src/control.rs",
                "pub struct One;\npub struct Two;\n",
            ),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/control.rs",
            &["`struct One` (line 1)", "`struct Two` (line 2)"]
        )]
    );
}

/// **Two unrelated public types are two roots, named with their lines.**
/// `BufferId` and `BufferDescriptor` name nothing of each other: the file is
/// named, each root at the line of its keyword (below its doc and
/// attributes). An `enum` and a `pub(crate) union` count as types too.
#[test]
fn primary_type_check_names_two_unrelated_types_with_their_lines() {
    const BUFFER: &str = "\
//! Buffers.

/// A buffer's id.
#[derive(Debug)]
pub struct BufferId(u64);
pub struct BufferDescriptor {
    pub size: u64,
}
";
    const MODES: &str = "\
pub enum Mode {
    Fast,
}
pub(crate) union Bits {
    a: u32,
}
";
    let list = primary_type_list(
        "primary-unrelated",
        &[
            ("crates/alpha/src/lib.rs", "mod buffer;\nmod modes;\n"),
            ("crates/alpha/src/buffer.rs", BUFFER),
            ("crates/alpha/src/modes.rs", MODES),
        ],
    );
    assert_eq!(
        list,
        vec![
            primary_type_violation(
                "crates/alpha/src/buffer.rs",
                &[
                    "`struct BufferId` (line 5)",
                    "`struct BufferDescriptor` (line 6)"
                ]
            ),
            primary_type_violation(
                "crates/alpha/src/modes.rs",
                &["`enum Mode` (line 1)", "`union Bits` (line 4)"]
            ),
        ]
    );
}

/// **A type named in another type's impl is secondary; a type named only in
/// its own impls is not.** `ReadinessHold` appears only as the return type of
/// a method in `impl Readiness`: one root. `Left` and `Right<T>` each appear
/// only in their own impls (`impl Left`, `impl<T: Clone> Clone for
/// Right<T>`): two roots.
#[test]
fn primary_type_check_counts_another_types_impl_but_not_a_types_own() {
    const READINESS: &str = "\
pub struct Readiness;
impl Readiness {
    pub fn hold(&self) -> ReadinessHold {
        ReadinessHold
    }
}
pub struct ReadinessHold;
";
    const OWN_IMPLS: &str = "\
pub struct Left;
impl Left {
    pub fn new() -> Left {
        Left
    }
}
pub struct Right<T>(T);
impl<T: Clone> Clone for Right<T> {
    fn clone(&self) -> Right<T> {
        Right(self.0.clone())
    }
}
";
    let list = primary_type_list(
        "primary-impls",
        &[
            (
                "crates/alpha/src/lib.rs",
                "mod readiness;\nmod own_impls;\n",
            ),
            ("crates/alpha/src/readiness.rs", READINESS),
            ("crates/alpha/src/own_impls.rs", OWN_IMPLS),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/own_impls.rs",
            &["`struct Left` (line 1)", "`struct Right` (line 7)"]
        )]
    );
}

/// **A trait another type of the file implements is secondary.**
/// `impl Validate for Foo` names `Validate`: `Foo` is the one root. Beside an
/// unrelated `Other`, the roots are `Foo` and `Other`, never the trait; a
/// trait nothing implements is a root of its own.
#[test]
fn primary_type_check_counts_a_trait_implemented_by_another_type_as_secondary() {
    const VALIDATE: &str = "\
pub trait Validate {
    fn validate(&self) -> bool;
}
pub struct Foo;
impl Validate for Foo {
    fn validate(&self) -> bool {
        true
    }
}
";
    let list = primary_type_list(
        "primary-trait",
        &[
            (
                "crates/alpha/src/lib.rs",
                "mod validate;\nmod with_stray;\nmod trait_root;\n",
            ),
            ("crates/alpha/src/validate.rs", VALIDATE),
            (
                "crates/alpha/src/with_stray.rs",
                "pub trait Check {}\npub struct Foo;\nimpl Check for Foo {}\npub struct Other;\n",
            ),
            (
                "crates/alpha/src/trait_root.rs",
                "pub trait Draw {}\npub struct Canvas;\n",
            ),
        ],
    );
    assert_eq!(
        list,
        vec![
            primary_type_violation(
                "crates/alpha/src/trait_root.rs",
                &["`trait Draw` (line 1)", "`struct Canvas` (line 2)"]
            ),
            primary_type_violation(
                "crates/alpha/src/with_stray.rs",
                &["`struct Foo` (line 2)", "`struct Other` (line 4)"]
            ),
        ]
    );
}

/// **Only a type, or a free item naming several types, joins types into one
/// family.** `Main` names `ByField` by a field and `ByBound` by a bound in its
/// impl: one family, `Main`. A free function, a macro call, a `const`, a
/// `static` or a type alias naming a single type does not join it to
/// another: `ByFn`, `ByMacro`, `ByConst`, `ByStatic`, `ByAlias` are a family
/// each, as is the unrelated `Stray`. The free function `both` names
/// `JoinedA` and `JoinedB`: one family, represented by `JoinedA`, the first by
/// line. Eight families, each named by its representative.
#[test]
fn primary_type_check_counts_every_kind_of_item_naming_a_type() {
    const KINDS: &str = "\
pub struct Main {
    pub field: ByField,
}
pub struct ByField;
pub struct ByFn;
pub struct ByMacro;
pub struct ByConst;
pub struct ByStatic;
pub struct ByAlias;
pub trait ByBound {}
pub struct Stray;
pub struct JoinedA;
pub struct JoinedB;
impl<T: ByBound> From<T> for Main {
    fn from(_: T) -> Self {
        Main { field: ByField }
    }
}
pub fn by_fn(_: ByFn) {}
inventory::submit! { ByMacro }
const DEFAULT: ByConst = ByConst;
static GLOBAL: Option<ByStatic> = None;
type Short = ByAlias;
pub fn both(_: JoinedA, _: JoinedB) {}
";
    let list = primary_type_list(
        "primary-kinds",
        &[
            ("crates/alpha/src/lib.rs", "mod kinds;\n"),
            ("crates/alpha/src/kinds.rs", KINDS),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/kinds.rs",
            &[
                "`struct Main` (line 1)",
                "`struct ByFn` (line 5)",
                "`struct ByMacro` (line 6)",
                "`struct ByConst` (line 7)",
                "`struct ByStatic` (line 8)",
                "`struct ByAlias` (line 9)",
                "`struct Stray` (line 11)",
                "`struct JoinedA` (line 12)",
            ]
        )]
    );
}

/// **A name in a comment or a literal names nothing.** `B` appears in a line
/// comment, in a doc comment on `A`, in a string, a raw string and a char
/// literal held by `const` items: `A` and `B` are both roots.
#[test]
fn primary_type_check_ignores_names_in_comments_and_literals() {
    const LITERALS: &str = "\
// B is mentioned here only.
const S: &str = \"B\";
const R: &str = r#\"B\"#;
const C: char = 'B';
/// See [`B`].
pub struct A;
pub struct B;
";
    let list = primary_type_list(
        "primary-literals",
        &[
            ("crates/alpha/src/lib.rs", "mod literals;\n"),
            ("crates/alpha/src/literals.rs", LITERALS),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/literals.rs",
            &["`struct A` (line 6)", "`struct B` (line 7)"]
        )]
    );
}

/// **A name counts as a whole word only.** `GpuMesh`, `MeshSource` and
/// `Mesh_` do not name `Mesh`; `GpuMeshPool` does not name `GpuMesh`: both
/// are roots.
#[test]
fn primary_type_check_matches_whole_words_only() {
    const MESH: &str = "\
pub struct Mesh;
pub struct GpuMesh {
    pub source: MeshSource,
}
pub fn upload(_: &GpuMeshPool, _: Mesh_) {}
";
    let list = primary_type_list(
        "primary-words",
        &[
            ("crates/alpha/src/lib.rs", "mod mesh;\n"),
            ("crates/alpha/src/mesh.rs", MESH),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/mesh.rs",
            &["`struct Mesh` (line 1)", "`struct GpuMesh` (line 2)"]
        )]
    );
}

/// **Private types and test items are not types of the file.** A private
/// `Helper` named nowhere beside `pub struct Main`: no violation. A
/// `#[cfg(test)] pub struct Fixture` and a `pub struct T` in the inline
/// `#[cfg(test)] mod tests` beside `Main`: no violation either. A file of
/// two unrelated types beside them is named, so the check did run.
#[test]
fn primary_type_check_ignores_private_types_and_test_items() {
    const CFG_TEST: &str = "\
pub struct Main;
#[cfg(test)]
pub struct Fixture;
#[cfg(test)]
mod tests {
    use super::*;
    pub struct T;
}
";
    let list = primary_type_list(
        "primary-private",
        &[
            (
                "crates/alpha/src/lib.rs",
                "mod private;\nmod cfg_test;\nmod control;\n",
            ),
            (
                "crates/alpha/src/private.rs",
                "struct Helper;\npub struct Main;\n",
            ),
            ("crates/alpha/src/cfg_test.rs", CFG_TEST),
            (
                "crates/alpha/src/control.rs",
                "pub struct One;\npub struct Two;\n",
            ),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/control.rs",
            &["`struct One` (line 1)", "`struct Two` (line 2)"]
        )]
    );
}

/// **A `use` or a `mod` naming a type does not make it secondary.** `pub use
/// crate::prelude::B;` names `B`, an inline `mod extra` imports and uses `A`:
/// `A` and `B` are still both roots.
#[test]
fn primary_type_check_ignores_use_and_mod_items() {
    const USES: &str = "\
pub use crate::prelude::B;
pub struct A;
pub struct B;
mod extra {
    use super::A;
    pub fn f(_: A) {}
}
";
    let list = primary_type_list(
        "primary-uses",
        &[
            ("crates/alpha/src/lib.rs", "mod uses;\n"),
            ("crates/alpha/src/uses.rs", USES),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/uses.rs",
            &["`struct A` (line 2)", "`struct B` (line 3)"]
        )]
    );
}

/// **Test files are not read.** A `tests.rs`, a file under a `tests/` folder,
/// a `#![cfg(test)]` file and a file declared by `#[cfg(test)] mod` each hold
/// two unrelated types: only the non-test `real.rs` is named.
#[test]
fn primary_type_check_skips_test_files() {
    const TWO: &str = "pub struct One;\npub struct Two;\n";
    let list = primary_type_list(
        "primary-test-files",
        &[
            (
                "crates/alpha/src/lib.rs",
                "mod real;\nmod gated;\n#[cfg(test)]\nmod support;\n#[cfg(test)]\nmod tests;\n",
            ),
            ("crates/alpha/src/real.rs", TWO),
            ("crates/alpha/src/tests.rs", TWO),
            ("crates/alpha/src/tests/fixtures.rs", TWO),
            ("crates/alpha/src/support.rs", TWO),
            (
                "crates/alpha/src/gated.rs",
                "#![cfg(test)]\npub struct One;\npub struct Two;\n",
            ),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/real.rs",
            &["`struct One` (line 1)", "`struct Two` (line 2)"]
        )]
    );
}

/// **Unrelated types named only by items that are not types are still more
/// than one primary type.** `SimSteps` and `SubSteps` are each returned by a
/// free function of their own; `ScenePassSlot` and `UiPassSlot` are each
/// submitted by a macro call, and `FrameGraph` is named by a type alias. No
/// type of either file names another, yet each file is left with no root at
/// all: both files are named.
#[test]
fn primary_type_check_names_unrelated_types_named_only_by_free_items() {
    const SIM_STEPS: &str = "\
pub struct SimSteps {
    pub count: u32,
}
pub struct SubSteps {
    pub count: u32,
}
pub fn compute_sim_steps(dt: f32) -> SimSteps {
    SimSteps { count: dt as u32 }
}
pub fn compute_sub_steps(dt: f32) -> SubSteps {
    SubSteps { count: dt as u32 }
}
";
    const FRAME_GRAPH: &str = "\
pub struct FrameGraph;
pub struct ScenePassSlot;
pub struct UiPassSlot;
pub type SharedFrameGraph = std::sync::Arc<FrameGraph>;
inventory::submit! { ScenePassSlot }
inventory::submit! { UiPassSlot }
";
    let list = primary_type_list(
        "primary-free-items",
        &[
            (
                "crates/alpha/src/lib.rs",
                "mod sim_steps;\nmod frame_graph;\n",
            ),
            ("crates/alpha/src/sim_steps.rs", SIM_STEPS),
            ("crates/alpha/src/frame_graph.rs", FRAME_GRAPH),
        ],
    );
    let named: Vec<&str> = list.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        named,
        vec![
            "crates/alpha/src/frame_graph.rs",
            "crates/alpha/src/sim_steps.rs"
        ]
    );
}

/// **An enum variant named like a type does not name the type.** `SourceKind`
/// has a variant `Locked`; the struct `Locked` beside it is named by nothing:
/// `SourceKind` and `Locked` are both roots.
#[test]
fn primary_type_check_does_not_read_a_variant_as_the_type_of_its_name() {
    const SOURCE: &str = "\
pub enum SourceKind {
    Locked,
    Free,
}
pub struct Locked {
    pub depth: u32,
}
";
    let list = primary_type_list(
        "primary-variant",
        &[
            ("crates/alpha/src/lib.rs", "mod source;\n"),
            ("crates/alpha/src/source.rs", SOURCE),
        ],
    );
    assert_eq!(
        list,
        vec![primary_type_violation(
            "crates/alpha/src/source.rs",
            &["`enum SourceKind` (line 1)", "`struct Locked` (line 5)"]
        )]
    );
}

// ---------------------------------------------------------------------------
// Function length
// ---------------------------------------------------------------------------
//
// A function reads in one sitting. The check below reads every function with
// a body — free functions, methods in `impl` and `trait` blocks, functions
// nested in others — in the non-test files of every Rust `src/` tree, and
// names those longer than `MAX_FN_LINES`.

/// Largest number of lines a non-test function may span, from its `fn`
/// keyword to the brace closing its body.
const MAX_FN_LINES: usize = 200;

/// A function with a body, as the length check reads it.
#[derive(Debug, PartialEq, Eq)]
struct FnSpan {
    /// 1-based line of the `fn` keyword.
    line: usize,
    name: String,
    /// Lines from the `fn` keyword to the brace closing the body, both
    /// counted.
    lines: usize,
}

/// Every function with a body in `text`, in order, at any depth: free
/// functions, methods of `impl` and `trait` blocks, functions nested in
/// others. Comments and literal contents are blanked first. Not read: a
/// function without a body (`fn f();` in a trait), a function type
/// (`fn(u8) -> u8`), a name a macro builds (`fn $name`, `fn #name`), and
/// anything inside an item under `#[cfg(test)]` (an inline test module, a
/// test helper, a test-only `impl`).
fn function_spans(text: &str) -> Vec<FnSpan> {
    let code = blank_literals(&strip_rust_comments(text));
    let chars: Vec<char> = code.chars().collect();
    let mut line_at = Vec::with_capacity(chars.len());
    let mut line = 1usize;
    for &ch in &chars {
        line_at.push(line);
        if ch == '\n' {
            line += 1;
        }
    }
    let tests = cfg_test_item_ranges(&chars);
    let in_tests = |at: usize| tests.iter().any(|range| range.contains(&at));
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i + 2 < chars.len() {
        let keyword = chars[i] == 'f'
            && chars[i + 1] == 'n'
            && chars[i + 2].is_whitespace()
            && (i == 0 || !is_ident_char(chars[i - 1]));
        if !keyword || in_tests(i) {
            i += 1;
            continue;
        }
        let mut at = i + 2;
        while chars.get(at).is_some_and(|ch| ch.is_whitespace()) {
            at += 1;
        }
        let name: String = chars[at..]
            .iter()
            .take_while(|&&ch| is_ident_char(ch))
            .collect();
        if !name.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') {
            i += 2;
            continue;
        }
        at += name.chars().count();
        // The body opens at the first `{` outside parentheses and brackets;
        // a `;` there first means no body.
        let mut depth = 0usize;
        let mut open = None;
        for (j, &ch) in chars.iter().enumerate().skip(at) {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '{' if depth == 0 => {
                    open = Some(j);
                    break;
                }
                ';' if depth == 0 => break,
                _ => {}
            }
        }
        if let Some(open) = open {
            let mut braces = 0usize;
            let mut close = chars.len() - 1;
            for (j, &ch) in chars.iter().enumerate().skip(open) {
                match ch {
                    '{' => braces += 1,
                    '}' => {
                        braces -= 1;
                        if braces == 0 {
                            close = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            spans.push(FnSpan {
                line: line_at[i],
                name,
                lines: line_at[close] - line_at[i] + 1,
            });
        }
        i = at;
    }
    spans
}

/// The character ranges of `chars` (comments and literals blanked) held by
/// an item under `#[cfg(test)]` or `#[cfg(all(test, …))]`, at any depth: from
/// the attribute to the `;` ending the item or the brace closing its body.
fn cfg_test_item_ranges(chars: &[char]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '#' || chars.get(i + 1) != Some(&'[') {
            i += 1;
            continue;
        }
        let attr_end = closing_bracket(chars, i + 1);
        let attr: String = chars[i..attr_end].iter().collect();
        if !is_cfg_test(&attr) {
            i = attr_end;
            continue;
        }
        let mut depth = 0usize;
        let mut end = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(attr_end) {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && ch == '}' {
                        end = j + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
        }
        ranges.push(i..end);
        i = end;
    }
    ranges
}

/// Per non-test file under `root`: each function with a body spanning more
/// than `max` lines (`function_spans`), keyed `<path>::<name>`.
fn long_function_violations(root: &Path, max: usize) -> Violations {
    let mut violations = Violations::new();
    for src in rust_src_roots(root) {
        for (path, text) in non_test_rust_files(&src) {
            let path = rel(root, &path);
            for span in function_spans(&text)
                .into_iter()
                .filter(|span| span.lines > max)
            {
                add(
                    &mut violations,
                    format!("{path}::{}", span.name),
                    format!(
                        "line {}: `fn {}` spans {} lines",
                        span.line, span.name, span.lines
                    ),
                );
            }
        }
    }
    violations
}

/// A non-test function spans at most `MAX_FN_LINES` lines, from its `fn`
/// keyword to the brace closing its body: free functions, methods of `impl`
/// and `trait` blocks, nested functions alike. Keyed by `<file>::<function>`.
#[test]
fn functions_under_200_lines() {
    ratchet(
        "functions_under_200_lines",
        &format!("functions over {MAX_FN_LINES} lines (split them along what they do)"),
        &long_function_violations(&repo_root(), MAX_FN_LINES),
    );
}

/// The function reader measures a free function, a method in an `impl`, a
/// default method in a `trait` and a function nested in another, from the
/// `fn` line to the closing brace; it skips a declaration without a body, a
/// function type, a macro-built name, braces in a string, a `;` in an array
/// type, and every function under `#[cfg(test)]` (a helper, an inline test
/// module, a test-only `impl`).
#[test]
fn function_spans_reads_every_function_with_a_body() {
    const TEXT: &str = "\
pub fn free(x: [u8; 4]) -> u8 {
    let s = \"} not the end {\";
    x[0]
}
pub struct S;
impl S {
    pub(crate) fn method(&self) {
        fn nested() {
        }
    }
}
pub trait T {
    fn declared(&self);
    fn defaulted(&self) -> fn(u8) -> u8 {
        |x| x
    }
}
macro_rules! make {
    ($name:ident) => {
        fn $name() {}
    };
}
#[cfg(test)]
fn test_helper() {
}
#[cfg(test)]
impl S {
    fn test_only(&self) {}
}
#[cfg(test)]
mod tests {
    fn in_tests() {
    }
}
";
    let spans = function_spans(TEXT);
    let found: Vec<(usize, &str, usize)> = spans
        .iter()
        .map(|span| (span.line, span.name.as_str(), span.lines))
        .collect();
    assert_eq!(
        found,
        vec![
            (1, "free", 4),
            (7, "method", 4),
            (8, "nested", 2),
            (14, "defaulted", 3),
        ]
    );
}

/// The function length check names each function longer than the bound in a
/// non-test file, keyed `<path>::<name>`, a method as well as a free
/// function; it passes a function at the bound, a long function in a test
/// file (`tests.rs`, a file under `tests/`, a `#[cfg(test)] mod` file) and a
/// long test in an inline test module.
#[test]
fn function_length_check_names_each_function_over_the_bound() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-fn-length-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    // Spans 5 lines: over a bound of 4.
    const LONG: &str = "fn long() {\n    let a = 1;\n    let b = 2;\n    let _ = a + b;\n}\n";
    // Spans 4 lines: at the bound.
    const AT_BOUND: &str = "fn at_bound() {\n    let a = 1;\n    let _ = a;\n}\n";
    let methods =
        "pub struct Big;\nimpl Big {\n    pub fn grow(&self) {\n        let a = 1;\n        \
                   let b = 2;\n        let _ = a + b;\n    }\n}\n";
    let inline_tests = format!("{AT_BOUND}#[cfg(test)]\nmod tests {{\n{LONG}}}\n");
    let files: Vec<(&str, &str)> = vec![
        (
            "crates/alpha/src/lib.rs",
            "mod free;\nmod methods;\nmod inline;\n#[cfg(test)]\nmod checks;\n",
        ),
        ("crates/alpha/src/free.rs", LONG),
        ("crates/alpha/src/methods.rs", methods),
        ("crates/alpha/src/inline.rs", &inline_tests),
        ("crates/alpha/src/checks.rs", LONG),
        ("crates/alpha/src/free/tests.rs", LONG),
        ("crates/alpha/src/tests/long.rs", LONG),
    ];
    let setup = write_order_tree(&dir, &files);
    let violations = long_function_violations(&dir, 4);
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        violation_list(violations),
        vec![
            (
                "crates/alpha/src/free.rs::long".to_owned(),
                vec!["line 1: `fn long` spans 5 lines".to_owned()]
            ),
            (
                "crates/alpha/src/methods.rs::grow".to_owned(),
                vec!["line 3: `fn grow` spans 5 lines".to_owned()]
            ),
        ]
    );
}

// ---------------------------------------------------------------------------
// The large files of the in-file split
// ---------------------------------------------------------------------------
//
// The files the cleanup splits along their responsibilities, each with the
// non-test lines it may keep once split. A bound is about what the planned
// split leaves, with a margin, and below the file's size before the split,
// so the check fails until the split lands. `non_test_code_under_700_lines`
// and `functions_under_200_lines` keep the result; this list goes once every
// file is split.
//
// `crates/khora-infra/src/ui/egui/ui_builder.rs` is not listed: it is one
// `impl UiBuilder for EguiUiBuilder` of about ninety methods, each of which
// stays in the file as at least a delegating method, so what it keeps cannot
// be bounded much below `MAX_NON_TEST_LINES`, which already holds it.

/// Each file to split, with the non-test lines it may keep (its folder's
/// `mod.rs` when it becomes a folder module).
const LARGE_FILES: &[(&str, usize)] = &[
    // 1196 lines, one trait impl: its methods delegate to inherent methods in
    // sibling files (pipelines, buffers, textures, bind groups); about 31
    // delegating methods and the device queries stay.
    (
        "crates/khora-infra/src/graphics/wgpu/device/graphics_device.rs",
        400,
    ),
    // 1088 lines: the fault, the step, the frames and the register file
    // leave; `Machine`, its run loop and its accessors stay.
    ("crates/khora-script/src/vm/mod.rs", 500),
    // 700 lines, one derive: attributes, fields, SoA, mirror, schema and
    // registration each in their file under `component/`.
    ("crates/khora-macros/src/component.rs", 400),
    // 700 lines: the progress, the advance and the park leave.
    ("crates/khora-lanes/src/script/turn.rs", 400),
    // 695 lines: the round, the admission, the plan, the share and the units.
    ("crates/khora-control/src/gorna/decide.rs", 400),
    // 665 non-test lines: the defaults, the engine resources, the phase
    // order and the scheduler's boot leave.
    ("crates/khora-sdk/src/engine/bootstrap.rs", 400),
    // 529 lines: the lending and the layers leave.
    ("crates/khora-control/src/scheduler/frame.rs", 400),
    // 362 lines: the panic guard and the pooled run (about 60 lines) leave.
    ("crates/khora-control/src/scheduler/execution.rs", 320),
    // 465 lines: the render frame (about 230 lines) and the app update leave.
    ("crates/khora-sdk/src/engine/frame.rs", 300),
    // 697 non-test lines: the type-erased store, the sub-assets and the
    // invalidation with its announcements leave.
    ("crates/khora-io/src/asset/asset_service.rs", 450),
];

/// Files of `LARGE_FILES` whose tests leave them for a `tests.rs` or a
/// `tests/` folder beside them.
const LARGE_FILES_WITHOUT_INLINE_TESTS: &[&str] = &["crates/khora-io/src/asset/asset_service.rs"];

/// The file `path` (relative to `root`) is now: itself, or its folder's
/// `mod.rs` when it became a folder module; `None` when neither exists.
fn split_file_now(root: &Path, path: &str) -> Option<PathBuf> {
    let file = root.join(path);
    if file.is_file() {
        return Some(file);
    }
    let folder_mod = file.with_extension("").join("mod.rs");
    folder_mod.is_file().then_some(folder_mod)
}

/// Each file of `files` (relative to `root`) holding more non-test lines
/// than its bound, and each of `without_inline_tests` still holding an
/// inline test module, as `<path>: …`. A file gone entirely (neither it nor
/// its folder's `mod.rs`) is split.
fn large_file_problems(
    root: &Path,
    files: &[(&str, usize)],
    without_inline_tests: &[&str],
) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, bound) in files {
        let Some(now) = split_file_now(root, path) else {
            continue;
        };
        let lines = non_test_lines(&read_text(&now));
        if lines > *bound {
            problems.push(format!(
                "{}: {lines} non-test lines, more than {bound} (split it along its \
                 responsibilities)",
                rel(root, &now)
            ));
        }
    }
    for path in without_inline_tests {
        let Some(now) = split_file_now(root, path) else {
            continue;
        };
        let inline: Vec<String> = top_level_items(&read_text(&now))
            .iter()
            .filter(|item| item.is_inline_test_module())
            .map(|item| format!("`{}` (line {})", item.name, item.line))
            .collect();
        if !inline.is_empty() {
            problems.push(format!(
                "{}: inline test module {} (move its tests to `tests.rs` or `tests/`)",
                rel(root, &now),
                inline.join(", ")
            ));
        }
    }
    problems.sort();
    problems
}

/// The large files are split along their responsibilities: each file of
/// `LARGE_FILES` (or its folder's `mod.rs`) holds at most its bound of
/// non-test lines, and the asset service holds no inline test module.
/// Strict.
#[test]
fn the_large_files_are_split() {
    let problems = large_file_problems(&repo_root(), LARGE_FILES, LARGE_FILES_WITHOUT_INLINE_TESTS);
    assert!(
        problems.is_empty(),
        "{} large files are not split yet:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// The large-file check measures a file, or its folder's `mod.rs` once it is
/// a folder module, against its bound, counting only the lines above its
/// inline test module; it names an inline test module in a file that must
/// hold none, and passes a file gone entirely.
#[test]
fn large_file_check_names_a_file_over_its_bound_or_holding_its_tests() {
    let dir = std::env::temp_dir().join(format!(
        "khora-layout-large-files-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let four = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";
    let tested = format!("fn a() {{}}\n#[cfg(test)]\nmod tests {{\n{four}}}\n");
    let files: Vec<(&str, &str)> = vec![
        ("crates/alpha/src/big.rs", four),
        ("crates/alpha/src/folder/mod.rs", four),
        ("crates/alpha/src/small.rs", "fn a() {}\n"),
        ("crates/alpha/src/tested.rs", &tested),
        ("crates/alpha/src/moved_out/mod.rs", "mod tests;\n"),
    ];
    let setup = write_order_tree(&dir, &files);
    let problems = large_file_problems(
        &dir,
        &[
            ("crates/alpha/src/big.rs", 3),
            ("crates/alpha/src/folder.rs", 3),
            ("crates/alpha/src/small.rs", 3),
            ("crates/alpha/src/tested.rs", 3),
            ("crates/alpha/src/gone.rs", 3),
        ],
        &[
            "crates/alpha/src/tested.rs",
            "crates/alpha/src/moved_out.rs",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp tree");

    assert_eq!(
        problems,
        vec![
            "crates/alpha/src/big.rs: 4 non-test lines, more than 3 (split it along its \
             responsibilities)"
                .to_owned(),
            "crates/alpha/src/folder/mod.rs: 4 non-test lines, more than 3 (split it along its \
             responsibilities)"
                .to_owned(),
            "crates/alpha/src/tested.rs: inline test module `tests` (line 3) (move its tests to \
             `tests.rs` or `tests/`)"
                .to_owned(),
        ]
    );
}
