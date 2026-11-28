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

//! The entries made beside another for a while — a write's temporary file, a
//! build's folders, a rename's entry moved aside — and what reclaims them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::Reclaimed;

/// What ends the name of a temporary file or folder made beside another.
const TEMPORARY_SUFFIX: &str = ".tmp";
/// What ends the name of a previous build moved aside while a new one is swapped in.
const PREVIOUS_SUFFIX: &str = ".previous.tmp";
/// What ends the name of an entry [`rename_entry`](super::rename_entry) moves aside on its way to another case.
const ASIDE_SUFFIX: &str = ".aside.tmp";

/// A number for each temporary file this process makes: two writes at once
/// never share one.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// Returns whether `name` is the name of an entry made beside another for a while: a write's, a build's or a rename's.
///
/// `<name>.<pid>.<n>.tmp`, `.previous.tmp` or `.aside.tmp`, as this module
/// makes them. A walk of a project never goes into such a folder: what it
/// holds is under another name, or not whole yet.
pub fn is_temporary(name: &str) -> bool {
    temporary_of(name).is_some()
}

/// Puts back under its name each entry under `root` a [`rename_entry`](super::rename_entry) cut short left aside; returns what it did with each.
///
/// An entry moved aside on its way to another case is put back at the name
/// it had when nothing is there; with an entry there, it is left where it is,
/// never removed. Links are not followed, nor is any other
/// [temporary](is_temporary) folder; every other entry is left as it is.
pub fn reclaim_asides(root: &Path) -> Vec<Reclaimed> {
    let mut asides = Vec::new();
    let mut walk = walkdir::WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = walk.next() {
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 {
            continue;
        }
        if is_temporary_folder(&entry) {
            walk.skip_current_dir();
        }
        let name = entry.file_name().to_string_lossy();
        if let Some((original, Leftover::Aside)) = temporary_of(&name) {
            asides.push((
                entry.path().to_path_buf(),
                entry.path().with_file_name(original),
            ));
        }
    }
    asides
        .into_iter()
        .map(|(aside, original)| put_back(aside, original))
        .collect()
}

/// What an entry made beside another for a while is, by its name.
pub(super) enum Leftover {
    /// A build's temporary folder, or a write's temporary file.
    Staged,
    /// A previous build moved aside.
    Previous,
    /// An entry a rename in case only moved aside.
    Aside,
}

/// Returns what the entry named `entry` beside the folder named `name` is, by
/// the pattern [`temporary_beside`] and [`previous_beside`] name it; `None` for any other.
pub(super) fn leftover_of(name: &str, entry: &str) -> Option<Leftover> {
    temporary_of(entry)
        .filter(|(original, _)| *original == name)
        .map(|(_, leftover)| leftover)
}

/// Returns the name the entry named `entry` was made beside, and what it is, when its name is a temporary's.
pub(super) fn temporary_of(entry: &str) -> Option<(&str, Leftover)> {
    let (rest, leftover) = if let Some(rest) = entry.strip_suffix(PREVIOUS_SUFFIX) {
        (rest, Leftover::Previous)
    } else if let Some(rest) = entry.strip_suffix(ASIDE_SUFFIX) {
        (rest, Leftover::Aside)
    } else {
        (entry.strip_suffix(TEMPORARY_SUFFIX)?, Leftover::Staged)
    };
    let (rest, count) = rest.rsplit_once('.')?;
    let (original, pid) = rest.rsplit_once('.')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (!original.is_empty() && digits(pid) && digits(count)).then_some((original, leftover))
}

/// Returns whether a walk's `entry` is a folder named as a [temporary](is_temporary), one a walk never goes into.
pub(super) fn is_temporary_folder(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir() && is_temporary(&entry.file_name().to_string_lossy())
}

/// Returns a temporary file's path beside `path`, unique to this process and this call.
///
/// Named `<path>.<pid>.<n>.tmp`, a [scratch](super::is_scratch) name: a walk running
/// meanwhile never counts it.
pub(super) fn temporary_beside(path: &Path) -> PathBuf {
    numbered_beside(path, TEMPORARY_SUFFIX)
}

/// Returns the path a previous build is moved aside to beside `path`, unique to this process and this call.
///
/// Named `<path>.<pid>.<n>.previous.tmp`, a [scratch](super::is_scratch) name.
pub(super) fn previous_beside(path: &Path) -> PathBuf {
    numbered_beside(path, PREVIOUS_SUFFIX)
}

/// Returns the path an entry renamed in case only is moved aside to beside `path`, unique to this process and this call.
///
/// Named `<path>.<pid>.<n>.aside.tmp`, a [scratch](super::is_scratch) name
/// [`reclaim_asides`] knows.
pub(super) fn aside_beside(path: &Path) -> PathBuf {
    numbered_beside(path, ASIDE_SUFFIX)
}

/// Returns `<path>.<pid>.<n><suffix>`, `n` this process's next number.
fn numbered_beside(path: &Path, suffix: &str) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}.{}{suffix}",
        std::process::id(),
        TEMPORARIES.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(tmp)
}

/// Renames the entry `aside` back to `original` when nothing is there; returns what it did.
fn put_back(aside: PathBuf, original: PathBuf) -> Reclaimed {
    if std::fs::symlink_metadata(&original).is_ok() {
        let error = format!("{} is there", original.display());
        return Reclaimed::Stranded { path: aside, error };
    }
    match std::fs::rename(&aside, &original) {
        Ok(()) => Reclaimed::Restored {
            from: aside,
            to: original,
        },
        Err(error) => Reclaimed::Stranded {
            path: aside,
            error: format!(
                "it could not be put back at {}: {error}",
                original.display()
            ),
        },
    }
}
