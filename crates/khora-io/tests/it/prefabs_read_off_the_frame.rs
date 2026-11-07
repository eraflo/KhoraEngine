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

//! Prefabs read off the frame: a [`ReaderPrefabs`] reads a project's
//! `.kprefab` files through the asset service's shared reader, on any
//! thread, without the service's lock. A scene's prefab instances expand
//! through it on a worker exactly as they do through the service.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::SerializationGoal;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::{
    capture_subtree, capture_world, collapse, expand, instantiate_prefab, write_scene_file,
    CompactEncoding, PrefabSource, SceneRecord,
};
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_io::serialization::{AssetPrefabs, ReaderPrefabs, SerializationService};
use khora_telemetry::MetricsRegistry;

/// A prefab of a root named `name` and one child.
fn prefab(name: &str) -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new(name)));
    let child = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new(format!("{name} barrel")),
    ));
    assert!(world.set_parent(child, Some(root)));
    for entity in [root, child] {
        world.mark_authored(entity).expect("authored");
    }
    capture_subtree(&world, root).expect("the prefab captures")
}

/// A project on disk.
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

    /// Writes `record` as the scene file at `relative` under the asset
    /// folder.
    fn write(&self, relative: &str, record: &SceneRecord) {
        let path = self.assets.join(relative);
        let file = write_scene_file(record, &CompactEncoding).expect("the record writes");
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
        std::fs::write(path, file.to_bytes()).expect("the file is written");
    }

    /// The asset service over the project as it is on disk now.
    fn open(&self) -> AssetService {
        let project: &Path = self.assets.parent().expect("the project root");
        AssetService::open_loose_files(
            project,
            &AssetIdRegistry::load(project),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens")
    }
}

/// The turret prefab's id.
fn turret() -> AssetUUID {
    AssetUUID::new_v5("prefabs/turret.kprefab")
}

/// A project holding the turret prefab and a level scene, and the turret's
/// record as written.
fn project() -> (Project, SceneRecord) {
    let project = Project::new();
    let turret = prefab("Turret");
    project.write("prefabs/turret.kprefab", &turret);
    project.write("scenes/level.kscene", &prefab("Level"));
    (project, turret)
}

/// A level holding two turret instances, the second's barrel moved, as a
/// record linking to the prefab.
fn level(prefabs: &dyn PrefabSource) -> SceneRecord {
    let mut world = World::new();
    let floor = world.spawn((Transform::identity(), Name::new("Floor")));
    world.mark_authored(floor).expect("authored");
    let first = instantiate_prefab(&mut world, turret(), prefabs).expect("the turret comes in");
    let second = instantiate_prefab(&mut world, turret(), prefabs).expect("the turret comes in");
    world.set_parent(first, Some(floor));
    world
        .get_mut::<Transform>(second)
        .expect("placed")
        .translation
        .x = 4.0;
    let record = collapse(&capture_world(&world).expect("captures"), prefabs)
        .expect("the instances collapse");
    assert_eq!(record.instances.len(), 2, "the level links to its turrets");
    record
}

/// **A prefab reader shares across threads**: it is `Send + Sync`, so it is
/// a serialization service's prefab source, and a load through it works.
#[test]
fn a_prefab_reader_is_a_shared_prefab_source() {
    fn shareable<T: Send + Sync + 'static>() {}
    shareable::<ReaderPrefabs>();

    let (project, _) = project();
    let service = project.open();
    let reader = ReaderPrefabs::new(service.reader());
    let assets = Arc::new(Mutex::new(service));
    let record = level(&AssetPrefabs::new(Arc::clone(&assets)));
    let file = write_scene_file(&record, &CompactEncoding).expect("writes");

    let serialization = SerializationService::with_prefabs(Arc::new(reader));
    let mut world = World::new();
    serialization
        .load_world(&file, &mut world)
        .expect("the level loads through the reader");
    let saved = serialization
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("saves");
    assert!(!saved.payload.is_empty());
}

/// **A prefab read through the shared reader is the one the service
/// gives.**
#[test]
fn a_prefab_reader_reads_the_prefab_the_service_reads() {
    let (project, written) = project();
    let service = project.open();
    let reader = ReaderPrefabs::new(service.reader());
    let through_service = AssetPrefabs::new(Arc::new(Mutex::new(service)));

    let read = reader.prefab(turret()).expect("the turret reads");
    assert_eq!(read, through_service.prefab(turret()).expect("reads"));
    assert_eq!(read, written);
}

/// **A scene's instances expand on a worker as they do on the frame**: the
/// same record, prefab by prefab, as through the service.
#[test]
fn a_prefab_reader_expands_a_scene_on_a_worker_as_the_service_does() {
    let (project, _) = project();
    let service = project.open();
    let reader = ReaderPrefabs::new(service.reader());
    let through_service = AssetPrefabs::new(Arc::new(Mutex::new(service)));
    let record = level(&through_service);
    let expected = expand(&record, &through_service).expect("expands through the service");

    let expanded = std::thread::scope(|scope| {
        scope
            .spawn(|| expand(&record, &reader))
            .join()
            .expect("the worker panicked")
    })
    .expect("expands through the reader");

    assert_eq!(expanded, expected);
}

/// **The reader holds no lock of the service's**: it reads on a worker
/// while the service's lock is held on the frame's thread.
#[test]
fn a_prefab_reader_reads_while_the_service_is_locked() {
    let (project, written) = project();
    let assets = Arc::new(Mutex::new(project.open()));
    let reader = ReaderPrefabs::new(assets.lock().expect("the service").reader());

    let held = assets.lock().expect("the service");
    let read = std::thread::scope(|scope| {
        scope
            .spawn(|| reader.prefab(turret()))
            .join()
            .expect("the worker panicked")
    });
    drop(held);
    assert_eq!(read, Ok(written));
}

/// **The reader keeps no copy**: a prefab whose file changed is read as it
/// is now.
#[test]
fn a_prefab_reader_reads_the_file_as_it_is_now() {
    let (project, written) = project();
    let reader = ReaderPrefabs::new(project.open().reader());
    assert_eq!(reader.prefab(turret()), Ok(written));

    let cannon = prefab("Cannon");
    project.write("prefabs/turret.kprefab", &cannon);

    assert_eq!(reader.prefab(turret()), Ok(cannon));
}

/// **What is not a prefab is refused**: a scene asset, an id no file has.
#[test]
fn a_prefab_reader_refuses_what_is_not_a_prefab() {
    let (project, _) = project();
    let reader = ReaderPrefabs::new(project.open().reader());

    assert!(
        reader
            .prefab(AssetUUID::new_v5("scenes/level.kscene"))
            .is_err(),
        "a scene was read as a prefab"
    );
    assert!(reader.prefab(AssetUUID::new()).is_err(), "an unknown id");
}
