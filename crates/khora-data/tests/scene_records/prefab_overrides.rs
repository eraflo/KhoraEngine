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

//! A prefab instance in a scene: what the prefab says, except where the
//! scene's author changed it — across an edit of the prefab made since.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Light, Name, PrefabInstance, Script, Tag, Transform};
use khora_data::scene::instantiate_prefab;
use khora_data::scene::record::EntityRef;

use super::prefab_sample::*;
use super::subtree::references_in;
use super::*;

/// **The point of a link.** A scene holds an instance it never changed; the
/// prefab's author then moves the barrel, renames the turret and switches
/// its light off. Opening the scene, the instance shows every one of those
/// edits — it is the prefab, not a copy of it made at the time.
#[test]
fn an_instance_follows_its_prefab_where_not_overridden() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world
            .get_mut::<Transform>(barrel)
            .expect("placed")
            .translation = Vec3::new(0.0, 2.0, 0.0);
        let root = entity(prefab_world, turret.root);
        prefab_world.get_mut::<Name>(root).expect("named").0 = "Turret Mk2".into();
        prefab_world.get_mut::<Light>(root).expect("lit").enabled = false;
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    let root = entity(&loaded, instance);
    let barrel = entity(&loaded, PersistentId::within(instance, turret.barrel));
    assert_eq!(translation(&loaded, barrel), Vec3::new(0.0, 2.0, 0.0));
    assert_eq!(name(&loaded, root), "Turret Mk2");
    assert!(!loaded.get::<Light>(root).expect("lit").enabled);
    assert_eq!(
        loaded.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab }),
        "the root still links to its prefab"
    );
}

/// The prefab gains a scope and loses its barrel after the scene was saved:
/// the instance gains and loses them too — under the ids the instance
/// derives for them.
#[test]
fn parts_the_prefab_gains_or_loses_reach_the_instance() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let scene = save(&world, &library);

    let (edited_turret, scope) = edited(&turret.record, |prefab_world| {
        let root = entity(prefab_world, turret.root);
        let scope = prefab_world.spawn((Transform::identity(), Name::new("Scope")));
        prefab_world.set_parent(scope, Some(root));
        let scope = prefab_world.mark_authored(scope).expect("authored");
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.despawn_subtree(barrel);
        scope
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    let root = entity(&loaded, instance);
    let scope = entity(&loaded, PersistentId::within(instance, scope));
    assert_eq!(name(&loaded, scope), "Scope");
    assert_eq!(parent(&loaded, scope), Some(root));
    assert_eq!(
        loaded.entity_with_id(PersistentId::within(instance, turret.barrel)),
        None,
        "the barrel the prefab lost is gone from the instance"
    );
    assert_eq!(loaded.iter_entities().count(), 3, "root, sight and scope");
}

/// The scene's author placed the instance, renamed it and pushed its barrel
/// sideways; the prefab's author then raises the barrel, scales the turret,
/// renames it and switches its light off. Each field the scene changed keeps
/// the scene's value; each it did not takes the prefab's new one — field by
/// field, so the barrel ends up both pushed and raised.
#[test]
fn an_override_survives_a_prefab_change() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 5.0;
    world
        .get_mut::<Transform>(root)
        .expect("placed")
        .translation = Vec3::new(10.0, 0.0, 0.0);
    world.get_mut::<Name>(root).expect("named").0 = "Gate turret".into();
    let scene = save(&world, &library);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world
            .get_mut::<Transform>(barrel)
            .expect("placed")
            .translation = Vec3::new(0.0, 2.0, 0.0);
        let root = entity(prefab_world, turret.root);
        prefab_world
            .get_mut::<Transform>(root)
            .expect("placed")
            .scale = Vec3::new(2.0, 2.0, 2.0);
        prefab_world.get_mut::<Name>(root).expect("named").0 = "Turret Mk2".into();
        prefab_world.get_mut::<Light>(root).expect("lit").enabled = false;
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    let root = entity(&loaded, instance);
    let barrel = entity(&loaded, PersistentId::within(instance, turret.barrel));
    assert_eq!(
        translation(&loaded, barrel),
        Vec3::new(5.0, 2.0, 0.0),
        "the scene's x, the prefab's new y"
    );
    let placed = loaded.get::<Transform>(root).expect("placed");
    assert_eq!(
        placed.translation,
        Vec3::new(10.0, 0.0, 0.0),
        "the scene's placement"
    );
    assert_eq!(
        placed.scale,
        Vec3::new(2.0, 2.0, 2.0),
        "the prefab's new scale"
    );
    assert_eq!(name(&loaded, root), "Gate turret", "the scene's name");
    assert!(
        !loaded.get::<Light>(root).expect("lit").enabled,
        "the prefab's light, which the scene never touched"
    );
}

/// What the scene's author took out of an instance stays out, and what they
/// put in stays in: a member deleted, a component removed from the root, a
/// component added to a member, and an entity of their own hung under a
/// member — across a prefab edit that still reaches what they left alone.
#[test]
fn removed_and_added_parts_of_an_instance_persist() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    let sight = entity(&world, PersistentId::within(instance, turret.sight));

    world.despawn_subtree(barrel);
    world
        .remove_component::<Light>(root)
        .expect("the light comes off");
    world
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");
    let muzzle = world.spawn((Transform::identity(), Name::new("Muzzle")));
    world.set_parent(muzzle, Some(sight));
    let muzzle = world.mark_authored(muzzle).expect("authored");
    let scene = save(&world, &library);

    assert!(
        scene.entities.contains(&muzzle) && has_row(&scene, muzzle),
        "an entity the author added under an instance is the scene's own"
    );

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let sight = entity(prefab_world, turret.sight);
        prefab_world
            .get_mut::<Transform>(sight)
            .expect("placed")
            .translation = Vec3::new(0.0, 0.0, 3.0);
        let barrel = entity(prefab_world, turret.barrel);
        prefab_world.get_mut::<Name>(barrel).expect("named").0 = "Barrel Mk2".into();
        let root = entity(prefab_world, turret.root);
        prefab_world.get_mut::<Light>(root).expect("lit").enabled = false;
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    let root = entity(&loaded, instance);
    let sight = entity(&loaded, PersistentId::within(instance, turret.sight));
    assert_eq!(
        loaded.entity_with_id(PersistentId::within(instance, turret.barrel)),
        None,
        "the deleted barrel stays deleted"
    );
    assert!(
        loaded.get::<Light>(root).is_none(),
        "the removed light stays removed"
    );
    assert!(
        loaded
            .get::<Tag>(sight)
            .is_some_and(|tag| tag.contains("armed")),
        "the added tag stays"
    );
    assert_eq!(
        translation(&loaded, sight),
        Vec3::new(0.0, 0.0, 3.0),
        "the prefab's edit reaches what the scene left alone"
    );
    let muzzle = entity(&loaded, muzzle);
    assert_eq!(name(&loaded, muzzle), "Muzzle");
    assert_eq!(
        parent(&loaded, muzzle),
        Some(sight),
        "still hung under the sight"
    );
    assert_eq!(loaded.iter_entities().count(), 3, "root, sight and muzzle");
}

/// A scene entity that refers to an entity inside an instance refers to it
/// by the id the instance derives for it — and that id is the same after
/// the prefab gained a part and its sight was moved and renamed, so the
/// reference still finds the sight. The prefab's own reference, the turret
/// aiming at its sight, finds the instance's sight too.
#[test]
fn references_into_an_instance_resolve_after_prefab_edit() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let sight_id = PersistentId::within(instance, turret.sight);
    let sight = entity(&world, sight_id);
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Guard"),
        Script::new("ai/guard.erg", "Guard").with_field("watch", ScriptValue::Entity(sight)),
    ));
    let guard = world.mark_authored(guard).expect("authored");
    let scene = save(&world, &library);

    let mut references = Vec::new();
    for page in &scene.pages {
        if let Some(row) = page.rows.iter().position(|row| *row == guard) {
            for column in &page.columns {
                references_in(&column[row], &mut references);
            }
        }
    }
    assert_eq!(
        references,
        vec![EntityRef::Id(sight_id)],
        "the guard names the sight by its id inside the instance"
    );

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let root = entity(prefab_world, turret.root);
        let scope = prefab_world.spawn((Transform::identity(), Name::new("Scope")));
        prefab_world.set_parent(scope, Some(root));
        prefab_world.mark_authored(scope).expect("authored");
        let sight = entity(prefab_world, turret.sight);
        prefab_world
            .get_mut::<Transform>(sight)
            .expect("placed")
            .translation = Vec3::new(0.0, 0.0, 4.0);
        prefab_world.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));

    let sight = entity(&loaded, sight_id);
    assert_eq!(name(&loaded, sight), "Sight Mk2");
    let guard = entity(&loaded, guard);
    assert_eq!(
        loaded
            .get::<Script>(guard)
            .expect("scripted")
            .field("watch"),
        Some(&ScriptValue::Entity(sight)),
        "the scene's reference finds the sight"
    );
    let root = entity(&loaded, instance);
    assert_eq!(
        loaded.get::<Script>(root).expect("scripted").field("aim"),
        Some(&ScriptValue::Entity(sight)),
        "the prefab's own reference finds the instance's sight"
    );
}
