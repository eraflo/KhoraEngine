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

//! Which instance an entity belongs to, and what its prefab says of it: the
//! inspector's view of an instance, read against the prefab as it is now.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::{Name, Transform};
use khora_data::scene::{instance_of, instantiate_prefab, prefab_world, InstanceOf};

use super::prefab_sample::*;
use super::*;

/// A turret instance placed in a world of its own.
struct Placed {
    world: World,
    library: Library,
    prefab: AssetUUID,
    turret: Turret,
    /// The instance root, in `world`.
    root: EntityId,
    /// The instance root's identity.
    instance: PersistentId,
}

fn placed() -> Placed {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    Placed {
        world,
        library,
        prefab,
        turret,
        root,
        instance,
    }
}

/// A tower instance — a tower prefab holding a turret instance — placed in a
/// world of its own.
struct Towered {
    world: World,
    library: Library,
    turret_prefab: AssetUUID,
    tower_prefab: AssetUUID,
    turret: Turret,
    /// The tower's root, in `world`.
    tower_root: EntityId,
    /// The nested turret's root, in `world`.
    turret_root: EntityId,
    /// The nested turret's root identity.
    nested: PersistentId,
}

fn towered() -> Towered {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let tower_root =
        instantiate_prefab(&mut world, tower_prefab, &library).expect("the tower instantiates");
    let nested = PersistentId::within(id(&world, tower_root), tower.turret_root);
    let turret_root = entity(&world, nested);
    Towered {
        world,
        library,
        turret_prefab,
        tower_prefab,
        turret,
        tower_root,
        turret_root,
        nested,
    }
}

/// The root of an instance and each entity the prefab put inside it belong
/// to that instance: the root found from itself, the barrel and the sight
/// found through their parent.
#[test]
fn an_instance_root_and_its_members_belong_to_the_instance() {
    let placed = placed();
    let expected = Some(InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    });
    let barrel = entity(
        &placed.world,
        PersistentId::within(placed.instance, placed.turret.barrel),
    );
    let sight = entity(
        &placed.world,
        PersistentId::within(placed.instance, placed.turret.sight),
    );

    for (what, member) in [("root", placed.root), ("barrel", barrel), ("sight", sight)] {
        assert_eq!(
            instance_of(&placed.world, member, &placed.library),
            expected,
            "the {what} belongs to the turret instance"
        );
    }
}

/// Inside a tower, the turret's barrel and the turret's root belong to the
/// turret — the innermost instance holding them — and the tower's root to
/// the tower.
#[test]
fn a_nested_member_belongs_to_its_innermost_instance() {
    let towered = towered();
    let barrel = entity(
        &towered.world,
        PersistentId::within(towered.nested, towered.turret.barrel),
    );
    let turret = Some(InstanceOf {
        root: towered.turret_root,
        prefab: towered.turret_prefab,
    });

    assert_eq!(
        instance_of(&towered.world, barrel, &towered.library),
        turret,
        "the nested barrel is the turret's"
    );
    assert_eq!(
        instance_of(&towered.world, towered.turret_root, &towered.library),
        turret,
        "the nested turret's root is the turret's own"
    );
    assert_eq!(
        instance_of(&towered.world, towered.tower_root, &towered.library),
        Some(InstanceOf {
            root: towered.tower_root,
            prefab: towered.tower_prefab,
        }),
        "the tower's root is the tower's"
    );
}

/// An entity the author hung under a member is the scene's own, not the
/// prefab's: it belongs to no instance — under a plain instance, and under a
/// member of an instance nested in another.
#[test]
fn an_entity_the_author_hung_under_a_member_is_not_part_of_the_instance() {
    let mut placed = placed();
    let sight = entity(
        &placed.world,
        PersistentId::within(placed.instance, placed.turret.sight),
    );
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(sight));
    placed.world.mark_authored(muzzle).expect("authored");
    assert_eq!(
        instance_of(&placed.world, muzzle, &placed.library),
        None,
        "an author's entity under the sight"
    );

    let mut towered = towered();
    let barrel = entity(
        &towered.world,
        PersistentId::within(towered.nested, towered.turret.barrel),
    );
    let flag = towered
        .world
        .spawn((Transform::identity(), Name::new("Flag")));
    towered.world.set_parent(flag, Some(barrel));
    towered.world.mark_authored(flag).expect("authored");
    assert_eq!(
        instance_of(&towered.world, flag, &towered.library),
        None,
        "an author's entity under a nested member belongs neither to the turret nor to the tower"
    );
}

/// An entity that is no part of a prefab belongs to no instance — a plain
/// one, and one an instance hangs under: the walk goes up, never down.
#[test]
fn a_plain_entity_belongs_to_no_instance() {
    let mut placed = placed();
    let lone = placed
        .world
        .spawn((Transform::identity(), Name::new("Lone")));
    placed.world.mark_authored(lone).expect("authored");
    let base = placed
        .world
        .spawn((Transform::identity(), Name::new("Base")));
    placed.world.mark_authored(base).expect("authored");
    placed.world.set_parent(placed.root, Some(base));

    assert_eq!(instance_of(&placed.world, lone, &placed.library), None);
    assert_eq!(
        instance_of(&placed.world, base, &placed.library),
        None,
        "the parent of an instance root is not inside the instance"
    );
}

/// The prefab's world holds the prefab's values under the instance's own
/// identities: the barrel the scene pushed sideways and the root it renamed
/// are found there by their ids, as the prefab has them.
#[test]
fn the_prefab_world_holds_the_prefabs_values_under_the_instances_ids() {
    let mut placed = placed();
    let barrel_id = PersistentId::within(placed.instance, placed.turret.barrel);
    let barrel = entity(&placed.world, barrel_id);
    placed
        .world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 5.0;
    placed.world.get_mut::<Name>(placed.root).expect("named").0 = "Gate turret".into();
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };

    let prefab = prefab_world(&placed.world, &instance, &placed.library)
        .unwrap_or_else(|e| panic!("the prefab world does not build: {e}"));

    assert_eq!(prefab.iter_entities().count(), 3, "root, barrel and sight");
    assert_eq!(
        translation(&prefab, entity(&prefab, barrel_id)),
        Vec3::new(0.0, 1.0, 0.0),
        "the prefab's barrel, not the scene's override"
    );
    assert_eq!(
        name(&prefab, entity(&prefab, placed.instance)),
        "Turret",
        "the root is known by the instance root's id, with the prefab's name"
    );
    assert_eq!(
        translation(&placed.world, barrel),
        Vec3::new(5.0, 1.0, 0.0),
        "the live world keeps its override"
    );
}

/// An edit of the prefab made since the instance was placed shows in the
/// prefab's world, though the live instance has not been loaded again.
#[test]
fn the_prefab_world_follows_a_prefab_edit() {
    let placed = placed();
    let (edited_turret, ()) = edited(&placed.turret.record, |prefab_world| {
        let barrel = entity(prefab_world, placed.turret.barrel);
        prefab_world
            .get_mut::<Transform>(barrel)
            .expect("placed")
            .translation = Vec3::new(0.0, 2.0, 0.0);
    });
    let library = Library::default().with(placed.prefab, edited_turret);
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };

    let prefab = prefab_world(&placed.world, &instance, &library)
        .unwrap_or_else(|e| panic!("the prefab world does not build: {e}"));

    let barrel = entity(
        &prefab,
        PersistentId::within(placed.instance, placed.turret.barrel),
    );
    assert_eq!(translation(&prefab, barrel), Vec3::new(0.0, 2.0, 0.0));
}

/// The prefab world of a turret nested in a tower is the turret prefab's:
/// the raise the tower gives its turret is the tower's override, not the
/// turret prefab's value.
#[test]
fn the_prefab_world_of_a_nested_instance_is_its_own_prefabs() {
    let towered = towered();
    assert_eq!(
        translation(&towered.world, towered.turret_root),
        Vec3::new(0.0, 5.0, 0.0),
        "the tower raises its turret"
    );
    let instance = InstanceOf {
        root: towered.turret_root,
        prefab: towered.turret_prefab,
    };

    let prefab = prefab_world(&towered.world, &instance, &towered.library)
        .unwrap_or_else(|e| panic!("the prefab world does not build: {e}"));

    assert_eq!(prefab.iter_entities().count(), 3, "the turret's three");
    assert_eq!(
        translation(&prefab, entity(&prefab, towered.nested)),
        Vec3::ZERO,
        "the turret prefab's root stands at its origin"
    );
    let barrel = entity(
        &prefab,
        PersistentId::within(towered.nested, towered.turret.barrel),
    );
    assert_eq!(translation(&prefab, barrel), Vec3::new(0.0, 1.0, 0.0));
}

/// The prefab world of an instance whose prefab can no longer be read is
/// refused, not built empty.
#[test]
fn the_prefab_world_of_an_instance_whose_prefab_is_gone_is_refused() {
    let placed = placed();
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };

    assert!(prefab_world(&placed.world, &instance, &Library::default()).is_err());
}
