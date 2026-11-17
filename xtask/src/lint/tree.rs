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

//! Walks the repository tree the checks scan, and reads its files.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Directory names never walked into, wherever they appear.
pub const SKIP_DIRS: &[&str] = &["target", ".git", ".codegraph", "node_modules"];

/// Directories at the repository root never walked into: generated AI
/// wrappers and tool state, not part of the tree the checks govern.
///
/// `.agent/` and `.github/` are sources and stay scanned.
pub const SKIP_ROOT_DIRS: &[&str] = &[
    ".claude",
    ".cursor",
    ".gemini",
    ".codegraph",
    ".gitagent",
    ".khora",
    ".impeccable",
    ".dist",
];

/// Returns the repository this crate was built in: its manifest folder's parent.
///
/// # Panics
///
/// Panics when the manifest folder has no parent.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("{} has no parent", env!("CARGO_MANIFEST_DIR")))
}

/// Lists the Rust `src/` trees under `root` that exist: `crates/*/src`,
/// `examples/*/src`, `xtask/src`.
pub fn rust_src_roots(root: &Path) -> Vec<PathBuf> {
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

/// Lists the directories directly in `dir` the walkers may enter, sorted.
pub fn child_dirs(dir: &Path) -> Vec<PathBuf> {
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

/// Tells whether the walkers must not enter the directory `path`:
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
pub fn is_skipped(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    SKIP_DIRS.iter().any(|skip| name == *skip)
        || (path.parent() == Some(repo_root().as_path())
            && SKIP_ROOT_DIRS.iter().any(|skip| name == *skip))
        || path.join(".git").exists()
}

/// Lists every file under `dir`, recursively, sorted; directories
/// [`is_skipped`] rejects are not entered.
pub fn walk_files(dir: &Path) -> Vec<PathBuf> {
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

/// Lists every directory strictly below `dir`, recursively, sorted;
/// directories [`is_skipped`] rejects are neither listed nor entered.
pub fn walk_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut stack = child_dirs(dir);
    while let Some(current) = stack.pop() {
        stack.extend(child_dirs(&current));
        dirs.push(current);
    }
    dirs.sort();
    dirs
}

/// Returns `path` relative to `root`, with `/` separators.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Returns the last component of `path`, or an empty string.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Tells whether `path` has the extension `ext`.
pub fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|found| found == ext)
}

/// Resolves `.` and `..` lexically, without touching the file system.
pub fn normalize(path: &Path) -> PathBuf {
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

/// Reads `path` as text, a leading byte order mark dropped.
///
/// # Panics
///
/// Panics when the file cannot be read.
pub fn read_text(path: &Path) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    text.strip_prefix('\u{feff}')
        .map(str::to_owned)
        .unwrap_or(text)
}
