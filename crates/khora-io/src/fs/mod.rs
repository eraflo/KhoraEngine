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

//! The file-system helpers every tool shares: resolving paths, writing a file
//! whole, reading one under a cap, walking a tree.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};

mod bounded_read_error;
mod file_stamp;
mod keyed;
mod ron_store;
mod root_name;

pub use bounded_read_error::BoundedReadError;
pub use file_stamp::FileStamp;
pub use keyed::Keyed;
pub use ron_store::RonStore;
pub use root_name::RootName;

/// A number for each temporary file this process makes: two writes at once
/// never share one.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// Returns `path` as the file system resolves it, or as given when it does not resolve.
pub fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Returns `path` relative to `root`, as given or once resolved; `None` outside it.
///
/// A path is under the root when it is under it as named or as resolved; a
/// file gone from disk is still under the root it was named under. A `..` in
/// a path as named is applied to its names, so it never climbs out unseen.
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    relative_to_resolved(path, root, &real_path(root))
}

/// Returns whether `path` is `root` or lies under it, as [`relative_to`] decides.
pub fn is_under(path: &Path, root: &Path) -> bool {
    relative_to(path, root).is_some()
}

/// Writes `bytes` at `path` whole: a crash or an error leaves the previous file as it was.
///
/// Makes the file's folder. See [`write_atomic_with`].
///
/// # Errors
///
/// [`std::io::ErrorKind::PermissionDenied`] when the file at `path` is
/// read-only; otherwise when the folder, the temporary file, the sync or the
/// rename fail.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_with(path, |file| file.write_all(bytes))
}

/// Writes the file at `path` whole through `write`: an error leaves the previous file as it was.
///
/// The file is written whole or not at all. A file already there keeps its
/// name and place: `path` spelled in another case, or a link, replaces the
/// file it names. Makes the file's folder. A scratch `.tmp` file may appear
/// beside it while it is written.
///
/// # Errors
///
/// What `write` returns; [`std::io::ErrorKind::PermissionDenied`] when the
/// file at `path` is read-only; otherwise when the folder, the temporary file,
/// the sync or the rename fail.
pub fn write_atomic_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    make_parent(path)?;
    // A rename takes the new name: onto `path` as spelled, a case-insensitive
    // system would rename `turret.kprefab` to `Turret.kprefab`.
    let target = real_path(path);
    // A rename replaces a read-only file on Unix, where writing into it is refused.
    if std::fs::metadata(&target).is_ok_and(|meta| meta.permissions().readonly()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is read-only", target.display()),
        ));
    }
    let tmp = temporary_beside(&target);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            write(&mut file)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Writes `bytes` at `path` whole, only when nothing is there: a file at `path` is never replaced.
///
/// Makes the file's folder.
///
/// # Errors
///
/// [`std::io::ErrorKind::AlreadyExists`] when a file is at `path`; otherwise
/// when the folder or the file cannot be written.
pub fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_new_as(path, bytes, Access::Default)
}

/// Writes `bytes` at `path` as [`write_new`] does, readable by its owner only on Unix.
///
/// For a secret of the user's, such as a signing key.
///
/// # Errors
///
/// As [`write_new`].
pub fn write_new_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_new_as(path, bytes, Access::OwnerOnly)
}

/// Returns the bytes of the file at `path`, refused past `cap` bytes.
///
/// Its size is read from the open file and the read stops at `cap + 1` bytes,
/// so a file growing during the read is refused too.
///
/// # Errors
///
/// [`BoundedReadError::TooLarge`] past `cap`; [`BoundedReadError::Io`] when the
/// file cannot be opened or read (`NotFound` for a missing one).
pub fn read_bounded(path: &Path, cap: u64) -> Result<Vec<u8>, BoundedReadError> {
    let file = std::fs::File::open(path).map_err(BoundedReadError::Io)?;
    let metadata = file.metadata().map_err(BoundedReadError::Io)?;
    if metadata.len() > cap {
        return Err(BoundedReadError::TooLarge {
            size: metadata.len(),
            cap,
            stamp: FileStamp::of_metadata(&metadata),
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(BoundedReadError::Io)?;
    if bytes.len() as u64 > cap {
        // It grew past its cap since its size was read.
        return Err(BoundedReadError::TooLarge {
            size: bytes.len() as u64,
            cap,
            stamp: None,
        });
    }
    Ok(bytes)
}

/// Returns the text of the file at `path`, refused past `cap` bytes, as [`read_bounded`].
///
/// # Errors
///
/// As [`read_bounded`], and [`BoundedReadError::Io`] of kind `InvalidData`
/// when the file is not UTF-8.
pub fn read_bounded_to_string(path: &Path, cap: u64) -> Result<String, BoundedReadError> {
    let bytes = read_bounded(path, cap)?;
    String::from_utf8(bytes).map_err(|error| {
        BoundedReadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

/// Returns whether a file named `name` is an editor's scratch file, never counted as a project file.
///
/// A backup (`name~`), a lock (`.#name`), a swap file (`.swp`) or a temporary
/// file (`.tmp`), the one [`write_atomic`] makes included.
pub fn is_scratch(name: &str) -> bool {
    name.ends_with('~')
        || name.starts_with(".#")
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
}

/// Returns every file under `root`, at any depth, by its path under `root`, sorted.
///
/// Links are not followed: they could lead out of `root`. [Scratch
/// files](is_scratch) are left out. Empty for a root that does not exist; an
/// entry that cannot be read is skipped.
pub fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file() && !is_scratch(&entry.file_name().to_string_lossy())
        })
        .filter_map(|entry| entry.path().strip_prefix(root).ok().map(Path::to_path_buf))
        .collect();
    found.sort();
    found
}

/// Returns `path` relative to the root named `root` and resolved to `real_root`.
///
/// See [`relative_to`].
fn relative_to_resolved(path: &Path, root: &Path, real_root: &Path) -> Option<PathBuf> {
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
    named_under(&real_path(path), real_root)
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

/// Who may read a file [`write_new_as`] makes.
#[derive(Clone, Copy)]
enum Access {
    /// What the system gives a new file.
    Default,
    /// Its owner alone, on Unix.
    OwnerOnly,
}

/// Writes `bytes` at `path`, never over a file there, through a file beside it linked in place.
fn write_new_as(path: &Path, bytes: &[u8], access: Access) -> std::io::Result<()> {
    make_parent(path)?;
    let tmp = temporary_beside(path);
    let written = create_new(&tmp, access).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let linked = written.and_then(|()| match std::fs::hard_link(&tmp, path) {
        // A file system without hard links: the file is made in place,
        // still never over one that is there.
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => create_new(path, access)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.sync_all()
            }),
        linked => linked,
    });
    let _ = std::fs::remove_file(&tmp);
    linked
}

/// Opens a new file at `path` for writing, failing when one is there.
fn create_new(path: &Path, access: Access) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if matches!(access, Access::OwnerOnly) {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = access;
    options.open(path)
}

/// Makes the folder the file at `path` goes in.
fn make_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => std::fs::create_dir_all(folder),
        _ => Ok(()),
    }
}

/// Returns a temporary file's path beside `path`, unique to this process and this call.
///
/// It ends in `.tmp`, a [scratch](is_scratch) name: a walk running meanwhile
/// never counts it.
fn temporary_beside(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        TEMPORARIES.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(tmp)
}

#[cfg(test)]
mod tests;
