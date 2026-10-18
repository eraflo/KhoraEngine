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

//! Entity ids reserved through a shared reference.
//!
//! A world-free lane cannot spawn, but it can hold its world's
//! [`EntityReserver`] and take ids from it without a lock. The world settles
//! them at its next exclusive moment: the fresh ones become reserved slots, and
//! whatever was handed out and never committed is released. Until a spawn
//! commits it, a reserved id names nothing alive — and it is never the id of
//! another entity, live, reserved, or the one a reference to nothing names.
//!
//! [`EntityReserver`]: khora_core::ecs::EntityReserver

use std::collections::HashSet;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{Transform, World};

/// A world that has lived a little: five entities spawned, two despawned, a
/// reference to nothing made, and one frame's reservations settled — so the
/// reserver hands out recycled indices as well as fresh ones.
fn lived_in() -> (World, Vec<EntityId>) {
    let mut world = World::new();
    let spawned: Vec<EntityId> = (0..5).map(|_| world.spawn(Transform::identity())).collect();
    assert!(world.despawn(spawned[1]));
    assert!(world.despawn(spawned[3]));
    world.nowhere();
    world.settle_reservations();
    world.release_unspent();
    let live = vec![spawned[0], spawned[2], spawned[4]];
    (world, live)
}

/// **A reservation is an id of its own.** However many are taken, each is
/// distinct, none is a live entity's or the one a reference to nothing names,
/// and none is alive — before settlement, after it, and once the frame
/// released what it did not spend.
#[test]
fn reserved_ids_are_distinct_and_none_is_alive() {
    let (mut world, live) = lived_in();
    let nowhere = world.nowhere_entity().expect("the world made one");
    let reserver = world.reserver();

    let reserved: Vec<EntityId> = (0..300).map(|_| reserver.reserve()).collect();

    let distinct: HashSet<EntityId> = reserved.iter().copied().collect();
    assert_eq!(distinct.len(), reserved.len(), "an id was handed out twice");
    for id in &reserved {
        assert_ne!(*id, nowhere, "the id a reference to nothing names");
        assert!(!live.contains(id), "{id:?} is a live entity's");
        assert!(!world.contains(*id), "{id:?} is alive before any commit");
    }

    world.settle_reservations();
    assert!(
        reserved.iter().all(|id| !world.contains(*id)),
        "settling brings nothing to life"
    );
    world.release_unspent();
    assert!(
        reserved.iter().all(|id| !world.contains(*id)),
        "releasing brings nothing to life"
    );
    assert_eq!(world.entity_count(), live.len());
    assert_eq!(world.nowhere_entity(), Some(nowhere));
}

/// A fresh world reserves too, past any id it has ever handed out, and never
/// runs dry: thousands taken in one frame, none failing, none repeated.
#[test]
fn a_reserver_never_runs_dry() {
    let mut world = World::new();
    let reserver = world.reserver();

    let reserved: HashSet<EntityId> = (0..5000).map(|_| reserver.reserve()).collect();

    assert_eq!(reserved.len(), 5000);
    world.settle_reservations();
    world.release_unspent();
    assert_eq!(world.entity_count(), 0, "a reservation is not an entity");
}

/// **Reserving takes no lock**, so several threads reserve at once — and
/// every id they take is distinct.
#[test]
fn concurrent_reservations_are_all_distinct() {
    const THREADS: usize = 8;
    const EACH: usize = 2000;
    let (mut world, live) = lived_in();
    let nowhere = world.nowhere_entity();
    let reserver = world.reserver();

    let taken: Vec<Vec<EntityId>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let reserver = Arc::clone(&reserver);
                scope.spawn(move || (0..EACH).map(|_| reserver.reserve()).collect::<Vec<_>>())
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a reserving thread panicked"))
            .collect()
    });

    let all: Vec<EntityId> = taken.into_iter().flatten().collect();
    let distinct: HashSet<EntityId> = all.iter().copied().collect();
    assert_eq!(distinct.len(), THREADS * EACH, "an id was handed out twice");
    assert!(all
        .iter()
        .all(|id| Some(*id) != nowhere && !live.contains(id)));

    world.settle_reservations();
    world.release_unspent();
    assert!(all.iter().all(|id| !world.contains(*id)));
    assert_eq!(world.entity_count(), live.len());
}

/// A world has one reserver, its own: asked twice it is the same one, and two
/// worlds never share one.
#[test]
fn the_reserver_is_the_worlds_own() {
    let world = World::new();
    let other = World::new();

    assert!(Arc::ptr_eq(&world.reserver(), &world.reserver()));
    assert!(!Arc::ptr_eq(&world.reserver(), &other.reserver()));
}

/// **A released id never names another entity.** An id reserved and never
/// spent is released at the end of its frame; when its index is handed out
/// again — to a reservation or a spawn — it comes with a newer generation.
#[test]
fn a_released_index_comes_back_with_a_newer_generation() {
    let (mut world, _) = lived_in();
    let released = world.reserver().reserve();
    world.settle_reservations();
    world.release_unspent();
    assert!(!world.contains(released));

    let reserver = world.reserver();
    let mut later: Vec<EntityId> = (0..1024).map(|_| reserver.reserve()).collect();
    world.settle_reservations();
    later.extend((0..1024).map(|_| world.spawn(Transform::identity())));

    let reused: Vec<&EntityId> = later
        .iter()
        .filter(|id| id.index == released.index)
        .collect();
    assert!(
        !reused.is_empty(),
        "the released index {} was handed out again",
        released.index
    );
    for id in reused {
        assert!(
            id.generation > released.generation,
            "{id:?} reuses {released:?} without a newer generation"
        );
    }
}

/// **A spawn made directly, between a reservation and its settlement, takes
/// no reserved id**: the fresh indices a reserver counted past the end of the
/// store are held, even before they are slots.
#[test]
fn a_direct_spawn_before_settlement_takes_no_reserved_id() {
    let mut world = World::new();
    let reserver = world.reserver();
    let reserved: HashSet<EntityId> = (0..200).map(|_| reserver.reserve()).collect();

    let spawned: Vec<EntityId> = (0..200)
        .map(|_| world.spawn(Transform::identity()))
        .collect();
    world.settle_reservations();

    for id in &spawned {
        assert!(!reserved.contains(id), "{id:?} was also reserved");
        assert!(
            reserved.iter().all(|held| held.index != id.index),
            "{id:?} shares an index with a reservation still held"
        );
        assert!(world.contains(*id));
    }
    assert_eq!(world.entity_count(), spawned.len());
}

/// **Spawning through the reserver reuses what was despawned.** A game that
/// spawns and despawns the same number of entities every frame — bullets —
/// holds that many alive at most, so the indices it is handed stay within a
/// bound however long it plays: a despawned index comes back to the reserver,
/// and the store does not grow by a frame's spawns every frame.
#[test]
fn a_steady_spawn_and_despawn_rate_keeps_the_indices_bounded() {
    const PER_FRAME: usize = 64;
    const FRAMES: usize = 200;
    let mut world = World::new();
    let reserver = world.reserver();
    let mut highest = 0;

    for _ in 0..FRAMES {
        let reserved: Vec<EntityId> = (0..PER_FRAME).map(|_| reserver.reserve()).collect();
        world.settle_reservations();
        for &id in &reserved {
            assert!(world.spawn_at(id, Transform::identity()));
        }
        world.release_unspent();
        for &id in &reserved {
            assert!(world.despawn(id));
        }
        highest = highest.max(reserved.iter().map(|id| id.index).max().unwrap_or(0));
    }

    assert!(
        (highest as usize) < 4 * PER_FRAME,
        "{FRAMES} frames of {PER_FRAME} spawns and despawns reached index {highest}: \
         the store grew instead of reusing what was despawned"
    );
}
