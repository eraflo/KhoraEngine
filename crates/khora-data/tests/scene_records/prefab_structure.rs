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

//! The shape an author gives a prefab instance in a scene — where its
//! entities hang, in which order, what the scene's own entities do around
//! them — as a save and a load keep it.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::SceneFile;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Name, PrefabInstance, Script, Transform};
use khora_data::scene::{
    capture_subtree, collapse, expand, instantiate_prefab, instantiate_subtree, read_scene_file,
    serialize_subtree, write_scene_file,
};

use super::prefab_sample::*;
use super::*;

/// The names of `entity`'s children, in their `Children` order.
fn child_names(world: &World, entity: EntityId) -> Vec<String> {
    world
        .get::<Children>(entity)
        .map(|children| children.0.iter().map(|child| name(world, *child)).collect())
        .unwrap_or_default()
}

/// The author hangs a muzzle of their own under an instance root, then moves
/// the barrel behind it: the root's children read sight, muzzle, barrel.
/// Saved and opened again, the root's children keep that order — sibling
/// order is the scene's, members and the author's additions alike.
#[test]
fn sibling_order_between_members_and_added_children_survives_a_save() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let muzzle = world.spawn((Transform::identity(), Name::new("Muzzle")));
    world.set_parent(muzzle, Some(root));
    world.mark_authored(muzzle).expect("authored");
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    assert!(
        world.set_parent(barrel, Some(root)),
        "the barrel moves last"
    );
    let before = child_names(&world, root);
    assert_eq!(before, ["Sight", "Muzzle", "Barrel"]);

    let loaded = open(&save(&world, &library), &library);

    assert_eq!(child_names(&loaded, entity(&loaded, instance)), before);
}

/// The author moves the sight out of its instance, under the level the
/// instance hangs from, after a lamp of their own: the level's children read
/// turret, lamp, sight. Saved and opened again, they keep that order.
#[test]
fn a_member_moved_out_of_its_instance_keeps_its_place_among_siblings() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let level = world.spawn((Transform::identity(), Name::new("Level")));
    let level = {
        let id = world.mark_authored(level).expect("authored");
        entity(&world, id)
    };
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    world.set_parent(root, Some(level));
    let instance = id(&world, root);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    world.set_parent(lamp, Some(level));
    world.mark_authored(lamp).expect("authored");
    let sight = entity(&world, PersistentId::within(instance, turret.sight));
    assert!(world.set_parent(sight, Some(level)), "the sight moves out");
    let before = child_names(&world, level);
    assert_eq!(before, ["Turret", "Lamp", "Sight"]);
    let level_id = id(&world, level);

    let loaded = open(&save(&world, &library), &library);

    assert_eq!(child_names(&loaded, entity(&loaded, level_id)), before);
    let sight = entity(&loaded, PersistentId::within(instance, turret.sight));
    assert_eq!(parent(&loaded, sight), Some(entity(&loaded, level_id)));
}

/// A prefab variant — a prefab saved from an instance of another, its root
/// itself an instance — is instantiated as an instance of the variant: the
/// variant's own change on top of the base prefab, the base's later edits
/// reaching what neither changed, and a scene holding it collapsing back to
/// itself.
#[test]
fn a_prefab_variant_follows_its_base_prefab() {
    let turret = turret();
    let (base, variant) = (AssetUUID::new(), AssetUUID::new());
    let library = Library::default().with(base, turret.record.clone());
    let mut author = World::new();
    let made = instantiate_prefab(&mut author, base, &library).expect("the turret instantiates");
    let sight = entity(
        &author,
        PersistentId::within(id(&author, made), turret.sight),
    );
    author.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
    let variant_record = collapse(
        &capture_subtree(&author, made).expect("the variant captures"),
        &library,
    )
    .expect("the variant collapses");
    let library = library.with(variant, variant_record);

    let mut world = World::new();
    let root = instantiate_prefab(&mut world, variant, &library).expect("the variant instantiates");
    let instance = id(&world, root);
    assert_eq!(world.iter_entities().count(), 3);
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab: variant })
    );
    let sight = entity(&world, PersistentId::within(instance, turret.sight));
    assert_eq!(name(&world, sight), "Sight Mk2", "the variant's change");
    let scene = save(&world, &library);
    assert_eq!(scene.instances.len(), 1, "{:?}", scene.instances);
    assert_eq!(scene.instances[0].prefab, variant);
    let again = collapse(&expand(&scene, &library).expect("expands"), &library).expect("collapses");
    assert_eq!(again, scene, "collapse undoes expand");

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.get_mut::<Name>(barrel).expect("named").0 = "Barrel Mk2".into();
    });
    let loaded = open(&scene, &library.clone().with(base, edited_turret));
    let barrel = entity(&loaded, PersistentId::within(instance, turret.barrel));
    let sight = entity(&loaded, PersistentId::within(instance, turret.sight));
    assert_eq!(name(&loaded, barrel), "Barrel Mk2", "the base's edit");
    assert_eq!(name(&loaded, sight), "Sight Mk2", "the variant's change");
}

/// Duplicating the turret nested in a tower instance copies it as a turret
/// instance of its own, with what the tower and the scene changed on it.
#[test]
fn a_nested_instance_duplicates_as_an_instance_with_its_overrides() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, tower_prefab, &library).expect("instantiates");
    let nested = PersistentId::within(id(&world, root), tower.turret_root);
    let barrel = entity(&world, PersistentId::within(nested, turret.barrel));
    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 7.0;

    let bytes = serialize_subtree(&world, entity(&world, nested), &library).expect("writes");
    let copy = instantiate_subtree(&mut world, &bytes, &library).expect("instantiates");

    let copied = id(&world, copy);
    assert_eq!(
        world.get::<PrefabInstance>(copy),
        Some(&PrefabInstance {
            prefab: turret_prefab
        })
    );
    assert_eq!(translation(&world, copy), Vec3::new(0.0, 5.0, 0.0));
    let barrel = entity(&world, PersistentId::within(copied, turret.barrel));
    assert_eq!(translation(&world, barrel), Vec3::new(7.0, 1.0, 0.0));
    assert_eq!(parent(&world, barrel), Some(copy));
    assert_eq!(world.iter_entities().count(), 4 + 3);
}

/// An instance's override that points out of it — the turret aiming at a
/// guard of the scene — is kept by the scene, through its file, and after a
/// prefab edit still finds the guard.
#[test]
fn a_reference_from_an_instance_to_the_scene_survives_a_file_and_a_prefab_edit() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let guard = world.spawn((Transform::identity(), Name::new("Guard")));
    let guard_id = world.mark_authored(guard).expect("authored");
    let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let instance = id(&world, root);
    world.get_mut::<Script>(root).expect("scripted").fields =
        Script::new("ai/turret.erg", "Turret")
            .with_field("aim", ScriptValue::Entity(entity(&world, guard_id)))
            .fields;
    let scene = save(&world, &library);
    for encoding in [
        &CompactEncoding as &dyn SceneEncoding,
        &TextEncoding,
        &MsgPackEncoding,
    ] {
        let file = write_scene_file(&scene, encoding).expect("writes");
        let file = SceneFile::from_bytes(&file.to_bytes()).expect("parses");
        let read = read_scene_file(&file).expect("reads");

        let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
            let sight = entity(prefab_world, turret.sight);
            prefab_world.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
        });
        let loaded = open(&read, &Library::default().with(prefab, edited_turret));
        let root = entity(&loaded, instance);
        assert_eq!(
            loaded.get::<Script>(root).expect("scripted").field("aim"),
            Some(&ScriptValue::Entity(entity(&loaded, guard_id))),
            "{}",
            encoding.id()
        );
    }
}

/// The author hangs a muzzle under the barrel and has a guard watch the
/// barrel; the prefab's author then deletes the barrel. The scene still
/// opens: the muzzle and the guard are the scene's own.
#[test]
fn a_scene_opens_after_its_prefab_loses_what_the_scene_hangs_from() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    let muzzle = world.spawn((Transform::identity(), Name::new("Muzzle")));
    world.set_parent(muzzle, Some(barrel));
    let muzzle = world.mark_authored(muzzle).expect("authored");
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Guard"),
        Script::new("ai/guard.erg", "Guard").with_field("watch", ScriptValue::Entity(barrel)),
    ));
    let guard = world.mark_authored(guard).expect("authored");
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.despawn_subtree(barrel);
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));
    assert_eq!(name(&loaded, entity(&loaded, muzzle)), "Muzzle");
    assert_eq!(name(&loaded, entity(&loaded, guard)), "Guard");
}

/// The author deletes the barrel from an instance; the prefab's author then
/// hangs a muzzle under the barrel. The barrel's subtree stays deleted in the
/// instance — the muzzle does not turn up loose, hanging from nothing.
#[test]
fn a_part_the_prefab_adds_under_a_deleted_member_stays_out() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    world.despawn_subtree(barrel);
    let scene = save(&world, &library);

    let (edited_turret, muzzle) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        let muzzle = prefab_world.spawn((Transform::identity(), Name::new("Muzzle")));
        prefab_world.set_parent(muzzle, Some(barrel));
        prefab_world.mark_authored(muzzle).expect("authored")
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    assert_eq!(
        loaded.entity_with_id(PersistentId::within(instance, muzzle)),
        None,
        "the muzzle hangs from the deleted barrel"
    );
    assert_eq!(loaded.iter_entities().count(), 2, "the root and the sight");
}

/// The prefab's author moves the barrel behind the sight. An instance the
/// scene never reordered follows: its root's children read sight, barrel —
/// the prefab's order, as every other part of the prefab the scene left
/// alone.
#[test]
fn an_instance_takes_the_sibling_order_of_its_prefab_where_it_kept_it() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let instance = id(&world, root);
    assert_eq!(child_names(&world, root), ["Barrel", "Sight"]);
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let root = entity(prefab_world, turret.root);
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.set_parent(barrel, Some(root));
    });
    let loaded = open(
        &scene,
        &Library::default().with(prefab, edited_turret.clone()),
    );

    let mut fresh = World::new();
    let fresh_root = instantiate_prefab(
        &mut fresh,
        prefab,
        &Library::default().with(prefab, edited_turret),
    )
    .expect("instantiates");
    assert_eq!(child_names(&fresh, fresh_root), ["Sight", "Barrel"]);
    assert_eq!(
        child_names(&loaded, entity(&loaded, instance)),
        ["Sight", "Barrel"]
    );
}

/// Two instances under one level, each with its sight moved out under the
/// level, after a lamp of the author's: the level's children read turret,
/// turret, sight of the second, lamp, sight of the first. Saved and opened
/// again, both instances' moved members and the lamp keep those places.
#[test]
fn members_of_two_instances_moved_under_one_parent_keep_their_order() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let level = world.spawn((Transform::identity(), Name::new("Level")));
    let level_id = world.mark_authored(level).expect("authored");
    let level = entity(&world, level_id);
    let mut instances = Vec::new();
    for _ in 0..2 {
        let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
        world.set_parent(root, Some(level));
        instances.push(id(&world, root));
    }
    let sight_of =
        |world: &World, instance| entity(world, PersistentId::within(instance, turret.sight));
    let (first, second) = (
        sight_of(&world, instances[0]),
        sight_of(&world, instances[1]),
    );
    world.get_mut::<Name>(first).expect("named").0 = "First sight".into();
    world.get_mut::<Name>(second).expect("named").0 = "Second sight".into();
    assert!(world.set_parent(second, Some(level)));
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    world.set_parent(lamp, Some(level));
    world.mark_authored(lamp).expect("authored");
    assert!(world.set_parent(first, Some(level)));
    let before = child_names(&world, level);
    assert_eq!(
        before,
        ["Turret", "Turret", "Second sight", "Lamp", "First sight"]
    );

    let loaded = open(&save(&world, &library), &library);

    assert_eq!(child_names(&loaded, entity(&loaded, level_id)), before);
}

/// The sight of one instance moved under the barrel of another, beside a
/// muzzle the author hung there, then put first: the barrel's children read
/// sight, muzzle. Saved and opened again, they still do.
#[test]
fn a_parents_children_spanning_two_instances_keep_their_order() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let a = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let b = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let (a, b) = (id(&world, a), id(&world, b));
    let barrel = entity(&world, PersistentId::within(b, turret.barrel));
    let sight = entity(&world, PersistentId::within(a, turret.sight));
    let muzzle = world.spawn((Transform::identity(), Name::new("Muzzle")));
    world.set_parent(muzzle, Some(barrel));
    world.mark_authored(muzzle).expect("authored");
    assert!(world.set_parent(sight, Some(barrel)));
    assert!(
        world.set_parent(muzzle, Some(barrel)),
        "the muzzle moves last"
    );
    let before = child_names(&world, barrel);
    assert_eq!(before, ["Sight", "Muzzle"]);

    let loaded = open(&save(&world, &library), &library);
    let barrel = entity(&loaded, PersistentId::within(b, turret.barrel));
    assert_eq!(child_names(&loaded, barrel), before);
}

/// The author reordered an instance root's children around a muzzle of
/// their own; the prefab's author then deletes the barrel. The instance
/// opens with the barrel gone and the rest in the author's order.
#[test]
fn a_sibling_order_naming_a_member_the_prefab_lost_still_opens() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    let instance = id(&world, root);
    let muzzle = world.spawn((Transform::identity(), Name::new("Muzzle")));
    world.set_parent(muzzle, Some(root));
    world.mark_authored(muzzle).expect("authored");
    let sight = entity(&world, PersistentId::within(instance, turret.sight));
    assert!(world.set_parent(sight, Some(root)), "the sight moves last");
    assert_eq!(child_names(&world, root), ["Barrel", "Muzzle", "Sight"]);
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.despawn_subtree(barrel);
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));
    assert_eq!(
        child_names(&loaded, entity(&loaded, instance)),
        ["Muzzle", "Sight"]
    );
}
