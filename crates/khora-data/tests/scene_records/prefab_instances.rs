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

//! Prefab instances as a scene keeps them — a link, not a copy — and as the
//! tools make them: instantiated, copied, nested.

use std::collections::HashSet;

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::SceneFile;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Name, PrefabInstance, Script, Transform};
use khora_data::scene::{
    collapse, expand, instantiate_prefab, instantiate_subtree, read_scene_file, serialize_subtree,
};

use super::prefab_sample::*;
use super::*;

/// A scene holding a turret instance under a plain level entity, its
/// barrel pushed sideways: the world, the instance root's id, the level.
struct Placed {
    world: World,
    instance: PersistentId,
    level: EntityId,
}

fn placed(turret: &Turret, prefab: AssetUUID, library: &Library) -> Placed {
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
    Placed {
        world,
        instance,
        level,
    }
}

/// Instantiating a prefab links the instance: its root carries the link,
/// takes a fresh authored id of its own — the prefab's root *is* the instance
/// root, not an entity under it — and every other entity of the prefab is
/// there under the id derived from that root, wired as in the prefab.
#[test]
fn instantiating_a_prefab_links_the_instance() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();

    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");

    let instance = id(&world, root);
    assert!(!instance.is_created(), "an instance is authored");
    assert_ne!(instance, turret.root, "the instance root takes a fresh id");
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab })
    );
    assert_eq!(
        name(&world, root),
        "Turret",
        "the prefab's root is the instance root"
    );
    assert_eq!(
        world.entity_with_id(PersistentId::within(instance, turret.root)),
        None,
        "no entity stands for the prefab's root under the instance"
    );
    assert_eq!(
        world.iter_entities().count(),
        3,
        "the prefab's three entities"
    );

    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    let sight = entity(&world, PersistentId::within(instance, turret.sight));
    assert_eq!(name(&world, barrel), "Barrel");
    assert_eq!(parent(&world, barrel), Some(root));
    assert_eq!(parent(&world, sight), Some(root));
    assert_eq!(
        world.get::<Script>(root).expect("scripted").field("aim"),
        Some(&ScriptValue::Entity(sight)),
        "a reference inside the prefab points inside the instance"
    );
    for member in [barrel, sight] {
        assert!(
            world.get::<PrefabInstance>(member).is_none(),
            "only the root carries the link"
        );
    }
}

/// An instance made by `instantiate_prefab` stays linked through a save and
/// a load: an edit of the prefab made in between reaches it.
#[test]
fn an_instantiated_prefab_takes_a_later_prefab_edit() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let sight = entity(prefab_world, turret.sight);
        prefab_world.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));
    let sight = entity(&loaded, PersistentId::within(instance, turret.sight));
    assert_eq!(name(&loaded, sight), "Sight Mk2");
}

/// A scene keeps an instance as a link: one instance record naming the root
/// and the prefab, and no row — in any page — for the root or anything the
/// prefab brings. The level the instance hangs from is an ordinary entity.
#[test]
fn a_scene_keeps_an_instance_as_a_link() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let placed = placed(&turret, prefab, &library);

    let scene = save(&placed.world, &library);

    assert_eq!(scene.instances.len(), 1, "{:?}", scene.instances);
    assert_eq!(scene.instances[0].root, placed.instance);
    assert_eq!(scene.instances[0].prefab, prefab);
    for member in [
        placed.instance,
        PersistentId::within(placed.instance, turret.barrel),
        PersistentId::within(placed.instance, turret.sight),
    ] {
        assert!(
            !has_row(&scene, member),
            "the scene holds a row of {member:?}"
        );
    }
    for member in [turret.barrel, turret.sight] {
        assert!(
            !scene
                .entities
                .contains(&PersistentId::within(placed.instance, member)),
            "the scene lists a member of the instance"
        );
    }
    let level = id(&placed.world, placed.level);
    assert!(scene.entities.contains(&level) && has_row(&scene, level));
}

/// An expanded scene can always be collapsed again — the ids say who belongs
/// to which instance — and collapsing it gives back the very record it was
/// expanded from. Expanded, it links to nothing and holds every member.
#[test]
fn collapsing_an_expanded_scene_gives_it_back() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut placed = placed(&turret, prefab, &library);
    let sight = entity(
        &placed.world,
        PersistentId::within(placed.instance, turret.sight),
    );
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(sight));
    placed.world.mark_authored(muzzle).expect("authored");
    let scene = save(&placed.world, &library);

    let expanded = expand(&scene, &library).expect("the scene expands");
    assert!(expanded.instances.is_empty());
    for member in [turret.barrel, turret.sight] {
        assert!(has_row(
            &expanded,
            PersistentId::within(placed.instance, member)
        ));
    }
    assert!(has_row(&expanded, placed.instance));

    let again = collapse(&expanded, &library).expect("the expanded scene collapses");
    assert_eq!(again, scene, "collapse undoes expand");
}

/// Two instances of one prefab share no id, and an override on one is not
/// an override on the other: after a prefab edit, the one the author
/// changed keeps the change, the other follows the prefab.
#[test]
fn two_instances_of_one_prefab_are_independent() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let first = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let second = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let (first, second) = (id(&world, first), id(&world, second));
    assert_ne!(first, second);
    let ids: HashSet<PersistentId> = world.iter_entities().map(|e| id(&world, e)).collect();
    assert_eq!(
        ids.len(),
        6,
        "two instances, three entities each, no id shared"
    );

    let barrel = entity(&world, PersistentId::within(first, turret.barrel));
    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 3.0;
    let scene = save(&world, &library);
    assert_eq!(scene.instances.len(), 2);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world
            .get_mut::<Transform>(barrel)
            .expect("placed")
            .translation
            .y = 2.0;
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));
    let barrel_of = |instance| entity(&loaded, PersistentId::within(instance, turret.barrel));
    assert_eq!(
        translation(&loaded, barrel_of(first)),
        Vec3::new(3.0, 2.0, 0.0)
    );
    assert_eq!(
        translation(&loaded, barrel_of(second)),
        Vec3::new(0.0, 2.0, 0.0)
    );
}

/// A tower prefab holds a turret instance. Placed in a scene, the turret's
/// entities are derived twice — from the scene's tower to the turret root,
/// from there to the member — and each root links to its own prefab. The
/// scene keeps one link, the tower's; an override it makes deep inside the
/// nested turret survives an edit of the turret prefab, which reaches every
/// field the scene and the tower left alone.
#[test]
fn nested_instances_compose_and_follow_their_prefabs() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    assert_eq!(
        tower.record.instances.len(),
        1,
        "the tower keeps its turret as a link"
    );
    assert_eq!(tower.record.instances[0].prefab, turret_prefab);
    assert_eq!(tower.record.instances[0].root, tower.turret_root);
    let library = library.with(tower_prefab, tower.record.clone());

    let mut world = World::new();
    let root =
        instantiate_prefab(&mut world, tower_prefab, &library).expect("the tower instantiates");
    let instance = id(&world, root);
    let nested = PersistentId::within(instance, tower.turret_root);
    let nested_barrel = PersistentId::within(nested, turret.barrel);
    let turret_root = entity(&world, nested);
    let barrel = entity(&world, nested_barrel);
    assert_eq!(
        world.iter_entities().count(),
        4,
        "the tower and its turret's three"
    );
    assert_eq!(parent(&world, barrel), Some(turret_root));
    assert_eq!(parent(&world, turret_root), Some(root));
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance {
            prefab: tower_prefab
        })
    );
    assert_eq!(
        world.get::<PrefabInstance>(turret_root),
        Some(&PrefabInstance {
            prefab: turret_prefab
        })
    );
    assert_eq!(
        translation(&world, turret_root),
        Vec3::new(0.0, 5.0, 0.0),
        "the tower's override on its turret"
    );

    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 7.0;
    let scene = save(&world, &library);
    assert_eq!(
        scene.instances.len(),
        1,
        "the nested turret is part of the tower's link"
    );
    assert_eq!(scene.instances[0].prefab, tower_prefab);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world
            .get_mut::<Transform>(barrel)
            .expect("placed")
            .translation = Vec3::new(0.0, 2.0, 0.0);
        prefab_world.get_mut::<Name>(barrel).expect("named").0 = "Barrel Mk2".into();
    });
    let library = library.with(turret_prefab, edited_turret);
    let loaded = open(&scene, &library);

    let barrel = entity(&loaded, nested_barrel);
    assert_eq!(translation(&loaded, barrel), Vec3::new(7.0, 2.0, 0.0));
    assert_eq!(name(&loaded, barrel), "Barrel Mk2");
    assert_eq!(
        translation(&loaded, entity(&loaded, nested)),
        Vec3::new(0.0, 5.0, 0.0),
        "the tower's override still holds"
    );
}

/// Copying a subtree that holds an instance — the editor's duplicate, a
/// prefab saved from it — writes the instance as a link and brings it back
/// as an instance of the same prefab: plain entities and the instance root
/// take fresh ids, the members the ids derived from the new root, and the
/// overrides come along.
#[test]
fn a_copied_subtree_keeps_its_instances_linked() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut placed = placed(&turret, prefab, &library);
    let original_root = entity(&placed.world, placed.instance);

    let bytes = serialize_subtree(&placed.world, placed.level, &library).expect("the level writes");
    let file = SceneFile::from_bytes(&bytes).expect("a scene file");
    let written = read_scene_file(&file).expect("the file reads");
    assert_eq!(
        written.instances.len(),
        1,
        "the turret is written as a link"
    );
    assert_eq!(written.instances[0].prefab, prefab);
    assert!(!has_row(
        &written,
        PersistentId::within(placed.instance, turret.barrel)
    ));

    for (copied, root_of) in [(placed.level, true), (original_root, false)] {
        let bytes = serialize_subtree(&placed.world, copied, &library).expect("writes");
        let copy = instantiate_subtree(&mut placed.world, &bytes, &library).expect("instantiates");
        let instance_root = if root_of {
            let level = id(&placed.world, placed.level);
            assert_ne!(id(&placed.world, copy), level, "the copied level is new");
            let children = placed
                .world
                .get::<khora_data::ecs::Children>(copy)
                .expect("the copied level has its turret")
                .0
                .clone();
            assert_eq!(children.len(), 1);
            children[0]
        } else {
            copy
        };
        let copied_instance = id(&placed.world, instance_root);
        assert_ne!(
            copied_instance, placed.instance,
            "the copy is a new instance"
        );
        assert!(!copied_instance.is_created());
        assert_eq!(
            placed.world.get::<PrefabInstance>(instance_root),
            Some(&PrefabInstance { prefab }),
            "the copy is still an instance"
        );
        let barrel = entity(
            &placed.world,
            PersistentId::within(copied_instance, turret.barrel),
        );
        assert_eq!(parent(&placed.world, barrel), Some(instance_root));
        assert_eq!(
            translation(&placed.world, barrel),
            Vec3::new(3.0, 1.0, 0.0),
            "the override comes along"
        );
    }
}
