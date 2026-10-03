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

//! Persistent identities, and entity ids held back for a load.

use std::collections::HashSet;

use khora_data::ecs::{Name, Transform, World};

use super::*;

/// Every live entity answers an identity when asked: the game's spawns in the
/// created namespace, numbered by the world; what an author makes is re-tagged
/// into the authored one, once — asking again changes nothing — and the
/// identity leaves with the entity.
#[test]
fn a_spawn_answers_a_created_id_and_mark_authored_retags() {
    let mut world = World::new();
    let first = world.spawn(Transform::identity());
    let second = world.spawn(Transform::identity());

    let first_id = world
        .persistent_id(first)
        .expect("a spawned entity has an id");
    let second_id = world
        .persistent_id(second)
        .expect("a spawned entity has an id");
    assert!(first_id.is_created(), "a spawn is the game's: created");
    assert!(second_id.is_created());
    assert_ne!(first_id, second_id, "two spawns, two identities");
    assert_eq!(world.entity_with_id(first_id), Some(first));
    assert_eq!(world.entity_with_id(second_id), Some(second));

    let authored = world
        .mark_authored(first)
        .expect("a live entity can be authored");
    assert!(!authored.is_created(), "an author's entity is authored");
    assert_eq!(world.persistent_id(first), Some(authored));
    assert_eq!(world.entity_with_id(authored), Some(first));
    assert_eq!(
        world.entity_with_id(first_id),
        None,
        "the created id the entity had is no longer its"
    );

    assert_eq!(
        world.mark_authored(first),
        Some(authored),
        "an authored entity keeps its identity"
    );
    assert_eq!(world.persistent_id(first), Some(authored));

    assert!(world.despawn(first));
    assert_eq!(
        world.persistent_id(first),
        None,
        "despawn drops the identity"
    );
    assert_eq!(world.entity_with_id(authored), None);
    assert_eq!(
        world.mark_authored(first),
        None,
        "a dead entity cannot be authored"
    );
    assert_eq!(
        world.entity_with_id(second_id),
        Some(second),
        "others keep theirs"
    );
}

/// Authored identities are random so that two people adding entities on two
/// branches do not hand out the same one; within one world they never repeat.
#[test]
fn authored_ids_do_not_repeat() {
    let mut world = World::new();
    let mut seen = HashSet::new();
    for _ in 0..500 {
        let entity = world.spawn(Transform::identity());
        let id = world.mark_authored(entity).expect("authored");
        assert!(!id.is_created());
        assert!(seen.insert(id), "{id:?} was handed out twice");
    }
}

/// A load gives an entity the identity its save recorded; the entity is then
/// found by it, and by nothing else.
#[test]
fn set_persistent_id_moves_the_identity() {
    let mut world = World::new();
    let entity = world.spawn(Transform::identity());
    let spawned_id = world.persistent_id(entity).expect("id");

    let saved = PersistentId::authored(0x00c0_ffee);
    world.set_persistent_id(entity, saved);
    assert_eq!(world.persistent_id(entity), Some(saved));
    assert_eq!(world.entity_with_id(saved), Some(entity));
    assert_eq!(world.entity_with_id(spawned_id), None);
}

/// An id reserved for a load names no live entity — no query, no iteration,
/// no identity sees it — and the world never hands it to a spawn while it is
/// held. Bringing it to life makes it an ordinary, empty entity, once.
#[test]
fn a_reserved_id_is_not_alive_until_spawned() {
    let mut world = World::new();
    let reserved = world.reserve_entity();
    assert!(!world.contains(reserved), "a reservation is not alive");
    assert_eq!(world.persistent_id(reserved), None);
    assert!(!world.iter_entities().any(|e| e == reserved));

    for _ in 0..64 {
        let spawned = world.spawn(Transform::identity());
        assert_ne!(
            spawned.index, reserved.index,
            "a spawn was handed the reserved slot"
        );
    }
    let again = world.reserve_entity();
    assert_ne!(again.index, reserved.index, "one slot, one reservation");

    assert!(
        world.spawn_reserved(reserved),
        "a reservation comes to life"
    );
    assert!(world.contains(reserved));
    assert!(
        world.get::<Transform>(reserved).is_none(),
        "it comes to life empty"
    );
    assert!(
        world.add_component(reserved, Name::new("Loaded")).is_ok(),
        "and takes components like any entity"
    );
    assert!(
        !world.spawn_reserved(reserved),
        "it cannot come to life twice"
    );

    let live = world.spawn(Transform::identity());
    assert!(
        !world.spawn_reserved(live),
        "a live entity is no reservation"
    );
}

/// A reservation given back never lived: it is not alive, cannot be brought
/// to life afterwards, and its slot returns to the pool for the next spawn.
#[test]
fn a_released_id_goes_back_to_the_pool() {
    let mut world = World::new();
    let reserved = world.reserve_entity();
    world.release_reserved(reserved);
    assert!(!world.contains(reserved));
    assert!(
        !world.spawn_reserved(reserved),
        "a released reservation cannot be spawned"
    );

    let next = world.spawn(Transform::identity());
    assert_eq!(
        next.index, reserved.index,
        "the released slot is the next one handed out"
    );
    assert!(world.contains(next));
}

/// A scene's identities come back as they were saved. What the game spawns
/// next, once asked for its identity, can never take one the scene already
/// uses.
#[test]
fn persistent_ids_survive_save_and_load() {
    let mut src = World::new();
    let mut entities: Vec<_> = (0..40)
        .map(|i| src.spawn((Transform::identity(), Name::new(format!("e{i}")))))
        .collect();
    // Gaps in the created numbering, and a few authored ids. Every entity is
    // asked for its identity first: created ids are numbered when first
    // asked for, so only an entity that had one leaves a gap.
    for &entity in &entities {
        src.persistent_id(entity).expect("a live entity has an id");
    }
    for gone in entities.drain(5..15) {
        src.despawn(gone);
    }
    for &entity in &entities[..3] {
        src.mark_authored(entity).expect("authored");
    }
    let saved: Vec<_> = entities
        .iter()
        .map(|&e| {
            (
                src.persistent_id(e).expect("id"),
                src.get::<Name>(e).cloned(),
            )
        })
        .collect();

    for (name, encoding) in every_encoding() {
        let (mut dst, _) = reload(&src, name, encoding);
        for (id, label) in &saved {
            let entity = dst
                .entity_with_id(*id)
                .unwrap_or_else(|| panic!("{name}: {id:?} did not survive"));
            assert_eq!(dst.persistent_id(entity), Some(*id), "{name}");
            assert_eq!(dst.get::<Name>(entity).cloned(), *label, "{name}: {id:?}");
        }

        let loaded: HashSet<_> = saved.iter().map(|(id, _)| *id).collect();
        for _ in 0..64 {
            let spawned = dst.spawn(Transform::identity());
            let id = dst.persistent_id(spawned).expect("a spawn has an id");
            assert!(id.is_created());
            assert!(
                !loaded.contains(&id),
                "{name}: a spawn after the load took the loaded identity {id:?}"
            );
            assert_eq!(dst.entity_with_id(id), Some(spawned), "{name}");
        }
        for (id, _) in &saved {
            assert!(
                dst.entity_with_id(*id).is_some(),
                "{name}: {id:?} was taken over by a later spawn"
            );
        }
    }
}
