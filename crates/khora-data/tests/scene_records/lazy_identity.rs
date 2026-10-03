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

//! Persistent identities are given when first needed, not at spawn.
//!
//! A spawn costs no identity: an entity is numbered in the created namespace
//! the first time something asks for its identity — `persistent_id`, a
//! capture — in the order they ask. Until then no identity names it.

use std::collections::HashSet;

use khora_data::ecs::{Name, Parent, Transform, World};
use khora_data::scene::capture_subtree;
use khora_data::scene::record::EntityRef;

use super::subtree::references_in;
use super::*;

/// The first created identities a world would hand out.
fn first_created(count: u64) -> Vec<PersistentId> {
    (0..count).map(PersistentId::created).collect()
}

/// Asserts no entity of `world` is known by any of the first `count` created
/// identities.
fn assert_no_created_id_held(world: &World, count: u64, why: &str) {
    for id in first_created(count) {
        assert_eq!(world.entity_with_id(id), None, "{why}: {id:?} is held");
    }
}

/// Spawning gives no identity: after spawning several entities — by
/// `spawn` and by bringing a reservation to life — no created identity names
/// any of them.
#[test]
fn a_spawn_assigns_no_identity() {
    let mut world = World::new();
    for i in 0..5 {
        world.spawn((Transform::identity(), Name::new(format!("e{i}"))));
    }
    let reserved = world.reserve_entity();
    assert!(
        world.spawn_reserved(reserved),
        "the reservation comes to life"
    );

    assert_no_created_id_held(&world, 16, "nothing was asked for");
}

/// Created identities are numbered in the order they are first asked for,
/// not in spawn order; asking again returns the same identity, and the
/// entity is then found by it.
#[test]
fn created_ids_are_numbered_in_the_order_first_asked_for() {
    let mut world = World::new();
    let a = world.spawn(Transform::identity());
    let b = world.spawn(Transform::identity());
    let c = world.spawn(Transform::identity());

    assert_eq!(world.persistent_id(c), Some(PersistentId::created(0)));
    assert_eq!(world.persistent_id(a), Some(PersistentId::created(1)));
    assert_eq!(
        world.persistent_id(c),
        Some(PersistentId::created(0)),
        "asking twice returns the same identity"
    );
    assert_eq!(world.persistent_id(b), Some(PersistentId::created(2)));

    for (entity, number) in [(c, 0), (a, 1), (b, 2)] {
        assert_eq!(
            world.entity_with_id(PersistentId::created(number)),
            Some(entity),
            "created({number}) names the entity first asked for it"
        );
    }
    assert_eq!(
        world.entity_with_id(PersistentId::created(3)),
        None,
        "three asked, three numbered"
    );
}

/// A dead entity — despawned before anyone asked, or never spawned — answers
/// no identity and takes no number: the next live entity asked for one gets
/// the first.
#[test]
fn a_dead_entity_answers_none_and_takes_no_number() {
    let mut world = World::new();
    let gone = world.spawn(Transform::identity());
    let live = world.spawn(Transform::identity());
    assert!(world.despawn(gone));
    let reserved = world.reserve_entity();
    let unknown = EntityId {
        index: 9_999,
        generation: 0,
    };

    for (what, entity) in [
        ("despawned", gone),
        ("reserved", reserved),
        ("unknown", unknown),
    ] {
        assert_eq!(world.persistent_id(entity), None, "{what}");
    }
    assert_no_created_id_held(&world, 8, "a dead entity is given nothing");
    assert_eq!(
        world.persistent_id(live),
        Some(PersistentId::created(0)),
        "the dead took no number"
    );
}

/// A capture identifies every entity it writes, row and reference alike, so
/// the save holds identities; asking afterwards returns the very identities
/// the save holds. Entities spawned and despawned before, never asked for,
/// leave no gap.
#[test]
fn a_capture_assigns_the_ids_it_writes_and_later_asks_agree() {
    let mut world = World::new();
    for _ in 0..4 {
        let scratch = world.spawn(Transform::identity());
        world.despawn(scratch);
    }
    let parent = world.spawn((Transform::identity(), Name::new("Parent")));
    let child = world.spawn((Transform::identity(), Name::new("Child")));
    let other = world.spawn((Transform::identity(), Name::new("Other")));
    assert!(world.set_parent(child, Some(parent)));

    let record = capture_world(&world).expect("captures");

    let recorded: HashSet<PersistentId> = record.entities.iter().copied().collect();
    assert_eq!(
        recorded,
        first_created(3).into_iter().collect(),
        "three entities written, numbered from the first created identity"
    );
    for entity in [parent, child, other] {
        let id = world
            .persistent_id(entity)
            .expect("a captured entity has an id");
        assert!(
            recorded.contains(&id),
            "{id:?} is the identity it was saved by"
        );
        assert_eq!(world.entity_with_id(id), Some(entity));
    }

    let child_id = world.persistent_id(child).expect("id");
    let (page, column, row) = slot_of(&record, child_id, "Parent").expect("the child's parent");
    let mut references = Vec::new();
    references_in(&record.pages[page].columns[column][row], &mut references);
    assert_eq!(
        references,
        vec![EntityRef::Id(world.persistent_id(parent).expect("id"))],
        "the reference is written by the identity asking returns"
    );
    assert_eq!(world.get::<Parent>(child).map(|p| p.0), Some(parent));
}

/// A subtree capture identifies what it writes, and nothing it leaves out:
/// an entity outside the subtree, never asked for, is still unnumbered.
#[test]
fn a_subtree_capture_assigns_only_the_subtree() {
    let mut world = World::new();
    let bystander = world.spawn((Transform::identity(), Name::new("Bystander")));
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    let leaf = world.spawn((Transform::identity(), Name::new("Leaf")));
    assert!(world.set_parent(leaf, Some(root)));

    let record = capture_subtree(&world, root).expect("captures");

    let recorded: HashSet<PersistentId> = record.entities.iter().copied().collect();
    assert_eq!(recorded, first_created(2).into_iter().collect());
    for entity in [root, leaf] {
        let id = world.persistent_id(entity).expect("id");
        assert!(recorded.contains(&id), "{id:?}");
    }
    assert_eq!(
        world.entity_with_id(PersistentId::created(2)),
        None,
        "the bystander was not numbered"
    );
    assert_eq!(
        world.persistent_id(bystander),
        Some(PersistentId::created(2)),
        "asked afterwards, it takes the next number"
    );
}

/// A load beside residents nobody has identified keeps every saved created
/// identity — an unidentified resident cannot collide — and a resident asked
/// afterwards gets a number no one holds.
#[test]
fn a_load_beside_unidentified_residents_keeps_every_saved_created_id() {
    let mut src = World::new();
    let saved: Vec<(PersistentId, String)> = (0..3)
        .map(|i| {
            let label = format!("Saved{i}");
            let entity = src.spawn((Transform::identity(), Name::new(label.clone())));
            (src.persistent_id(entity).expect("id"), label)
        })
        .collect();
    assert!(saved.iter().all(|(id, _)| id.is_created()));
    let record = capture_world(&src).expect("captures");

    let mut dst = World::new();
    let residents: Vec<EntityId> = (0..3)
        .map(|i| dst.spawn((Transform::identity(), Name::new(format!("Resident{i}")))))
        .collect();

    let applied = apply(&mut dst, &record, Identity::Keep).expect("loads");
    assert_eq!(applied.entities.len(), saved.len());
    for (id, label) in &saved {
        let entity = dst
            .entity_with_id(*id)
            .unwrap_or_else(|| panic!("{id:?} is held by no one after the load"));
        assert_eq!(
            dst.get::<Name>(entity).map(|n| n.as_str()),
            Some(label.as_str()),
            "{id:?} stays with the entity it was saved for"
        );
        assert_eq!(dst.persistent_id(entity), Some(*id));
    }
    assert!(
        applied
            .entities
            .iter()
            .all(|(recorded, _)| saved.iter().any(|(id, _)| id == recorded)),
        "{:?}",
        applied.entities
    );

    let saved_ids: HashSet<PersistentId> = saved.iter().map(|(id, _)| *id).collect();
    let mut seen = HashSet::new();
    for &resident in &residents {
        let id = dst
            .persistent_id(resident)
            .expect("a live resident answers an id");
        assert!(id.is_created(), "{id:?}");
        assert!(
            !saved_ids.contains(&id),
            "a resident asked after the load took the saved identity {id:?}"
        );
        assert!(seen.insert(id), "{id:?} handed out twice");
        assert_eq!(dst.entity_with_id(id), Some(resident));
    }
}

/// Despawning an identified entity forgets its identity: no entity is found
/// by it afterwards, and the dead entity answers none.
#[test]
fn despawning_an_identified_entity_frees_its_id() {
    let mut world = World::new();
    let unasked = world.spawn(Transform::identity());
    let asked = world.spawn(Transform::identity());
    let id = world.persistent_id(asked).expect("id");
    assert_eq!(id, PersistentId::created(0), "the first asked for");

    assert!(world.despawn(asked));
    assert_eq!(world.entity_with_id(id), None, "the identity left with it");
    assert_eq!(world.persistent_id(asked), None);

    assert!(
        world.despawn(unasked),
        "an unidentified entity despawns too"
    );
    assert_eq!(world.persistent_id(unasked), None);
    assert_eq!(world.iter_entities().count(), 0);
}

/// Asking through a shared `&World` from many threads at once numbers each
/// entity once: every thread sees the same identity for the same entity, and
/// the entities asked share the first created numbers between them — the
/// unasked spawned before took none.
#[test]
fn asking_from_many_threads_numbers_each_entity_once() {
    const ASKED: u64 = 64;
    let mut world = World::new();
    for _ in 0..100 {
        world.spawn(Transform::identity());
    }
    let targets: Vec<EntityId> = (0..ASKED)
        .map(|_| world.spawn(Transform::identity()))
        .collect();

    let world = &world;
    let answers: Vec<Vec<PersistentId>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|thread| {
                let targets = &targets;
                scope.spawn(move || {
                    // Each thread walks the targets from a different start.
                    let shift = thread * 7;
                    let mut ids = vec![None; targets.len()];
                    for step in 0..targets.len() {
                        let at = (step + shift) % targets.len();
                        ids[at] = world.persistent_id(targets[at]);
                    }
                    ids.into_iter()
                        .map(|id| id.expect("a live entity answers an id"))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a thread asking does not panic"))
            .collect()
    });

    for other in &answers[1..] {
        assert_eq!(other, &answers[0], "every thread sees the same identities");
    }
    let ids: HashSet<PersistentId> = answers[0].iter().copied().collect();
    assert_eq!(ids.len(), targets.len(), "one identity per entity");
    assert_eq!(ids, first_created(ASKED).into_iter().collect());
    for (entity, id) in targets.iter().zip(&answers[0]) {
        assert_eq!(world.entity_with_id(*id), Some(*entity));
    }
}
