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

//! The hold one writer has on a file or a folder it writes, across processes.

use std::path::{Path, PathBuf};

/// What ends the name of the lock file beside a held path.
const LOCK_SUFFIX: &str = ".lock";

/// The hold one writer has on the file or folder at a path, until it is dropped.
///
/// An exclusive OS lock on a file beside it, `<path>.lock`: two holds of one
/// path, in one process or two, never last at once. The lock is on a file of
/// its own, so the held file can be replaced by a rename while it is held.
/// The lock file is left in place once the hold ends: removed, a writer that
/// had it open would hold a file gone from its path, beside a writer that
/// makes the file anew. The system releases the lock of a process that ends,
/// killed or not, so a hold never outlives its writer. A program that takes
/// no such lock (`git`) is not kept out.
pub struct PathLock {
    /// The lock file beside the held path.
    path: PathBuf,
    /// The lock file, open and locked while the hold lasts.
    _file: std::fs::File,
}

impl PathLock {
    /// Holds the path for this writer, or refuses when another holds it.
    ///
    /// Never waits: a second writer is told at once. Makes the path's parent,
    /// and the lock file beside it.
    ///
    /// # Errors
    ///
    /// [`std::io::ErrorKind::ResourceBusy`] when another writer holds the
    /// path, in this process or another; otherwise when the parent or the
    /// lock file cannot be made, or the system cannot lock it.
    pub fn try_hold(path: &Path) -> std::io::Result<Self> {
        let (lock, file) = open_lock_file(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                path: lock,
                _file: file,
            }),
            Err(std::fs::TryLockError::WouldBlock) => Err(std::io::Error::new(
                std::io::ErrorKind::ResourceBusy,
                format!(
                    "{} is held by another writer, in this program or another (it holds {}): \
                     try again once it ends",
                    path.display(),
                    lock.display()
                ),
            )),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Holds the path for this writer, waiting until another that holds it lets it go.
    ///
    /// For a write that is short: the caller is blocked while another holds
    /// it. Makes the path's parent, and the lock file beside it.
    ///
    /// # Errors
    ///
    /// When the parent or the lock file cannot be made, or the system cannot lock it.
    pub fn hold(path: &Path) -> std::io::Result<Self> {
        let (lock, file) = open_lock_file(path)?;
        file.lock()?;
        Ok(Self {
            path: lock,
            _file: file,
        })
    }

    /// Returns the lock file beside the held path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Opens the lock file beside `path`, `<path>.lock`, making it and its folder.
fn open_lock_file(path: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut lock = path.as_os_str().to_owned();
    lock.push(LOCK_SUFFIX);
    let lock = PathBuf::from(lock);
    super::make_parent(&lock)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)?;
    Ok((lock, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// **A path is held by one writer at a time.**
    ///
    /// A second hold of one path is refused while the first lasts, by
    /// another handle as another process would take it, and says so; once the
    /// first is dropped, the path is held again.
    #[test]
    fn a_path_is_held_by_one_writer_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dist").join("windows");

        let first = PathLock::try_hold(&target).expect("the first writer holds it");
        let second = PathLock::try_hold(&target);

        let refused = second
            .err()
            .expect("a second writer of the path is refused");
        assert_eq!(refused.kind(), std::io::ErrorKind::ResourceBusy);
        assert!(refused.to_string().contains("another writer"), "{refused}");
        assert_eq!(first.path(), dir.path().join("dist").join("windows.lock"));
        drop(first);
        PathLock::try_hold(&target).expect("once the first ends, it is held again");
    }

    /// **Two paths are held at once.**
    #[test]
    fn two_paths_are_held_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let _windows = PathLock::try_hold(&dir.path().join("windows")).unwrap();
        PathLock::try_hold(&dir.path().join("linux")).expect("another path is free");
    }

    /// **A hold that waits gets the path once the writer holding it lets it go.**
    #[test]
    fn a_hold_that_waits_gets_the_path_once_the_other_lets_it_go() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".khora").join("asset-registry.ron");
        let first = PathLock::hold(&target).expect("the first writer holds it");
        let got = AtomicBool::new(false);

        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| {
                let held = PathLock::hold(&target).expect("the second writer holds it");
                got.store(true, Ordering::SeqCst);
                held
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(
                !got.load(Ordering::SeqCst),
                "the second writer held the path beside the first"
            );
            drop(first);
            let second = waiting.join().expect("the second writer ends");
            assert!(got.load(Ordering::SeqCst));
            assert!(
                PathLock::try_hold(&target).is_err(),
                "the second writer holds it"
            );
            drop(second);
        });
    }
}
