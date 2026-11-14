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

//! The asset trees the engine watches on the application's behalf.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use super::{AssetChangeEvent, AssetWatcher};

/// The asset trees the engine watches for the application, one [`AssetWatcher`] per root.
///
/// Roots are added and removed through `&self`, at any frame. Each subscriber
/// (the script pump, the shader pump, the editor's VFS) reads every root from
/// its own cursor.
#[derive(Default)]
pub struct AssetWatch {
    roots: Mutex<Vec<Watched>>,
}

/// One root, as named and as the file system resolves it.
struct Watched {
    /// As the application named it: what [`AssetWatch::roots`] lists.
    named: PathBuf,
    /// Resolved: what two names of one tree are compared by.
    resolved: PathBuf,
    watcher: AssetWatcher,
}

impl AssetWatch {
    /// Watches `root`, recursively; a root already watched is left as it is.
    ///
    /// # Errors
    /// When the root cannot be watched, or lies inside or holds a root already
    /// watched: two watches over one file would report each save twice.
    pub fn watch(&self, root: &Path) -> Result<()> {
        let resolved = root
            .canonicalize()
            .with_context(|| format!("cannot watch {}", root.display()))?;
        let mut roots = self.lock();
        for watched in roots.iter() {
            if watched.resolved == resolved {
                return Ok(());
            }
            if resolved.starts_with(&watched.resolved) || watched.resolved.starts_with(&resolved) {
                bail!(
                    "{} and {} are nested: one watch would report each change twice — watch one of them",
                    root.display(),
                    watched.named.display()
                );
            }
        }
        // Absolute, as the OS reports the paths it saw: a relative name would
        // match none of them, and every change under it would be dropped.
        let absolute = crate::asset::root_path::absolute(root);
        let watcher = AssetWatcher::new(absolute)?;
        roots.push(Watched {
            named: root.to_path_buf(),
            resolved,
            watcher,
        });
        Ok(())
    }

    /// Stops watching `root`, dropping its unread changes; an unwatched root is no error.
    pub fn unwatch(&self, root: &Path) {
        let resolved = root.canonicalize().ok();
        self.lock().retain(|watched| {
            watched.named != root && resolved.as_ref() != Some(&watched.resolved)
        });
    }

    /// Returns the name `root` is watched by, as first named, for any spelling of that folder.
    ///
    /// The changes under it carry this name, made absolute: keep whatever is kept
    /// per root under it, or one folder ends up with two entries.
    pub fn named(&self, root: &Path) -> Option<PathBuf> {
        let resolved = root.canonicalize().ok();
        self.lock()
            .iter()
            .find(|watched| watched.named == root || resolved.as_ref() == Some(&watched.resolved))
            .map(|watched| watched.named.clone())
    }

    /// Everything `subscriber` has not yet seen, on every watched root.
    pub fn poll_for(&self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        self.lock()
            .iter()
            .flat_map(|watched| watched.watcher.poll_for(subscriber))
            .collect()
    }

    /// Returns the watched roots, as they were named.
    pub fn roots(&self) -> Vec<PathBuf> {
        self.lock()
            .iter()
            .map(|watched| watched.named.clone())
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Watched>> {
        match self.roots.lock() {
            Ok(roots) => roots,
            // A panic while holding it left a valid list behind: the roots
            // are added and removed whole.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
