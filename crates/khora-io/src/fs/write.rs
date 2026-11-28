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

//! Writing a file whole: replacing one, or making one where none is.

use std::io::Write;
use std::path::Path;

use super::real_path;
use super::temporary::temporary_beside;

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
pub(super) fn make_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => std::fs::create_dir_all(folder),
        _ => Ok(()),
    }
}
