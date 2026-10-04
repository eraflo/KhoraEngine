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
    "every_agent_folder_has_agent_rs",
    "lanes_mirror_agents",
    "no_tracked_ignored_files",
    "relative_links_resolve",
    "cited_source_paths_exist",
    "no_duplicate_asset_files",
    "one_spelling_of_util",
    "no_generic_module_file_names",
    "docs_pages_are_kebab_case",
    "no_stuttering_file_names",
    "retired_file_names",
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
const BATCH_WORDS: &[&str] = &["B0", "B1", "B2", "B3", "B4", "B5", "B6", "Ergon"];

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
/// after its primary type" comes first: `physics/physics_material.rs` holds
/// `PhysicsMaterial`, `asset/asset_uuid.rs` would hold `AssetUUID`. "Named after"
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
        ("PhysicsMaterial", "physics_material"),
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
