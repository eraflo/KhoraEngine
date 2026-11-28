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

//! Paths as the file system resolves them: under a root or not, spelled on disk, one entry or two.

use std::path::{Component, Path, PathBuf, Prefix};

/// Returns `path` as the file system resolves it, or as given when it does not resolve.
pub fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Returns `path` as the file system spells it: the part of it that exists resolved, the rest as given.
///
/// [`real_path`] for a path that exists. For one that does not, its deepest
/// folder that exists is resolved and the names below it kept, so a file not
/// made yet, named through a folder in another case, is named as that folder is.
pub fn spelled_on_disk(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let mut below = Vec::new();
    let mut at = path;
    while let Some(parent) = at.parent() {
        if let Some(name) = at.file_name() {
            below.push(name.to_owned());
        } else {
            // A `..` or a `.` ending `at`: resolved with the rest once found.
            break;
        }
        if let Ok(mut real) = std::fs::canonicalize(parent) {
            real.extend(below.iter().rev());
            return real;
        }
        at = parent;
    }
    path.to_path_buf()
}

/// Returns `path` relative to `root`, as given or once resolved; `None` outside it.
///
/// A path is under the root when it is under it as named or as resolved; a
/// file gone from disk, or not made yet, is under the root the folder it is
/// named in resolves under. A `..` in a path as named is applied to its names,
/// so it never climbs out unseen.
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    relative_to_resolved(path, root, &real_path(root))
}

/// Returns whether `path` is `root` or lies under it, as [`relative_to`] decides.
pub fn is_under(path: &Path, root: &Path) -> bool {
    relative_to(path, root).is_some()
}

/// Returns `path` relative to `real_root`, a folder as the file system resolves it, once `path` is resolved; `None` outside it.
///
/// `path` is resolved as [`spelled_on_disk`] spells it: the deepest part of it
/// that exists, links followed and names as spelled on disk, then the names
/// below it as given. So a path through a link that leads out of the root is
/// outside, whatever its names say: the rule the trust guard refuses a read
/// as an escape by.
pub fn resolved_relative_to(path: &Path, real_root: &Path) -> Option<PathBuf> {
    resolved_within(&spelled_on_disk(path), real_root)
}

/// Returns whether `a` and `b` name one entry of a folder: the same file or folder, in another case where file names ignore it.
///
/// Both lie in the same folder once resolved, their names are equal but for
/// case, and they are one entry on disk. Two hard links to one file are two
/// entries, and so are a link and what it leads to. False when either is missing.
pub fn is_same_entry(a: &Path, b: &Path) -> bool {
    let (Some(name_a), Some(name_b)) = (a.file_name(), b.file_name()) else {
        return false;
    };
    if name_a.to_string_lossy().to_lowercase() != name_b.to_string_lossy().to_lowercase() {
        return false;
    }
    let folder = |path: &Path| {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        std::fs::canonicalize(parent.unwrap_or(Path::new("."))).ok()
    };
    folder(a).is_some_and(|folder_a| folder(b) == Some(folder_a)) && one_entry_on_disk(a, b)
}

/// Returns `path` relative to the root named `root` and resolved to `real_root`.
///
/// See [`relative_to`].
pub(super) fn relative_to_resolved(path: &Path, root: &Path, real_root: &Path) -> Option<PathBuf> {
    if let Some(rel) = named_under(path, root) {
        return Some(rel);
    }
    if let Some(rel) = named_under(path, real_root) {
        return Some(rel);
    }
    // A resolved path on Windows is verbatim (`\\?\C:\…`); a path as given never is.
    if let Some(rel) = plain(real_root).and_then(|plain_root| named_under(path, &plain_root)) {
        return Some(rel);
    }
    named_under(&spelled_on_disk(path), real_root)
}

/// Returns `real`, a path as the file system resolves it, relative to `real_root`, resolved too; `None` outside it.
///
/// The one rule a resolved path is held to a root by: [`resolved_relative_to`]
/// and the trust guard's escape check.
pub(crate) fn resolved_within(real: &Path, real_root: &Path) -> Option<PathBuf> {
    named_under(real, real_root)
}

/// Returns `path` relative to `root` by their names alone, `..` applied; `None` outside it.
fn named_under(path: &Path, root: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(root).ok()?;
    let mut rel = PathBuf::new();
    for component in rest.components() {
        match component {
            Component::Normal(name) => rel.push(name),
            Component::CurDir => {}
            // Climbing above the root leaves it.
            Component::ParentDir => {
                if !rel.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(rel)
}

/// Returns a verbatim disk path (`\\?\C:\…`) as a plain one (`C:\…`); `None` for any other.
fn plain(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    let Prefix::VerbatimDisk(letter) = prefix.kind() else {
        return None;
    };
    let mut plain = PathBuf::from(format!("{}:\\", char::from(letter)));
    plain.extend(components.filter(|c| !matches!(c, Component::RootDir)));
    Some(plain)
}

/// Returns whether `a` and `b` are one entry on disk: one file identity, a link not followed.
fn one_entry_on_disk(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        // No file identity without a handle: the one path both resolve to.
        std::fs::canonicalize(a)
            .is_ok_and(|real| std::fs::canonicalize(b).is_ok_and(|other| other == real))
    }
}
