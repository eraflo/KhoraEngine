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

use khora_core::sync::LockExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use super::{AssetChangeEvent, AssetWatcher};
use crate::fs::RootName;

/// The generations every watch of this process draws from: two watches, or
/// two changes of one, never share a number.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// The asset trees the engine watches for the application, one [`AssetWatcher`] per root.
///
/// Roots are added and removed through `&self`, at any frame. Each subscriber
/// (the script pump, the shader pump, the editor's VFS) reads every root from
/// its own cursor.
pub struct AssetWatch {
    // A panic while holding it left a valid list behind: the roots
    // are added and removed whole.
    roots: Mutex<Vec<Watched>>,
    /// Moves each time a root is added or removed.
    generation: AtomicU64,
}

impl Default for AssetWatch {
    fn default() -> Self {
        Self {
            roots: Mutex::new(Vec::new()),
            generation: AtomicU64::new(GENERATIONS.fetch_add(1, Ordering::Relaxed)),
        }
    }
}

/// One root, and the watcher reporting its changes.
struct Watched {
    /// As named, what [`AssetWatch::roots`] lists; resolved, what two names
    /// of one tree are compared by.
    root: RootName,
    watcher: AssetWatcher,
}

impl AssetWatch {
    /// Watches `root`, recursively; a root already watched is left as it is.
    ///
    /// # Errors
    /// When the root cannot be watched, or lies inside or holds a root already
    /// watched: two watches over one file would report each save twice.
    pub fn watch(&self, root: &Path) -> Result<()> {
        let name =
            RootName::resolve(root).with_context(|| format!("cannot watch {}", root.display()))?;
        let mut roots = self.roots.lock_recovered();
        for watched in roots.iter() {
            let resolved = &watched.root.resolved;
            if *resolved == name.resolved {
                return Ok(());
            }
            if name.resolved.starts_with(resolved) || resolved.starts_with(&name.resolved) {
                bail!(
                    "{} and {} are nested: one watch would report each change twice — watch one of them",
                    root.display(),
                    watched.root.named.display()
                );
            }
        }
        // Absolute, as the OS reports the paths it saw: a relative name would
        // match none of them, and every change under it would be dropped.
        let absolute = crate::asset::root_path::absolute(root);
        let watcher = AssetWatcher::new(absolute)?;
        roots.push(Watched {
            root: name,
            watcher,
        });
        self.moved();
        Ok(())
    }

    /// Stops watching `root`, dropping its unread changes; an unwatched root is no error.
    pub fn unwatch(&self, root: &Path) {
        let mut roots = self.roots.lock_recovered();
        let before = roots.len();
        roots.retain(|watched| !watched.root.names(root));
        if roots.len() != before {
            self.moved();
        }
    }

    /// Gives the watch a generation no watch had.
    fn moved(&self) {
        (self.generation).store(
            GENERATIONS.fetch_add(1, Ordering::Relaxed),
            Ordering::Release,
        );
    }

    /// Returns a number that moves each time a root is watched or unwatched.
    ///
    /// A reader compares it to know the roots are the ones it last saw; no
    /// other watch of the process ever had it.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Returns the name `root` is watched by, as first named, for any spelling of that folder.
    ///
    /// The changes under it carry this name, made absolute: keep whatever is kept
    /// per root under it, or one folder ends up with two entries.
    pub fn named(&self, root: &Path) -> Option<PathBuf> {
        self.roots
            .lock_recovered()
            .iter()
            .find(|watched| watched.root.names(root))
            .map(|watched| watched.root.named.clone())
    }

    /// Everything `subscriber` has not yet seen, on every watched root.
    pub fn poll_for(&self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        self.roots
            .lock_recovered()
            .iter()
            .flat_map(|watched| watched.watcher.poll_for(subscriber))
            .collect()
    }

    /// Returns the watched roots, as they were named.
    pub fn roots(&self) -> Vec<PathBuf> {
        self.roots
            .lock_recovered()
            .iter()
            .map(|watched| watched.root.named.clone())
            .collect()
    }
}
