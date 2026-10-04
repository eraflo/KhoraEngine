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

//! Scenes holding prefab instances through the serialization service: saved
//! as links when the service can read prefabs, expanded when it cannot, and
//! loaded from the prefabs as they are now — game saves included.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_data::ecs::{Name, Parent, PrefabInstance, Transform, World};
use khora_data::scene::{
    apply, capture_subtree, instantiate_prefab, read_save_file, read_scene_file, write_scene_file,
    CompactEncoding, Identity, PrefabSource, SceneRecord,
};
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_io::serialization::{AssetPrefabs, SerializationService};
use khora_telemetry::MetricsRegistry;

/// Prefabs held in memory, by asset id.
#[derive(Debug, Clone, Default)]
struct Library(HashMap<AssetUUID, SceneRecord>);

impl Library {
    fn with(mut self, id: AssetUUID, record: SceneRecord) -> Self {
        self.0.insert(id, record);
        self
    }

    fn service(&self) -> SerializationService {
        SerializationService::with_prefabs(Arc::new(self.clone()))
    }
}

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?} in the library"))
    }
}

/// A turret prefab — a root and its barrel and sight — by its ids.
struct Turret {
    record: SceneRecord,
    root: PersistentId,
    barrel: PersistentId,
    sight: PersistentId,
}

fn turret() -> Turret {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Turret")));
    let barrel = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Barrel"),
    ));
    let sight = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.0, 1.0)),
        Name::new("Sight"),
    ));
    world.set_parent(barrel, Some(root));
    world.set_parent(sight, Some(root));
    for entity in [root, barrel, sight] {
        world.mark_authored(entity).expect("authored");
    }
    Turret {
        record: capture_subtree(&world, root).expect("the turret captures"),
        root: id(&world, root),
        barrel: id(&world, barrel),
        sight: id(&world, sight),
    }
}

/// The turret with its sight renamed — an edit of the prefab.
fn renamed_sight(turret: &Turret) -> SceneRecord {
    let mut world = World::new();
    apply(&mut world, &turret.record, Identity::Keep).expect("the prefab opens");
    let sight = entity(&world, turret.sight);
    world.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
    capture_subtree(&world, entity(&world, turret.root)).expect("captures")
}

/// A level holding a turret instance, its barrel pushed sideways.
struct Level {
    world: World,
    instance: PersistentId,
}

fn level(prefab: AssetUUID, turret: &Turret, library: &Library) -> Level {
    let mut world = World::new();
    let level = world.spawn((Transform::identity(), Name::new("Level")));
    world.mark_authored(level).expect("authored");
    let root = instantiate_prefab(&mut world, prefab, library).expect("the turret instantiates");
    world.set_parent(root, Some(level));
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 3.0;
    Level { world, instance }
}

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

fn entity(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the world is {id:?}"))
}

/// One entity as a test sees it: identity, name, translation, parent's
/// identity, the prefab it is an instance of.
type Observed = (
    PersistentId,
    Option<String>,
    Option<Vec3>,
    Option<PersistentId>,
    Option<AssetUUID>,
);

/// What a test can observe of a world, sorted by identity.
fn observed(world: &World) -> Vec<Observed> {
    let mut seen: Vec<_> = world
        .iter_entities()
        .map(|e| {
            (
                id(world, e),
                world.get::<Name>(e).map(|n| n.as_str().to_owned()),
                world.get::<Transform>(e).map(|t| t.translation),
                world
                    .get::<Parent>(e)
                    .and_then(|p| world.persistent_id(p.0)),
                world.get::<PrefabInstance>(e).map(|link| link.prefab),
            )
        })
        .collect();
    seen.sort_by_key(|(id, ..)| *id);
    seen
}

/// A world holding one entity, to check a refused load leaves it as it was.
fn bystander_world() -> World {
    let mut world = World::new();
    let bystander = world.spawn((Transform::identity(), Name::new("Bystander")));
    world.mark_authored(bystander).expect("authored");
    world
}

const RECORD_GOALS: [SerializationGoal; 3] = [
    SerializationGoal::EditorInterchange,
    SerializationGoal::HumanReadableDebug,
    SerializationGoal::PortableBinary,
];

/// A service that reads prefabs saves an instance as a link — the file
/// holds the link and no member — and loads it back, from its own bytes, as
/// the world that was saved: every member under its derived id, the
/// override, the link on the root.
#[test]
fn a_scene_with_an_instance_round_trips_through_files() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let level = level(prefab, &turret, &library);
    let service = library.service();

    for goal in RECORD_GOALS {
        let file = service
            .save_world(&level.world, goal)
            .expect("the scene saves");
        let file = SceneFile::from_bytes(&file.to_bytes()).expect("its bytes read back");
        let record = read_scene_file(&file).expect("the file reads");
        assert_eq!(
            record.instances.len(),
            1,
            "{goal:?}: the instance is a link"
        );
        assert!(
            !record.pages.iter().any(|page| page
                .rows
                .contains(&PersistentId::within(level.instance, turret.barrel))),
            "{goal:?}: the file holds a member's row"
        );

        let mut dst = World::new();
        service
            .replace_world(&file, &mut dst)
            .unwrap_or_else(|e| panic!("{goal:?}: the scene does not load: {e}"));
        assert_eq!(observed(&dst), observed(&level.world), "{goal:?}");
    }
}

/// A scene saved with an instance takes an edit of its prefab made since:
/// the same file, loaded by a service reading the edited prefab, shows it —
/// and keeps the scene's override.
#[test]
fn a_prefab_edit_reaches_a_saved_scene() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let level = level(prefab, &turret, &library);
    let file = library
        .service()
        .save_world(&level.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    let edited = Library::default().with(prefab, renamed_sight(&turret));
    let mut dst = World::new();
    edited
        .service()
        .replace_world(&file, &mut dst)
        .expect("the scene loads");

    let sight = entity(&dst, PersistentId::within(level.instance, turret.sight));
    assert_eq!(dst.get::<Name>(sight).map(Name::as_str), Some("Sight Mk2"));
    let barrel = entity(&dst, PersistentId::within(level.instance, turret.barrel));
    assert_eq!(
        dst.get::<Transform>(barrel).map(|t| t.translation),
        Some(Vec3::new(3.0, 1.0, 0.0))
    );
}

/// A service with no prefab source cannot collapse an instance: it saves it
/// expanded — every member a row, the link a component of the root — and
/// that file loads anywhere. A service that reads prefabs, saving the world
/// it loaded, collapses the instance again.
#[test]
fn without_prefabs_an_instance_is_saved_expanded_and_collapses_later() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let level = level(prefab, &turret, &library);

    let plain = SerializationService::new();
    let file = plain
        .save_world(&level.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let record = read_scene_file(&file).expect("the file reads");
    assert!(record.instances.is_empty(), "nothing to collapse with");
    for member in [turret.barrel, turret.sight] {
        let member = PersistentId::within(level.instance, member);
        assert!(
            record.pages.iter().any(|page| page.rows.contains(&member)),
            "the expanded scene holds {member:?}"
        );
    }

    let mut dst = World::new();
    plain
        .replace_world(&file, &mut dst)
        .expect("an expanded scene loads anywhere");
    assert_eq!(observed(&dst), observed(&level.world));

    let file = library
        .service()
        .save_world(&dst, SerializationGoal::EditorInterchange)
        .expect("the scene saves again");
    let record = read_scene_file(&file).expect("the file reads");
    assert_eq!(record.instances.len(), 1, "collapsed again");
    assert_eq!(record.instances[0].root, level.instance);
}

/// A scene that links to prefabs cannot be loaded by a service that reads
/// none — nor by one whose prefabs lack the one it names: the load is
/// refused whole and the world is left exactly as it was.
#[test]
fn a_linked_scene_is_refused_without_its_prefab() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let level = level(prefab, &turret, &library);
    let file = library
        .service()
        .save_world(&level.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    for (case, service) in [
        ("no prefab source", SerializationService::new()),
        ("a source without it", Library::default().service()),
    ] {
        let mut world = bystander_world();
        let before = observed(&world);
        assert!(
            service.replace_world(&file, &mut world).is_err(),
            "{case}: replace"
        );
        assert_eq!(
            observed(&world),
            before,
            "{case}: replace changed the world"
        );
        assert!(
            service.load_world(&file, &mut world).is_err(),
            "{case}: load"
        );
        assert_eq!(observed(&world), before, "{case}: load changed the world");
    }
}

/// The fastest load — Play's snapshot — keeps the world expanded, as it
/// is: it loads without any prefab, members and link included.
#[test]
fn a_snapshot_keeps_instances_expanded() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let level = level(prefab, &turret, &library);

    let file = library
        .service()
        .save_world(&level.world, SerializationGoal::FastestLoad)
        .expect("the snapshot saves");
    let mut dst = World::new();
    SerializationService::new()
        .replace_world(&file, &mut dst)
        .expect("a snapshot needs no prefab");
    assert_eq!(observed(&dst), observed(&level.world));
}

/// A game played on a scene holding an instance saves what play changed —
/// against the scene as expanded, so the instance's untouched members are
/// not taken for entities the game made — and loads back with the scene:
/// what play moved stays moved, and an edit of the prefab made since reaches
/// what play left alone.
#[test]
fn a_game_save_over_a_scene_with_instances_loads() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut level = level(prefab, &turret, &library);
    let service = library.service();
    let base_file = service
        .save_world(&level.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let base_id = AssetUUID::new_v5("scenes/level.kscene");

    let barrel = entity(
        &level.world,
        PersistentId::within(level.instance, turret.barrel),
    );
    level
        .world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .z = 9.0;
    let save_file = service
        .save_game(
            &level.world,
            base_id,
            &base_file,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    let save = read_save_file(&save_file).expect("the save reads");
    assert!(
        save.created.is_empty(),
        "the scene's members are not created: {:?}",
        save.created
    );
    assert!(save.destroyed.is_empty(), "{:?}", save.destroyed);
    assert_eq!(
        save.changes.entities,
        vec![PersistentId::within(level.instance, turret.barrel)],
        "only the barrel changed"
    );

    let mut dst = bystander_world();
    service
        .load_game(&mut dst, &save_file, &base_file)
        .expect("the game loads");
    assert_eq!(observed(&dst), observed(&level.world));

    let edited = Library::default().with(prefab, renamed_sight(&turret));
    let mut dst = World::new();
    edited
        .service()
        .load_game(&mut dst, &save_file, &base_file)
        .expect("the game loads over the edited prefab");
    let sight = entity(&dst, PersistentId::within(level.instance, turret.sight));
    assert_eq!(dst.get::<Name>(sight).map(Name::as_str), Some("Sight Mk2"));
    let barrel = entity(&dst, PersistentId::within(level.instance, turret.barrel));
    assert_eq!(
        dst.get::<Transform>(barrel).map(|t| t.translation),
        Some(Vec3::new(3.0, 1.0, 9.0))
    );
}

/// A project's prefabs are its `.kprefab` files, read through its asset
/// service: the prefab a file holds, by the file's asset id; an error for an
/// id no file has. A scene saved and loaded through files with them follows
/// the prefab file as it is on disk when the scene is opened.
#[test]
fn a_projects_prefabs_are_read_through_its_asset_service() {
    let project = tempfile::tempdir().expect("a temporary project");
    let assets = project.path().join("assets");
    std::fs::create_dir_all(assets.join("prefabs")).expect("the prefab folder");
    std::fs::create_dir_all(assets.join("scenes")).expect("the scene folder");
    let turret = turret();
    let write_prefab = |record: &SceneRecord| {
        let file = write_scene_file(record, &CompactEncoding).expect("the prefab writes");
        std::fs::write(
            assets.join("prefabs").join("turret.kprefab"),
            file.to_bytes(),
        )
        .expect("the prefab file is written");
    };
    write_prefab(&turret.record);

    let registry = AssetIdRegistry::load(project.path());
    let assets_service =
        AssetService::open_loose_files(project.path(), &registry, Arc::new(MetricsRegistry::new()))
            .expect("the project opens");
    let prefabs = AssetPrefabs::new(Arc::new(Mutex::new(assets_service)));
    let prefab = AssetUUID::new_v5("prefabs/turret.kprefab");
    assert_eq!(prefabs.prefab(prefab), Ok(turret.record.clone()));
    assert!(
        prefabs.prefab(AssetUUID::new()).is_err(),
        "no file has that id"
    );

    let service = SerializationService::with_prefabs(Arc::new(prefabs));
    let level = level(
        prefab,
        &turret,
        &Library::default().with(prefab, turret.record.clone()),
    );
    let scene_path = assets.join("scenes").join("level.kscene");
    let file = service
        .save_world(&level.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    std::fs::write(&scene_path, file.to_bytes()).expect("the scene file is written");

    let open = || {
        let bytes = std::fs::read(&scene_path).expect("the scene file reads");
        let file = SceneFile::from_bytes(&bytes).expect("a scene file");
        let mut world = World::new();
        service
            .replace_world(&file, &mut world)
            .expect("the scene loads");
        world
    };
    assert_eq!(observed(&open()), observed(&level.world));

    write_prefab(&renamed_sight(&turret));
    let world = open();
    let sight = entity(&world, PersistentId::within(level.instance, turret.sight));
    assert_eq!(
        world.get::<Name>(sight).map(Name::as_str),
        Some("Sight Mk2")
    );
}
