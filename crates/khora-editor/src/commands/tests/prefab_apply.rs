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

//! Applying an instance's overrides to its prefab against a project on
//! disk: the prefab's file is rewritten, the override is gone from the
//! instance that made it, and the prefab's other instances take the value.

use std::path::{Path, PathBuf};

use khora_sdk::editor_ui::{InspectedPrefab, PrefabApplyScope};
use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_data::ecs::{PrefabInstance, World};
use khora_sdk::khora_data::scene::{
    apply as apply_record, capture_subtree, read_scene_file, write_scene_file, CompactEncoding,
    Identity,
};
use khora_sdk::prelude::math::Vec3;
use khora_sdk::{MetricsRegistry, SceneFile};

use super::super::*;
use crate::ops::prefab_overrides::inspect_prefab;
use crate::project_vfs::ProjectPrefabs;

const TURRET: &str = "prefabs/turret.kprefab";
const LEVEL: &str = "scenes/level.kscene";

/// The turret prefab's entities, by their ids in it.
struct TurretIds {
    root: PersistentId,
    barrel: PersistentId,
    sight: PersistentId,
}

/// A project on disk holding a turret prefab — a lit root, a barrel raised
/// one unit, a sight — as `TURRET`, removed when the test ends.
struct Project {
    root: PathBuf,
    vfs: Arc<Mutex<ProjectVfs>>,
    ids: TurretIds,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn project(name: &str) -> Project {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Turret"), Light::point()));
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
    let ids = TurretIds {
        root: world.mark_authored(root).expect("authored"),
        barrel: world.mark_authored(barrel).expect("authored"),
        sight: world.mark_authored(sight).expect("authored"),
    };
    let record = capture_subtree(&world, root).expect("the turret captures");

    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("an old project is removed");
    }
    let prefabs = dir.join("assets").join("prefabs");
    std::fs::create_dir_all(&prefabs).expect("the prefab folder");
    std::fs::create_dir_all(dir.join("assets").join("scenes")).expect("the scene folder");
    let file = write_scene_file(&record, &CompactEncoding).expect("the prefab writes");
    std::fs::write(prefabs.join("turret.kprefab"), file.to_bytes()).expect("the prefab is written");
    let vfs =
        ProjectVfs::open(dir.clone(), Arc::new(MetricsRegistry::new())).expect("the project opens");
    Project {
        root: dir,
        vfs: Arc::new(Mutex::new(vfs)),
        ids,
    }
}

impl Project {
    fn turret_file(&self) -> PathBuf {
        self.root
            .join("assets")
            .join("prefabs")
            .join("turret.kprefab")
    }

    /// The turret prefab as its file holds it now, opened in its own ids.
    fn turret_on_disk(&self) -> World {
        let bytes = std::fs::read(self.turret_file()).expect("the prefab file reads");
        let record = read_scene_file(&SceneFile::from_bytes(&bytes).expect("a scene file"))
            .expect("the prefab reads");
        assert!(record.instances.is_empty(), "{:?}", record.instances);
        let mut world = World::new();
        apply_record(&mut world, &record, Identity::Keep).expect("the prefab opens");
        world
    }

    fn inspect(&self, world: &GameWorld, entity: EntityId) -> InspectedPrefab {
        inspect_prefab(world, entity, &ProjectPrefabs(&self.vfs))
            .unwrap_or_else(|| panic!("{entity:?} belongs to no instance"))
    }
}

/// A turret instance spawned from the project, by its root.
struct Instance {
    root: EntityId,
    id: PersistentId,
}

impl Instance {
    fn member(&self, world: &GameWorld, inner: PersistentId) -> EntityId {
        world
            .inner_world()
            .entity_with_id(PersistentId::within(self.id, inner))
            .expect("the member is known by its derived id")
    }
}

/// Spawns `count` turrets from the project into `world`, as dropping the
/// prefab does.
fn spawn_turrets(project: &Project, world: &mut GameWorld, count: usize) -> Vec<Instance> {
    let state = Arc::new(Mutex::new(EditorState::default()));
    for _ in 0..count {
        state.lock().expect("the editor state").pending_prefab_spawn =
            Some((TURRET.to_owned(), None));
        process_pending_prefab_spawn(Some(&project.vfs), world, &state);
    }
    let instances: Vec<Instance> = world
        .iter_entities()
        .filter(|e| world.get_component::<PrefabInstance>(*e).is_some())
        .map(|root| Instance {
            root,
            id: world
                .inner_world()
                .persistent_id(root)
                .expect("the root has an id"),
        })
        .collect();
    assert_eq!(instances.len(), count, "every turret spawns");
    instances
}

/// Queues `scope` for `entity` and runs the command; the request is drained.
fn apply_to_prefab(
    project: &Project,
    world: &GameWorld,
    entity: EntityId,
    scope: PrefabApplyScope,
) {
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_apply = Some((entity, scope));

    process_pending_prefab_apply(Some(&project.vfs), world, &state);

    assert!(
        state
            .lock()
            .expect("the editor state")
            .pending_prefab_apply
            .is_none(),
        "the request is drained"
    );
}

fn transform_mut(world: &mut GameWorld, entity: EntityId) -> &mut Transform {
    world
        .get_component_mut::<Transform>(entity)
        .expect("placed")
}

fn translation(world: &World, id: PersistentId) -> Vec3 {
    let entity = world.entity_with_id(id).expect("the entity is there");
    world.get::<Transform>(entity).expect("placed").translation
}

fn path(steps: &[&str]) -> Vec<String> {
    steps.iter().map(|step| (*step).to_owned()).collect()
}

/// Applying the barrel's translation rewrites the turret's file with it —
/// and with nothing else: the scale the instance also changed stays its
/// override, and the translation is one no longer.
#[test]
fn applying_a_field_rewrites_the_prefab_file() {
    let project = project("prefab-apply-field");
    let mut world = GameWorld::new();
    let turret = spawn_turrets(&project, &mut world, 1).remove(0);
    let barrel = turret.member(&world, project.ids.barrel);
    transform_mut(&mut world, barrel).translation.x = 5.0;
    transform_mut(&mut world, barrel).scale = Vec3::new(2.0, 2.0, 2.0);

    apply_to_prefab(
        &project,
        &world,
        barrel,
        PrefabApplyScope::Field {
            type_name: "Transform".to_owned(),
            path: path(&["translation"]),
        },
    );

    let prefab = project.turret_on_disk();
    assert_eq!(
        translation(&prefab, project.ids.barrel),
        Vec3::new(5.0, 1.0, 0.0)
    );
    let prefab_barrel = prefab
        .entity_with_id(project.ids.barrel)
        .expect("the barrel is there");
    assert_eq!(
        prefab
            .get::<Transform>(prefab_barrel)
            .expect("placed")
            .scale,
        Vec3::ONE,
        "the scale is not applied"
    );
    let mut fields = project
        .inspect(&world, barrel)
        .fields_of("Transform")
        .to_vec();
    fields.sort();
    assert_eq!(
        fields,
        vec![
            path(&["scale", "x"]),
            path(&["scale", "y"]),
            path(&["scale", "z"])
        ],
        "the translation is no override any more; the scale still is"
    );
}

/// Applying a tag the scene put on the sight writes it into the turret's
/// file; the sight overrides nothing afterwards.
#[test]
fn applying_a_component_rewrites_the_prefab_file() {
    let project = project("prefab-apply-component");
    let mut world = GameWorld::new();
    let turret = spawn_turrets(&project, &mut world, 1).remove(0);
    let sight = turret.member(&world, project.ids.sight);
    world
        .inner_world_mut()
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");

    apply_to_prefab(
        &project,
        &world,
        sight,
        PrefabApplyScope::Component {
            type_name: "Tag".to_owned(),
        },
    );

    let prefab = project.turret_on_disk();
    let prefab_sight = prefab
        .entity_with_id(project.ids.sight)
        .expect("the sight is there");
    assert!(prefab
        .get::<Tag>(prefab_sight)
        .is_some_and(|tag| tag.contains("armed")));
    assert_eq!(project.inspect(&world, sight).override_count(), 0);
}

/// Applying the whole instance from its barrel writes every override of the
/// instance — the root's name, its light taken off, the barrel's push — and
/// leaves the instance overriding nothing.
#[test]
fn applying_the_instance_rewrites_the_prefab_file() {
    let project = project("prefab-apply-instance");
    let mut world = GameWorld::new();
    let turret = spawn_turrets(&project, &mut world, 1).remove(0);
    let barrel = turret.member(&world, project.ids.barrel);
    transform_mut(&mut world, barrel).translation.x = 5.0;
    world
        .get_component_mut::<Name>(turret.root)
        .expect("named")
        .0 = "Gate turret".into();
    world
        .inner_world_mut()
        .remove_component::<Light>(turret.root)
        .expect("the light comes off");

    apply_to_prefab(&project, &world, barrel, PrefabApplyScope::Instance);

    let prefab = project.turret_on_disk();
    let prefab_root = prefab
        .entity_with_id(project.ids.root)
        .expect("the root keeps its id");
    assert_eq!(
        prefab
            .get::<Name>(prefab_root)
            .map(|n| n.as_str().to_owned()),
        Some("Gate turret".to_owned())
    );
    assert!(prefab.get::<Light>(prefab_root).is_none());
    assert_eq!(
        translation(&prefab, project.ids.barrel),
        Vec3::new(5.0, 1.0, 0.0)
    );
    assert_eq!(project.inspect(&world, turret.root).override_count(), 0);
    assert_eq!(project.inspect(&world, barrel).override_count(), 0);
}

/// The other turret of the level, which the author never touched, takes the
/// applied push when the level is opened again through the project.
#[test]
fn another_instance_takes_the_applied_value_when_its_scene_is_opened() {
    let project = project("prefab-apply-propagates");
    let mut world = GameWorld::new();
    let turrets = spawn_turrets(&project, &mut world, 2);
    let barrel = turrets[0].member(&world, project.ids.barrel);
    transform_mut(&mut world, barrel).translation.x = 5.0;
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(
            scene_io::save_scene_in_project(&mut pvfs, &world, Path::new(LEVEL)),
            "the level saves"
        );
    }

    apply_to_prefab(
        &project,
        &world,
        barrel,
        PrefabApplyScope::Field {
            type_name: "Transform".to_owned(),
            path: path(&["translation"]),
        },
    );

    let mut opened = GameWorld::new();
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(
            scene_io::load_scene_in_project(&mut pvfs, &mut opened, LEVEL),
            "the level opens"
        );
    }
    let other = PersistentId::within(turrets[1].id, project.ids.barrel);
    assert_eq!(
        translation(opened.inner_world(), other),
        Vec3::new(5.0, 1.0, 0.0),
        "the untouched turret takes the applied push"
    );
}

/// A request for an entity that belongs to no instance writes no prefab and
/// is still drained.
#[test]
fn applying_from_an_entity_outside_any_instance_writes_nothing() {
    let project = project("prefab-apply-outside");
    let before = std::fs::read(project.turret_file()).expect("the prefab file reads");
    let mut world = GameWorld::new();
    spawn_turrets(&project, &mut world, 1);
    let lone = world.spawn((Transform::identity(), Name::new("Lone")));

    apply_to_prefab(&project, &world, lone, PrefabApplyScope::Instance);

    assert_eq!(
        std::fs::read(project.turret_file()).expect("the prefab file reads"),
        before,
        "the turret's file is untouched"
    );
}

/// Queues `scope` for `entity`, runs the command, and applies the edits it
/// leaves for the next frame — the other instances' rebase.
fn apply_and_rebase(
    project: &Project,
    world: &mut GameWorld,
    entity: EntityId,
    scope: PrefabApplyScope,
) {
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_apply = Some((entity, scope));

    process_pending_prefab_apply(Some(&project.vfs), world, &state);

    let mut state = state.lock().expect("the editor state");
    crate::ops::apply_edits(world, &mut state);
}

/// The other turret in the editor takes the applied push at once, not on
/// its next load — and keeps the scale it overrides itself.
#[test]
fn another_live_instance_takes_the_applied_value_and_keeps_its_own_overrides() {
    let project = project("prefab-apply-rebase-value");
    let mut world = GameWorld::new();
    let turrets = spawn_turrets(&project, &mut world, 2);
    let barrel = turrets[0].member(&world, project.ids.barrel);
    let other = turrets[1].member(&world, project.ids.barrel);
    transform_mut(&mut world, barrel).translation.x = 5.0;
    transform_mut(&mut world, other).scale = Vec3::new(3.0, 3.0, 3.0);

    apply_and_rebase(
        &project,
        &mut world,
        barrel,
        PrefabApplyScope::Field {
            type_name: "Transform".to_owned(),
            path: path(&["translation"]),
        },
    );

    let other_transform = *world.get_component::<Transform>(other).expect("placed");
    assert_eq!(other_transform.translation, Vec3::new(5.0, 1.0, 0.0));
    assert_eq!(other_transform.scale, Vec3::new(3.0, 3.0, 3.0));
    let mut fields = project
        .inspect(&world, other)
        .fields_of("Transform")
        .to_vec();
    fields.sort();
    assert_eq!(
        fields,
        vec![
            path(&["scale", "x"]),
            path(&["scale", "y"]),
            path(&["scale", "z"])
        ]
    );
}

/// A component applied to the prefab reaches the other turret at once: its
/// sight, which never removed the tag, does not come to override the prefab
/// by lacking it.
#[test]
fn another_live_instance_takes_an_applied_component() {
    let project = project("prefab-apply-rebase-added");
    let mut world = GameWorld::new();
    let turrets = spawn_turrets(&project, &mut world, 2);
    let sight = turrets[0].member(&world, project.ids.sight);
    let other = turrets[1].member(&world, project.ids.sight);
    world
        .inner_world_mut()
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");

    apply_and_rebase(
        &project,
        &mut world,
        sight,
        PrefabApplyScope::Component {
            type_name: "Tag".to_owned(),
        },
    );

    let inspected = project.inspect(&world, other);
    assert!(
        inspected.removed.is_empty(),
        "the other sight overrides the prefab by lacking the tag: {:?}",
        inspected.removed
    );
    assert_eq!(inspected.override_count(), 0);
}

/// A component the applied instance took off leaves the other turret at
/// once: its root, which never added the light, does not come to override
/// the prefab by keeping it.
#[test]
fn another_live_instance_loses_a_component_the_prefab_no_longer_has() {
    let project = project("prefab-apply-rebase-removed");
    let mut world = GameWorld::new();
    let turrets = spawn_turrets(&project, &mut world, 2);
    world
        .inner_world_mut()
        .remove_component::<Light>(turrets[0].root)
        .expect("the light comes off");

    apply_and_rebase(
        &project,
        &mut world,
        turrets[0].root,
        PrefabApplyScope::Instance,
    );

    let inspected = project.inspect(&world, turrets[1].root);
    assert!(
        inspected.components.is_empty(),
        "the other root overrides the prefab by keeping the light: {:?}",
        inspected.components
    );
    assert_eq!(inspected.override_count(), 0);
}

/// A turret nested in a tower is an instance of the turret like any other:
/// a component applied from a turret of the scene reaches the tower's turret
/// at once.
#[test]
fn a_nested_live_instance_takes_an_applied_component() {
    let project = project("prefab-apply-rebase-nested");
    let mut made = GameWorld::new();
    let tower = made.spawn((Transform::identity(), Name::new("Tower")));
    made.inner_world_mut()
        .mark_authored(tower)
        .expect("authored");
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn =
        Some((TURRET.to_owned(), Some(tower)));
    process_pending_prefab_spawn(Some(&project.vfs), &mut made, &state);
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_prefab_at = Some((tower, "prefabs/tower.kprefab".to_owned()));
    process_pending_save_as_prefab(Some(&project.vfs), &made, &state);

    let mut world = GameWorld::new();
    let turret = spawn_turrets(&project, &mut world, 1).remove(0);
    state.lock().expect("the editor state").pending_prefab_spawn =
        Some(("prefabs/tower.kprefab".to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
    let nested = world
        .iter_entities()
        .find(|e| {
            world.get_component::<PrefabInstance>(*e).is_some()
                && world.get_component::<Parent>(*e).is_some()
        })
        .expect("the tower's turret");
    let nested = Instance {
        root: nested,
        id: world
            .inner_world()
            .persistent_id(nested)
            .expect("the nested root has an id"),
    };
    let sight = turret.member(&world, project.ids.sight);
    let other = nested.member(&world, project.ids.sight);
    world
        .inner_world_mut()
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");

    apply_and_rebase(
        &project,
        &mut world,
        sight,
        PrefabApplyScope::Component {
            type_name: "Tag".to_owned(),
        },
    );

    assert!(
        world
            .get_component::<Tag>(other)
            .is_some_and(|tag| tag.contains("armed")),
        "the tower's turret sight takes the tag"
    );
}
