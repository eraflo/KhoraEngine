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
//!
//! One service, many roots: an application names the roots it wants watched
//! (`watch`) and stops watching them (`unwatch`), through `&self`, at any
//! frame. Every subscriber — the script pump, the shader pump, the editor's
//! VFS — reads every root's changes from its own cursor.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use super::{AssetChangeEvent, AssetWatcher};

/// The asset trees the engine watches for the application.
///
/// One [`AssetWatcher`] per root, each computing paths against its own root
/// exactly as it did alone; a subscriber's cursor lives in each, under its
/// name, so `poll_for` reads every root from where that subscriber left it.
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
    /// Watches `root`, recursively. Watching a root already watched succeeds
    /// and does nothing. Fails when the root cannot be watched, or when it
    /// lies inside — or holds — a root already watched: two watches over one
    /// file would report each save twice, a cost no test would see.
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
        let absolute = std::path::absolute(root)
            .with_context(|| format!("cannot watch {}", root.display()))?;
        let watcher = AssetWatcher::new(absolute)?;
        roots.push(Watched {
            named: root.to_path_buf(),
            resolved,
            watcher,
        });
        Ok(())
    }

    /// Stops watching `root`; what it produced and nobody read goes with it.
    /// A root that is not watched is not an error.
    pub fn unwatch(&self, root: &Path) {
        let resolved = root.canonicalize().ok();
        self.lock().retain(|watched| {
            watched.named != root && resolved.as_ref() != Some(&watched.resolved)
        });
    }

    /// The name the root `root` is watched by — as it was first named — when
    /// `root` is that name or another spelling of the same folder. What the
    /// changes under it carry, made absolute: whatever is kept per root must
    /// be kept under this name, or one folder ends up with two entries.
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

    /// The roots watched now, as they were named.
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
