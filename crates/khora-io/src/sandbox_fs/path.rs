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

//! Deciding whether a path is allowed, before anything is opened.
//!
//! Two checks, and both are needed — neither catches what the other does.
//!
//! **Lexical** ([`parse`]) rejects the shape of the path: an absolute path, a
//! `..` component, a backslash that would be a separator on Windows, a `:` that
//! would name an alternate data stream. This is cheap and runs first, so the
//! obvious attempt never reaches the filesystem at all.
//!
//! **Containment** ([`descend`]) rejects where the path actually *lands*. A path
//! made only of plain names still escapes if one of those names is a symlink
//! pointing out of the root, and no amount of string inspection can see that —
//! only asking the filesystem where a link goes can. So each level is resolved
//! and checked against the root as the walk descends, rather than resolving the
//! whole path and checking once: a link caught at the second component never
//! gets the third one appended to it.

use std::path::{Path, PathBuf};

use super::{FsError, Root};

/// A path that has passed the lexical checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualPath {
    /// Which root it names.
    pub root: Root,
    /// The path under that root, in plain components.
    pub components: Vec<String>,
}

/// Splits `"save://slot1.json"` into a root and its components, refusing
/// anything that could name something outside that root.
pub fn parse(path: &str) -> Result<VirtualPath, FsError> {
    let Some((scheme, rest)) = path.split_once("://") else {
        return Err(FsError::NoRoot(path.to_owned()));
    };

    let root = Root::from_scheme(scheme).ok_or_else(|| FsError::UnknownRoot(scheme.to_owned()))?;

    // A leading separator would make the rest look absolute to `Path::join`,
    // which silently *replaces* the root rather than extending it — the one
    // way a join can lose its base without erroring.
    if rest.starts_with('/') || rest.starts_with('\\') {
        return Err(FsError::Absolute(path.to_owned()));
    }

    let mut components = Vec::new();
    for raw in rest.split('/') {
        // Trailing and doubled separators are a typo, not an attack: `a//b` and
        // `a/b/` mean `a/b`, the same way the module resolver normalises them.
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            return Err(FsError::Escapes(path.to_owned()));
        }
        check_component(raw, path)?;
        components.push(raw.to_owned());
    }

    Ok(VirtualPath { root, components })
}

/// Rejects a component that would mean something other than a plain name.
///
/// Each of these is a separator or a namespace marker on *some* platform. A
/// script is written once and shipped everywhere, so a path is refused if it
/// would escape on any target — not only on the one building it.
fn check_component(component: &str, path: &str) -> Result<(), FsError> {
    if component.contains('\\') {
        return Err(FsError::Escapes(path.to_owned()));
    }
    // A drive letter (`C:`) or an NTFS alternate data stream (`file:hidden`).
    if component.contains(':') {
        return Err(FsError::Escapes(path.to_owned()));
    }
    if component.contains('\0') {
        return Err(FsError::Escapes(path.to_owned()));
    }
    Ok(())
}

/// Walks `components` down from `root`, checking containment at every level.
///
/// `create_missing` makes the intermediate directories as it goes, which a write
/// to a fresh save slot needs. The final component is never created here — it is
/// the file, and creating it is the caller's operation.
///
/// Returns the real path to operate on.
pub fn descend(
    root: &Path,
    components: &[String],
    create_missing: bool,
) -> Result<PathBuf, FsError> {
    let mut current = root.to_path_buf();

    let last = components.len().saturating_sub(1);
    for (index, component) in components.iter().enumerate() {
        current.push(component);

        let is_final = index == last;
        if current.exists() {
            // The link check. `symlink_metadata` does not follow, so this sees
            // the link itself; canonicalising then says where it actually goes.
            let real = current
                .canonicalize()
                .map_err(|error| FsError::Io(error.to_string()))?;
            if !real.starts_with(root) {
                return Err(FsError::Escapes(current.display().to_string()));
            }
            current = real;
        } else if create_missing && !is_final {
            std::fs::create_dir(&current).map_err(|error| FsError::Io(error.to_string()))?;
        }
    }
    Ok(current)
}

/// Canonicalises `root`, creating it if it does not exist yet.
///
/// Canonical up front so every containment check compares like with like: on
/// Windows `canonicalize` returns a `\\?\` path, and a prefix test against a
/// plain one would fail for every path in the sandbox.
pub fn prepare_root(root: &Path) -> Result<PathBuf, FsError> {
    if !root.exists() {
        std::fs::create_dir_all(root).map_err(|error| FsError::Io(error.to_string()))?;
    }
    root.canonicalize()
        .map_err(|error| FsError::Io(error.to_string()))
}
