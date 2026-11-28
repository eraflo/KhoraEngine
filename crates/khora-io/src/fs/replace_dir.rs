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

//! Building a folder whole, beside the one it replaces.

use std::path::{Path, PathBuf};

use super::temporary::{leftover_of, previous_beside, temporary_beside, Leftover};
use super::write::make_parent;
use super::Reclaimed;

/// Builds the folder at `path` whole through `build`: an error leaves the previous folder as it was.
///
/// As [`replace_dir_with_reporting`], what it did with what an interrupted
/// build left beside the folder logged instead of returned.
///
/// # Errors
///
/// As [`replace_dir_with_reporting`].
pub fn replace_dir_with<T, E: From<std::io::Error>>(
    path: &Path,
    build: impl FnOnce(&Path) -> Result<T, E>,
) -> Result<T, E> {
    let (built, reclaimed) = replace_dir_with_reporting(path, build);
    for reclaimed in reclaimed {
        match reclaimed {
            Reclaimed::Left { .. } => log::warn!("{reclaimed}"),
            _ => log::info!("{reclaimed}"),
        }
    }
    built
}

/// Builds the folder at `path` whole through `build`, and returns what it did with an interrupted build's leftovers.
///
/// First, what an earlier build interrupted (killed, the machine stopped) left
/// beside `path` is reclaimed: with no folder at `path`, the previous build it
/// had moved aside is put back; every other leftover is removed. A leftover is
/// a folder named after `path` by the pattern only this function and
/// [`write_atomic`](super::write_atomic) use (`<name>.<pid>.<n>.tmp`, `<name>.<pid>.<n>.previous.tmp`),
/// so no other folder is touched. Then `build` fills a temporary folder beside
/// `path`, which it is given. Once it returns, the previous folder is moved
/// aside, the new one renamed to `path` and the previous one removed; a rename
/// that fails puts the previous folder back. An error from `build` or a rename
/// removes the temporary folder. Makes the folder's parent.
///
/// Returns the build's result and, either way, each leftover reclaimed, put
/// back or left, and the previous build left when it could not be removed.
/// One folder is built by one build at a time: the caller holds a
/// [`PathLock`](super::PathLock) on `path` around the call, so a second build of the folder,
/// in this process or another, never reclaims the first's temporary folder.
///
/// # Errors
///
/// What `build` returns; otherwise when the parent or the temporary folder
/// cannot be made, or a rename fails: the previous folder held open by
/// another program, say.
pub fn replace_dir_with_reporting<T, E: From<std::io::Error>>(
    path: &Path,
    build: impl FnOnce(&Path) -> Result<T, E>,
) -> (Result<T, E>, Vec<Reclaimed>) {
    let mut reclaimed = Vec::new();
    if let Err(error) = make_parent(path) {
        return (Err(error.into()), reclaimed);
    }
    reclaim_beside(path, &mut reclaimed);
    let staged = temporary_beside(path);
    if let Err(error) = std::fs::create_dir(&staged) {
        return (Err(error.into()), reclaimed);
    }
    let built = build(&staged).and_then(|value| {
        swap_in(&staged, path, &mut reclaimed)?;
        Ok(value)
    });
    if built.is_err() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    (built, reclaimed)
}

/// Renames the folder `staged` to `path`, the folder at `path` moved aside and removed.
///
/// A rename that fails puts the folder at `path` back. A previous folder that
/// cannot be removed is added to `reclaimed` as left.
fn swap_in(staged: &Path, path: &Path, reclaimed: &mut Vec<Reclaimed>) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path).is_err() {
        return std::fs::rename(staged, path);
    }
    let previous = previous_beside(path);
    std::fs::rename(path, &previous)?;
    if let Err(error) = std::fs::rename(staged, path) {
        let _ = std::fs::rename(&previous, path);
        return Err(error);
    }
    if let Some(left) = remove_leftover(&previous).filter(|r| matches!(r, Reclaimed::Left { .. })) {
        reclaimed.push(left);
    }
    Ok(())
}

/// Reclaims what an interrupted build left beside the folder at `path`, adding each to `reclaimed`.
///
/// With no folder at `path`, the newest previous build moved aside is put
/// back; every other leftover is removed.
fn reclaim_beside(path: &Path, reclaimed: &mut Vec<Reclaimed>) {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let mut previous = Vec::new();
    let mut staged = Vec::new();
    for entry in entries.flatten() {
        // A link is never followed nor removed: no build makes one.
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let entry_name = entry.file_name();
        match entry_name.to_str().and_then(|n| leftover_of(name, n)) {
            Some(Leftover::Previous) => previous.push(entry.path()),
            Some(Leftover::Staged) => staged.push(entry.path()),
            // A rename's, put back at a project's open, never by a build.
            Some(Leftover::Aside) | None => {}
        }
    }
    if std::fs::symlink_metadata(path).is_err() {
        let modified = |at: &PathBuf| std::fs::metadata(at).and_then(|m| m.modified()).ok();
        previous.sort_by_key(modified);
        if let Some(newest) = previous.pop() {
            match std::fs::rename(&newest, path) {
                Ok(()) => reclaimed.push(Reclaimed::PutBack { from: newest }),
                Err(error) => reclaimed.push(Reclaimed::Left {
                    path: newest,
                    error: format!("it could not be put back: {error}"),
                }),
            }
        }
    }
    for leftover in previous.into_iter().chain(staged) {
        reclaimed.extend(remove_leftover(&leftover));
    }
}

/// Removes the folder `at` an earlier build left; `None` when it is already gone.
fn remove_leftover(at: &Path) -> Option<Reclaimed> {
    match std::fs::remove_dir_all(at) {
        Ok(()) => Some(Reclaimed::Removed {
            path: at.to_path_buf(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => Some(Reclaimed::Left {
            path: at.to_path_buf(),
            error: error.to_string(),
        }),
    }
}
