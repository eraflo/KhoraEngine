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

//! Prefab instances made in play: rooted at an id a script reserved in its
//! turn, with a created identity — the game made it, no author did — and
//! linked like any instance. And the catalog of the prefabs a project holds,
//! which a script checks its spawns against.

use std::collections::{BTreeSet, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::EntityReserver;
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{Name, PrefabInstance, Transform};
use khora_data::flow::{Flow, PrefabCatalogFlow, PrefabCatalogView};
use khora_data::scene::{instantiate_prefab_at, NoPrefabs, PrefabCatalog};

use super::declared_components::field_of;
use super::prefab_sample::*;
use super::*;

/// The world's live entities, and how many.
fn census(world: &World) -> (usize, HashSet<EntityId>) {
    (world.entity_count(), world.iter_entities().collect())
}

/// **The instance root lands at the reserved id.** It carries the link, a
/// created identity — not an authored one — and every other entity of the
/// prefab is there under the id derived from that root, wired as in the
/// prefab.
#[test]
fn an_instance_made_in_play_lands_at_its_reserved_id() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = world.reserver().reserve();

    instantiate_prefab_at(&mut world, root, prefab, &library)
        .unwrap_or_else(|e| panic!("the turret instantiates: {e}"));

    assert!(world.contains(root), "the root is alive at {root:?}");
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab })
    );
    assert_eq!(
        name(&world, root),
        "Turret",
        "the prefab's root is the root"
    );
    let instance = id(&world, root);
    assert!(instance.is_created(), "{instance:?} is a created identity");
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
        field_of(&world, root, TURRET, "aim"),
        Some(ScriptValue::Entity(sight)),
        "a reference inside the prefab points inside the instance"
    );
    for member in [barrel, sight] {
        assert!(
            world.get::<PrefabInstance>(member).is_none(),
            "only the root carries the link"
        );
    }
}

/// **Two instances made in play are two**: each root at its own id, each
/// with its own created identity, their members apart.
#[test]
fn two_instances_made_in_play_have_their_own_identities() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let reserver = world.reserver();
    let (first, second) = (reserver.reserve(), reserver.reserve());

    instantiate_prefab_at(&mut world, first, prefab, &library).expect("the first instantiates");
    instantiate_prefab_at(&mut world, second, prefab, &library).expect("the second instantiates");

    let (a, b) = (id(&world, first), id(&world, second));
    assert_ne!(a, b);
    assert!(a.is_created() && b.is_created());
    assert_eq!(world.iter_entities().count(), 6);
    assert_ne!(
        entity(&world, PersistentId::within(a, turret.barrel)),
        entity(&world, PersistentId::within(b, turret.barrel))
    );
}

/// **An instance made in play is linked**: saved, it is a link to its
/// prefab, and a later edit of the prefab reaches it on load.
#[test]
fn an_instance_made_in_play_stays_linked() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = world.reserver().reserve();
    instantiate_prefab_at(&mut world, root, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);

    let scene = save(&world, &library);
    assert_eq!(scene.instances.len(), 1, "{:?}", scene.instances);
    assert_eq!(scene.instances[0].root, instance);
    assert_eq!(scene.instances[0].prefab, prefab);

    let (edited_turret, ()) = edited(&turret.record, |prefab_world| {
        let sight = entity(prefab_world, turret.sight);
        prefab_world.get_mut::<Name>(sight).expect("named").0 = "Sight Mk2".into();
    });
    let loaded = open(&scene, &Library::default().with(prefab, edited_turret));
    let sight = entity(&loaded, PersistentId::within(instance, turret.sight));
    assert_eq!(name(&loaded, sight), "Sight Mk2");
}

/// **An id the world's reserver never handed out is refused**, and the world
/// is left as it was: a live entity's id, one another world reserved, one a
/// reserver no world owns gave.
#[test]
fn an_id_the_reserver_did_not_hand_out_is_refused() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let live = world.spawn((Transform::identity(), Name::new("Level")));
    let other = World::new();
    let before = census(&world);

    for refused in [
        live,
        other.reserver().reserve(),
        EntityReserver::new().reserve(),
    ] {
        assert!(
            instantiate_prefab_at(&mut world, refused, prefab, &library).is_err(),
            "{refused:?} was taken"
        );
        assert_eq!(census(&world), before, "the world changed for {refused:?}");
        assert_eq!(name(&world, live), "Level");
        assert!(world.get::<PrefabInstance>(live).is_none());
    }
}

/// **An id committed once is refused the second time**: the first instance
/// stays as it was, and nothing else is made.
#[test]
fn an_id_already_committed_is_refused() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = world.reserver().reserve();
    instantiate_prefab_at(&mut world, root, prefab, &library).expect("the turret instantiates");
    let before = census(&world);
    let instance = id(&world, root);

    assert!(instantiate_prefab_at(&mut world, root, prefab, &library).is_err());

    assert_eq!(census(&world), before);
    assert_eq!(id(&world, root), instance);
}

/// **A missing prefab leaves the world as it was**: nothing is made, the
/// reserved id holds no entity, and the error names the prefab.
#[test]
fn a_missing_prefab_leaves_the_world_as_it_was() {
    let turret = turret();
    let known = AssetUUID::new();
    let missing = AssetUUID::new();
    let library = Library::default().with(known, turret.record.clone());
    let mut world = World::new();
    world.spawn((Transform::identity(), Name::new("Level")));
    let before = census(&world);

    for (prefabs, what) in [
        (
            &library as &dyn khora_data::scene::PrefabSource,
            "a library",
        ),
        (&NoPrefabs, "no source"),
    ] {
        let root = world.reserver().reserve();
        let failure =
            instantiate_prefab_at(&mut world, root, missing, prefabs).expect_err("no such prefab");
        assert!(
            failure.message.contains(&missing.to_string()),
            "{what}: {}",
            failure.message
        );
        assert!(!world.contains(root), "{what}: the root is alive");
        assert_eq!(census(&world), before, "{what}: the world changed");
    }
}

/// The reserved id a refused instance did not use is the world's still: a
/// later instance at it lands.
#[test]
fn a_refused_instance_leaves_its_id_usable() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = world.reserver().reserve();

    assert!(instantiate_prefab_at(&mut world, root, AssetUUID::new(), &library).is_err());
    instantiate_prefab_at(&mut world, root, prefab, &library).expect("the turret instantiates");

    assert!(world.contains(root));
    assert_eq!(world.iter_entities().count(), 3);
}

// ─── The catalog ────────────────────────────────────────────────────────────

/// **A new catalog holds no prefab.**
#[test]
fn a_new_catalog_holds_nothing() {
    let catalog = PrefabCatalog::new();
    assert!(!catalog.holds(AssetUUID::new()));
    assert!(catalog.snapshot().is_empty());
}

/// **Replacing the catalog makes it exactly what it was given**: what it
/// held before and was not given again is gone.
#[test]
fn replacing_the_catalog_makes_it_what_it_was_given() {
    let (a, b, c) = (AssetUUID::new(), AssetUUID::new(), AssetUUID::new());
    let catalog = PrefabCatalog::new();

    catalog.replace([a, b]);
    assert!(catalog.holds(a) && catalog.holds(b));
    assert!(!catalog.holds(c));
    assert_eq!(*catalog.snapshot(), BTreeSet::from([a, b]));

    catalog.replace([c]);
    assert!(!catalog.holds(a) && !catalog.holds(b));
    assert!(catalog.holds(c));
    assert_eq!(*catalog.snapshot(), BTreeSet::from([c]));
}

/// **A snapshot is what the catalog held when it was taken**: a later
/// replacement does not reach into it.
#[test]
fn a_catalog_snapshot_keeps_what_it_was() {
    let (a, b) = (AssetUUID::new(), AssetUUID::new());
    let catalog = PrefabCatalog::new();
    catalog.replace([a]);

    let taken = catalog.snapshot();
    catalog.replace([b]);

    assert_eq!(*taken, BTreeSet::from([a]));
    assert_eq!(*catalog.snapshot(), BTreeSet::from([b]));
}

/// **The flow publishes the runtime's catalog**, and `None` — nothing to
/// check against — when the runtime holds none.
#[test]
fn the_catalog_flow_publishes_the_runtimes_catalog() {
    let world = World::new();
    let prefab = AssetUUID::new();
    let catalog = PrefabCatalog::new();
    catalog.replace([prefab]);
    let mut runtime = Runtime::default();
    runtime.resources.insert(catalog);
    let mut flow = PrefabCatalogFlow;

    let selection = flow.select(&world, &runtime);
    let view: PrefabCatalogView = flow.project(&world, &selection, &runtime);
    assert_eq!(view.prefabs.as_deref(), Some(&BTreeSet::from([prefab])));

    let none = Runtime::default();
    let selection = flow.select(&world, &none);
    assert!(flow.project(&world, &selection, &none).prefabs.is_none());
}

// ─── Instances made in play, nested, damaged and saved ──────────────────────

/// **A prefab holding another, made in play, brings every member of both**:
/// the tower's root at the reserved id, the turret nested in it with its own
/// members, raised as the tower placed it.
#[test]
fn a_nested_prefab_made_in_play_brings_every_member() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let mut library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let tower_prefab = AssetUUID::new();
    library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let root = world.reserver().reserve();

    instantiate_prefab_at(&mut world, root, tower_prefab, &library)
        .unwrap_or_else(|e| panic!("the tower instantiates: {e}"));

    assert_eq!(name(&world, root), "Tower");
    assert_eq!(world.iter_entities().count(), 4, "the tower and its turret");
    let instance = id(&world, root);
    let nested = entity(&world, PersistentId::within(instance, tower.turret_root));
    assert_eq!(name(&world, nested), "Turret");
    assert_eq!(parent(&world, nested), Some(root));
    assert_eq!(
        world.get::<Transform>(nested).map(|t| t.translation.y),
        Some(5.0),
        "the tower's override of its turret"
    );
    let names: HashSet<String> = world
        .iter_entities()
        .map(|entity| name(&world, entity).to_owned())
        .collect();
    assert!(
        names.contains("Barrel") && names.contains("Sight"),
        "{names:?}"
    );
}

/// **A prefab that fails while its rows are read leaves the reserved id the
/// world's**: nothing is made, and a later instance at the same id lands
/// whole — no more entities than the prefab holds.
#[test]
fn a_damaged_prefab_leaves_its_reserved_id_usable() {
    let turret = turret();
    let mut damaged = turret.record.clone();
    *value_mut(&mut damaged, turret.barrel, "Transform") = Record::Str("not a transform".into());
    let (good, bad) = (AssetUUID::new(), AssetUUID::new());
    let library = Library::default()
        .with(good, turret.record.clone())
        .with(bad, damaged);
    let mut world = World::new();
    world.spawn((Transform::identity(), Name::new("Level")));
    let before = census(&world);
    let root = world.reserver().reserve();

    assert!(instantiate_prefab_at(&mut world, root, bad, &library).is_err());
    assert_eq!(census(&world), before, "a damaged prefab changed the world");
    assert!(!world.contains(root));

    instantiate_prefab_at(&mut world, root, good, &library).expect("the turret instantiates");
    assert!(world.contains(root));
    assert_eq!(world.iter_entities().count(), 4, "the level and the turret");
}

/// **An instance made in play survives a game save**: saved against the
/// scene it was made in and loaded again, its root keeps its created
/// identity and its link, its members their ids within it — and an instance
/// made after the load takes another identity.
#[test]
fn an_instance_made_in_play_survives_a_game_save() {
    use khora_data::scene::{capture_save, compose, prepare_game};

    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let level = world.spawn((Transform::identity(), Name::new("Level")));
    world.mark_authored(level).expect("authored");
    let base = save(&world, &library);
    let root = world.reserver().reserve();
    instantiate_prefab_at(&mut world, root, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);

    let saved = capture_save(&world, AssetUUID::new(), &base)
        .unwrap_or_else(|e| panic!("the game saves: {e}"));
    let mut loaded = World::new();
    prepare_game(&mut loaded, &compose(&base, &saved))
        .unwrap_or_else(|e| panic!("the game loads: {e}"))
        .commit(&mut loaded, Identity::Keep);

    let back = entity(&loaded, instance);
    assert_eq!(name(&loaded, back), "Turret");
    assert_eq!(
        loaded.get::<PrefabInstance>(back),
        Some(&PrefabInstance { prefab })
    );
    let barrel = entity(&loaded, PersistentId::within(instance, turret.barrel));
    assert_eq!(parent(&loaded, barrel), Some(back));
    assert_eq!(loaded.iter_entities().count(), 4);

    let again = loaded.reserver().reserve();
    instantiate_prefab_at(&mut loaded, again, prefab, &library).expect("another instantiates");
    assert_ne!(
        id(&loaded, again),
        instance,
        "two instances share an identity"
    );
    assert_eq!(loaded.iter_entities().count(), 7);
}
