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

//! The modules each mounted root brought in.
//!
//! Kept beside the hot-reload pump rather than with the mount alone: a folder
//! moved out of a script folder, or deleted, reports only itself, and the
//! modules it held are then known only from what was recorded here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::script::hot_reload::module_names;
use crate::script::layout::script_dir;

/// The modules each mounted root brought in, by root: what an unmount takes
/// back, and what a folder gone from a script folder held. And the roots
/// unmounted since, until mounted again: a compile that finishes for one of
/// them brings none of its modules back.
#[derive(Debug, Default)]
pub struct MountedScripts(Mutex<Records>);

#[derive(Debug, Default)]
struct Records {
    by_root: BTreeMap<PathBuf, BTreeSet<String>>,
    closed: BTreeSet<PathBuf>,
}

impl MountedScripts {
    /// Records `modules` as brought in by `root`.
    pub fn record(&self, root: &Path, modules: impl IntoIterator<Item = String>) {
        let mut records = self.lock();
        let root = key(root);
        records.closed.remove(&root);
        records.by_root.entry(root).or_default().extend(modules);
    }

    /// Forgets `root`, returning the modules it had brought in. The root is
    /// closed until it is recorded again.
    pub fn forget(&self, root: &Path) -> BTreeSet<String> {
        let mut records = self.lock();
        let root = key(root);
        let modules = records.by_root.remove(&root).unwrap_or_default();
        records.closed.insert(root);
        modules
    }

    /// Records one module `root` now brings in: one the pump compiled. Not
    /// into a root closed since: a compile finishing after its root was
    /// unmounted must not bring it back.
    pub fn record_module(&self, root: &Path, module: &str) {
        let mut records = self.lock();
        let root = key(root);
        if records.closed.contains(&root) {
            return;
        }
        records
            .by_root
            .entry(root)
            .or_default()
            .insert(module.to_owned());
    }

    /// Whether `root` was unmounted and not mounted again since.
    pub fn is_closed(&self, root: &Path) -> bool {
        self.lock().closed.contains(&key(root))
    }

    /// Forgets one module of `root`: one the pump withdrew.
    pub fn forget_module(&self, root: &Path, module: &str) {
        if let Some(recorded) = self.lock().by_root.get_mut(&key(root)) {
            recorded.remove(module);
        }
    }

    /// Every root with a record, as the records key them: made absolute.
    pub fn roots(&self) -> Vec<PathBuf> {
        self.lock().by_root.keys().cloned().collect()
    }

    /// The modules recorded for `root`.
    pub fn modules_of(&self, root: &Path) -> BTreeSet<String> {
        self.lock()
            .by_root
            .get(&key(root))
            .cloned()
            .unwrap_or_default()
    }

    /// The modules the roots still mounted bring in — recorded at their mount,
    /// and on disk now.
    pub fn others(&self) -> BTreeSet<String> {
        self.lock()
            .by_root
            .iter()
            .flat_map(|(root, recorded)| {
                recorded.iter().cloned().chain(
                    script_dir(root)
                        .map(|dir| module_names(&dir))
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Records> {
        match self.0.lock() {
            Ok(mounted) => mounted,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// `root` as the records key it: made absolute, as the watch reports the root
/// its changes happened under — so a mount naming the root relatively and the
/// pump hearing of a change under it read and write one record.
fn key(root: &Path) -> PathBuf {
    crate::asset::root_path::absolute(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_recorded_one_by_one_is_listed_and_forgotten() {
        let mounted = MountedScripts::default();
        let root = Path::new("project-a/assets");
        mounted.record(root, ["main.erg".to_owned()]);
        mounted.record_module(root, "ai/guard.erg");

        assert_eq!(
            mounted.modules_of(root),
            BTreeSet::from(["ai/guard.erg".to_owned(), "main.erg".to_owned()])
        );

        mounted.forget_module(root, "ai/guard.erg");
        assert_eq!(
            mounted.modules_of(root),
            BTreeSet::from(["main.erg".to_owned()])
        );
    }

    /// **One root, one record, however it is named**: the mount may name it
    /// relatively, the pump hears of it absolutely.
    #[test]
    fn a_root_named_relatively_and_absolutely_is_one_record() {
        let mounted = MountedScripts::default();
        let relative = Path::new("project-b/assets");
        let absolute = std::path::absolute(relative).expect("the current directory");
        mounted.record(relative, ["main.erg".to_owned()]);

        assert_eq!(
            mounted.modules_of(&absolute),
            BTreeSet::from(["main.erg".to_owned()])
        );
        assert_eq!(mounted.forget(&absolute).len(), 1);
        assert!(mounted.modules_of(relative).is_empty());
    }

    #[test]
    fn every_root_with_a_record_is_listed_absolute() {
        let mounted = MountedScripts::default();
        mounted.record(Path::new("project-c/assets"), ["main.erg".to_owned()]);
        mounted.record_module(Path::new("project-d/assets"), "x.erg");

        let roots = mounted.roots();
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().all(|root| root.is_absolute()), "{roots:?}");
    }

    #[test]
    fn forgetting_a_module_of_an_unknown_root_does_nothing() {
        let mounted = MountedScripts::default();
        mounted.forget_module(Path::new("nowhere"), "x.erg");
        assert!(mounted.modules_of(Path::new("nowhere")).is_empty());
    }

    /// **A root unmounted is closed until it is mounted again**, and a
    /// module recorded into a closed root does not bring it back: a compile
    /// finishing after its root closed leaves no trace of it. A root never
    /// mounted is not closed.
    #[test]
    fn a_module_recorded_after_its_root_is_forgotten_does_not_bring_it_back() {
        let mounted = MountedScripts::default();
        let root = Path::new("project");

        assert!(!mounted.is_closed(root), "never mounted is not closed");
        mounted.record(root, ["a.erg".to_owned()]);
        mounted.record_module(root, "b.erg");
        assert_eq!(
            mounted.modules_of(root),
            BTreeSet::from(["a.erg".to_owned(), "b.erg".to_owned()])
        );

        mounted.forget(root);
        mounted.record_module(root, "c.erg");
        assert!(mounted.is_closed(root));
        assert!(mounted.modules_of(root).is_empty(), "not brought back");

        mounted.record(root, ["a.erg".to_owned()]);
        assert!(!mounted.is_closed(root), "mounted again");
    }
}
