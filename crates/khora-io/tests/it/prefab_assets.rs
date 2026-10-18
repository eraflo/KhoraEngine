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

//! Prefabs as assets: a project's `.kprefab` files decoded once by its
//! [`AssetService`] and kept — a bullet spawned every frame reads nothing
//! from the disk — until the hot reload invalidates a changed one. And the
//! project's prefab catalog: the identities of exactly its prefabs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::asset::AssetUUID;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::{
    capture_subtree, write_scene_file, CompactEncoding, PrefabSource, SceneRecord,
};
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_io::serialization::{prefab_catalog, AssetPrefabs};
use khora_telemetry::MetricsRegistry;

/// A one-entity prefab named `name`.
fn named_prefab(name: &str) -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new(name)));
    world.mark_authored(root).expect("authored");
    capture_subtree(&world, root).expect("the prefab captures")
}

/// Writes `record` as the scene file at `path`.
fn write_record(path: &Path, record: &SceneRecord) {
    let file = write_scene_file(record, &CompactEncoding).expect("the record writes");
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
    std::fs::write(path, file.to_bytes()).expect("the file is written");
}

/// A project on disk, and its asset service over it.
struct Project {
    _root: tempfile::TempDir,
    assets: PathBuf,
}

impl Project {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary project");
        let assets = root.path().join("assets");
        std::fs::create_dir_all(&assets).expect("the asset folder");
        Self {
            _root: root,
            assets,
        }
    }

    /// Writes `record` at `relative` under the asset folder.
    fn write(&self, relative: &str, record: &SceneRecord) {
        write_record(&self.assets.join(relative), record);
    }

    /// The asset service over the project as it is on disk now.
    fn open(&self) -> AssetService {
        let project = self.assets.parent().expect("the project root");
        AssetService::open_loose_files(
            project,
            &AssetIdRegistry::load(project),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens")
    }
}

/// **A prefab is read from the disk once.** Read again — its file changed,
/// then removed — it is the prefab first read: the service keeps it.
#[test]
fn a_prefab_is_read_from_the_disk_once() {
    let project = Project::new();
    let written = named_prefab("Bullet");
    project.write("prefabs/bullet.kprefab", &written);
    let prefabs = AssetPrefabs::new(Arc::new(Mutex::new(project.open())));
    let bullet = AssetUUID::new_v5("prefabs/bullet.kprefab");

    let first = prefabs.prefab(bullet).expect("the bullet reads");
    assert_eq!(first, written);

    project.write("prefabs/bullet.kprefab", &named_prefab("Rocket"));
    assert_eq!(
        prefabs.prefab(bullet),
        Ok(first.clone()),
        "a changed file was read again"
    );
    std::fs::remove_file(project.assets.join("prefabs/bullet.kprefab")).expect("removed");
    assert_eq!(
        prefabs.prefab(bullet),
        Ok(first),
        "a removed file was read again"
    );
}

/// **Invalidating a prefab drops it**: the service held it, and the next
/// read decodes the file as it is now.
#[test]
fn an_invalidated_prefab_is_read_again() {
    let project = Project::new();
    project.write("prefabs/bullet.kprefab", &named_prefab("Bullet"));
    let assets = Arc::new(Mutex::new(project.open()));
    let prefabs = AssetPrefabs::new(Arc::clone(&assets));
    let bullet = AssetUUID::new_v5("prefabs/bullet.kprefab");
    let first = prefabs.prefab(bullet).expect("the bullet reads");

    let rocket = named_prefab("Rocket");
    project.write("prefabs/bullet.kprefab", &rocket);
    assert!(
        assets.lock().expect("the service").invalidate(&bullet),
        "the service held the prefab it read"
    );

    let again = prefabs.prefab(bullet).expect("the prefab reads again");
    assert_ne!(again, first);
    assert_eq!(again, rocket);
}

/// **An asset that is not a prefab is refused**, and so is an id no file
/// has.
#[test]
fn an_asset_that_is_not_a_prefab_is_refused() {
    let project = Project::new();
    project.write("prefabs/bullet.kprefab", &named_prefab("Bullet"));
    project.write("scenes/level.kscene", &named_prefab("Level"));
    let prefabs = AssetPrefabs::new(Arc::new(Mutex::new(project.open())));

    assert!(prefabs
        .prefab(AssetUUID::new_v5("prefabs/bullet.kprefab"))
        .is_ok());
    assert!(
        prefabs
            .prefab(AssetUUID::new_v5("scenes/level.kscene"))
            .is_err(),
        "a scene was read as a prefab"
    );
    assert!(prefabs.prefab(AssetUUID::new()).is_err());
}

/// **The catalog is exactly the project's prefabs**: every `.kprefab`, and
/// no scene or other asset.
#[test]
fn the_prefab_catalog_lists_exactly_the_prefabs() {
    let project = Project::new();
    project.write("prefabs/bullet.kprefab", &named_prefab("Bullet"));
    project.write("prefabs/enemies/grunt.kprefab", &named_prefab("Grunt"));
    project.write("scenes/level.kscene", &named_prefab("Level"));
    std::fs::create_dir_all(project.assets.join("textures")).expect("the folder");
    std::fs::write(project.assets.join("textures/stone.png"), b"PNG").expect("a texture");

    let catalog = prefab_catalog(&project.open());

    assert_eq!(
        catalog,
        BTreeSet::from([
            AssetUUID::new_v5("prefabs/bullet.kprefab"),
            AssetUUID::new_v5("prefabs/enemies/grunt.kprefab"),
        ])
    );
}

/// A project with no prefab has an empty catalog.
#[test]
fn a_project_without_prefabs_has_an_empty_catalog() {
    let project = Project::new();
    project.write("scenes/level.kscene", &named_prefab("Level"));

    assert!(prefab_catalog(&project.open()).is_empty());
}
