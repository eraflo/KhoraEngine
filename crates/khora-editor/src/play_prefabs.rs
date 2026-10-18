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

//! The prefabs a game in play spawns from: the open project's, read through
//! its VFS and kept decoded by its asset service.
//!
//! Installed in the runtime when the editor starts, before any project is
//! open: the project is followed, not captured.

use std::sync::{Arc, Mutex, PoisonError, TryLockError};

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_data::scene::{PrefabCatalog, PrefabSource, SceneRecord};

use crate::project_vfs::ProjectVfs;

/// The open project's prefabs, or none before one opens.
#[derive(Default)]
pub struct PlayPrefabs(Mutex<Option<Arc<Mutex<ProjectVfs>>>>);

impl PlayPrefabs {
    /// Follows `open`, the project open now — however it was opened — and
    /// keeps `catalog` to its prefabs. Nothing changes while it stays the
    /// one followed.
    pub fn follow(&self, open: Option<&Arc<Mutex<ProjectVfs>>>, catalog: &PrefabCatalog) {
        let mut followed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let same = match (followed.as_ref(), open) {
            (Some(followed), Some(open)) => Arc::ptr_eq(followed, open),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        *followed = open.cloned();
        match open {
            Some(open) => open
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .follow_prefab_catalog(catalog.clone()),
            None => catalog.replace(std::iter::empty()),
        }
    }
}

impl PrefabSource for PlayPrefabs {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        let open = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "no project is open".to_owned())?;
        // Tried, not waited for: the frame boundary must never stall on — or,
        // on its own thread, deadlock against — an editor action holding the
        // project. A spawn refused here is logged, and nothing is made.
        let mut pvfs = match open.try_lock() {
            Ok(pvfs) => pvfs,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return Err("the project is busy: the editor is reading or writing it".to_owned())
            }
        };
        khora_sdk::read_prefab(&mut pvfs.asset_service, id)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use khora_sdk::khora_data::ecs::World;
    use khora_sdk::khora_data::scene::{
        capture_subtree, write_scene_file, CompactEncoding, PrefabCatalog, PrefabSource,
    };
    use khora_sdk::prelude::ecs::*;
    use khora_sdk::MetricsRegistry;

    use super::PlayPrefabs;
    use crate::project_vfs::ProjectVfs;

    /// A project on disk holding one prefab, removed when dropped.
    struct Project {
        root: PathBuf,
        vfs: Arc<Mutex<ProjectVfs>>,
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn project(name: &str) -> Project {
        let mut world = World::new();
        let root = world.spawn((Transform::identity(), Name::new(name)));
        world.mark_authored(root).expect("authored");
        let record = capture_subtree(&world, root).expect("the prefab captures");
        let dir = std::env::temp_dir().join(format!(
            "khora-editor-play-prefabs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let assets = dir.join("assets");
        std::fs::create_dir_all(&assets).expect("the asset folder");
        let file = write_scene_file(&record, &CompactEncoding).expect("the prefab writes");
        std::fs::write(assets.join(format!("{name}.kprefab")), file.to_bytes())
            .expect("the prefab is written");
        let vfs = ProjectVfs::open(dir.clone(), Arc::new(MetricsRegistry::new()))
            .expect("the project opens");
        Project {
            root: dir,
            vfs: Arc::new(Mutex::new(vfs)),
        }
    }

    fn only_prefab(catalog: &PrefabCatalog) -> khora_sdk::khora_core::asset::AssetUUID {
        let held = catalog.snapshot();
        assert_eq!(held.len(), 1, "one prefab: {held:?}");
        *held.iter().next().expect("one")
    }

    #[test]
    fn the_project_opened_next_is_the_one_whose_prefabs_spawn() {
        let (first, second) = (project("Crate"), project("Barrel"));
        let (prefabs, catalog) = (PlayPrefabs::default(), PrefabCatalog::new());

        prefabs.follow(Some(&first.vfs), &catalog);
        let crate_prefab = only_prefab(&catalog);
        assert!(prefabs.prefab(crate_prefab).is_ok());

        prefabs.follow(Some(&second.vfs), &catalog);
        let barrel = only_prefab(&catalog);
        assert_ne!(barrel, crate_prefab, "the catalog is the second project's");
        assert!(prefabs.prefab(barrel).is_ok());
        assert!(
            prefabs.prefab(crate_prefab).is_err(),
            "the first project's is gone"
        );
    }

    /// The same project opened again is a new project to follow: a prefab
    /// written to it meanwhile is one the catalog holds and the game reads,
    /// and the catalog follows the new opening's reindexes.
    #[test]
    fn the_same_project_opened_again_is_followed_anew() {
        let first = project("Chest");
        let (prefabs, catalog) = (PlayPrefabs::default(), PrefabCatalog::new());
        prefabs.follow(Some(&first.vfs), &catalog);
        let chest = only_prefab(&catalog);

        let copy = |to: &str| {
            std::fs::copy(
                first.root.join("assets").join("Chest.kprefab"),
                first.root.join("assets").join(format!("{to}.kprefab")),
            )
            .expect("the prefab is copied");
        };
        copy("Coffer");
        let reopened = Arc::new(Mutex::new(
            ProjectVfs::open(first.root.clone(), Arc::new(MetricsRegistry::new()))
                .expect("the project opens again"),
        ));
        prefabs.follow(Some(&reopened), &catalog);

        let held = catalog.snapshot();
        assert_eq!(held.len(), 2, "the prefab written meanwhile: {held:?}");
        for prefab in held.iter() {
            assert!(prefabs.prefab(*prefab).is_ok(), "{prefab} reads");
        }
        assert!(held.contains(&chest));

        copy("Trunk");
        reopened
            .lock()
            .expect("unpoisoned")
            .rebuild_index()
            .expect("the project reindexes");
        assert_eq!(
            catalog.snapshot().len(),
            3,
            "the reopened project's reindex"
        );
    }

    #[test]
    fn with_no_project_open_nothing_spawns() {
        let first = project("Lamp");
        let (prefabs, catalog) = (PlayPrefabs::default(), PrefabCatalog::new());
        prefabs.follow(Some(&first.vfs), &catalog);
        let lamp = only_prefab(&catalog);

        prefabs.follow(None, &catalog);

        assert!(catalog.snapshot().is_empty());
        assert!(prefabs.prefab(lamp).is_err());
    }
}
