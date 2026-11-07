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

//! Entity ids held across frames for a load prepared off the frame.
//!
//! An ordinary reservation is released at the end of its frame. A load
//! prepared on a worker takes longer than a frame, so it reserves under a
//! hold: what it holds is released by no frame, handed to no spawn and to no
//! other reservation, until the load commits it — or gives it back, by being
//! abandoned or dropped.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{Transform, World};

/// One frame of a game spawning through scripts and directly: `count` ids
/// reserved and half of them spawned, `count` entities spawned directly, the
/// first of the directly spawned despawned, then the frame's end. Returns
/// every id the frame handed out, spawned or not.
fn busy_frame(world: &mut World, count: usize) -> Vec<EntityId> {
    let reserver = world.reserver();
    let reserved: Vec<EntityId> = (0..count).map(|_| reserver.reserve()).collect();
    world.settle_reservations();
    for &id in reserved.iter().step_by(2) {
        assert!(world.spawn_at(id, Transform::identity()));
    }
    let spawned: Vec<EntityId> = (0..count)
        .map(|_| world.spawn(Transform::identity()))
        .collect();
    assert!(world.despawn(spawned[0]));
    world.release_unspent();
    reserved.into_iter().chain(spawned).collect()
}

/// Asserts what a hold keeps while it holds:
/// - no id of `others` is a held id (index and generation) — an index may be
///   handed out again once its previous holder died, with a newer
///   generation, and that is no collision;
/// - no held id is alive;
/// - no live entity sits in a held slot, under any generation.
fn assert_untouched(world: &World, held: &[EntityId], others: &[EntityId], when: &str) {
    let held_ids: HashSet<EntityId> = held.iter().copied().collect();
    for id in others {
        assert!(
            !held_ids.contains(id),
            "{when}: the held {id:?} was handed out again"
        );
    }
    for id in held {
        assert!(!world.contains(*id), "{when}: the held {id:?} is alive");
    }
    let held_indices: HashSet<u32> = held.iter().map(|id| id.index).collect();
    for live in world.iter_entities() {
        assert!(
            !held_indices.contains(&live.index),
            "{when}: {live:?} is alive in a held slot"
        );
    }
}

/// **A held id survives the frame's end.** Over several frames of spawning,
/// through scripts and directly, with despawns, no frame releases a held
/// id, and no spawn or reservation is handed its index.
#[test]
fn held_ids_survive_the_frames_end_over_several_frames() {
    let mut world = World::new();
    for _ in 0..4 {
        world.spawn(Transform::identity());
    }
    let hold = world.hold_reservations();
    let held: Vec<EntityId> = (0..8).map(|_| hold.reserve()).collect();
    assert_eq!(
        held.iter().copied().collect::<HashSet<_>>().len(),
        held.len(),
        "a hold handed out an id twice"
    );

    let mut others = Vec::new();
    for frame in 0..6 {
        others.extend(busy_frame(&mut world, 32));
        assert_untouched(&world, &held, &others, &format!("frame {frame}"));
    }
    // Every id here was handed out after the hold took its own, while it
    // held them: none of them can be in a held slot, under any generation.
    let held_indices: HashSet<u32> = held.iter().map(|id| id.index).collect();
    for id in &others {
        assert!(
            !held_indices.contains(&id.index),
            "{id:?} was handed a slot the hold held"
        );
    }
    drop(hold);
}

/// **A dropped hold gives its ids back, at once.** A hundred holds of eight
/// ids, each dropped before the next is taken — with no frame in between,
/// then with one — leave the store as small as one hold needs: the next
/// reservations are handed the ids given back.
#[test]
fn a_dropped_hold_gives_its_ids_back_to_the_next_reservation() {
    let mut world = World::new();
    let mut highest = 0;
    for round in 0..200 {
        let hold = world.hold_reservations();
        let held: Vec<EntityId> = (0..8).map(|_| hold.reserve()).collect();
        highest = highest.max(held.iter().map(|id| id.index).max().unwrap_or(0));
        drop(hold);
        if round >= 100 {
            world.settle_reservations();
            world.release_unspent();
        }
    }
    assert!(
        highest < 16,
        "two hundred dropped holds of eight ids reached index {highest}"
    );
    let spawned = world.spawn(Transform::identity());
    assert!(spawned.index < 16, "{spawned:?}");
    assert_eq!(world.entity_count(), 1, "a held id came to life");
}

/// **A hold reserves on a worker while the world runs its frames.** Ids a
/// worker reserves under a hold, as frames of spawning go by on the world's
/// thread, are never handed to a spawn or released by a frame.
#[test]
fn a_hold_reserving_on_a_worker_is_kept_by_the_worlds_frames() {
    let mut world = World::new();
    let hold = world.hold_reservations();

    let (hold, held, others) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let mut held = Vec::new();
            for _ in 0..50 {
                held.extend((0..8).map(|_| hold.reserve()));
                std::thread::yield_now();
            }
            (hold, held)
        });
        let mut others = Vec::new();
        for _ in 0..20 {
            others.extend(busy_frame(&mut world, 16));
        }
        let (hold, held) = worker.join().expect("the worker panicked");
        (hold, held, others)
    });

    // Two more frames once the worker is done: nothing it held is released.
    let mut later = others;
    later.extend(busy_frame(&mut world, 16));
    later.extend(busy_frame(&mut world, 16));
    assert_eq!(
        held.iter().copied().collect::<HashSet<_>>().len(),
        held.len(),
        "a hold handed out an id twice"
    );
    assert_untouched(&world, &held, &later, "after the worker");
    drop(hold);
}

/// **Every id a hold takes while the world allocates comes to life when the
/// load commits it.** A worker reserves under a hold while the world's
/// thread spawns, reserves, despawns, readies recycled ids and ends frames:
/// the held ids are distinct, none is alive or shares a live entity's
/// index, and every one is committed — the world then holding exactly its
/// own entities and the held ones.
#[test]
fn ids_held_while_the_world_allocates_all_commit() {
    for round in 0..40 {
        let mut world = World::new();
        let mut live: Vec<EntityId> = (0..8).map(|_| world.spawn(Transform::identity())).collect();
        let hold = world.hold_reservations();

        let (hold, held) = std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let mut held = Vec::new();
                for i in 0..400 {
                    held.push(hold.reserve());
                    if i % 7 == 0 {
                        std::thread::yield_now();
                    }
                }
                (hold, held)
            });
            let mut step = 0usize;
            while !worker.is_finished() || step < 60 {
                match step % 6 {
                    0 => live.push(world.spawn(Transform::identity())),
                    1 => {
                        let reserved = world.reserve_entity();
                        assert!(
                            world.spawn_reserved(reserved),
                            "round {round}: {reserved:?}"
                        );
                        live.push(reserved);
                    }
                    2 => {
                        if live.len() > 4 {
                            let gone = live.remove(step % live.len());
                            assert!(world.despawn(gone), "round {round}: {gone:?}");
                        }
                    }
                    3 => world.ready_reservations(3),
                    4 => {
                        let _ = world.reserver().reserve();
                        world.release_unspent();
                    }
                    _ => world.settle_reservations(),
                }
                step += 1;
            }
            worker.join().expect("the worker panicked")
        });

        assert_eq!(
            held.iter().copied().collect::<HashSet<_>>().len(),
            held.len(),
            "round {round}: a hold handed out an id twice"
        );
        let live_indices: HashSet<u32> = live.iter().map(|id| id.index).collect();
        for id in &held {
            assert!(!world.contains(*id), "round {round}: {id:?} is alive");
            assert!(
                !live_indices.contains(&id.index),
                "round {round}: {id:?} shares a live entity's index"
            );
        }
        for id in &held {
            assert!(
                world.commit_held(&hold, *id, true),
                "round {round}: the held {id:?} could not be committed"
            );
        }
        for id in held.iter().chain(&live) {
            assert!(world.contains(*id), "round {round}: {id:?} is not alive");
        }
        assert_eq!(
            world.entity_count(),
            live.len() + held.len(),
            "round {round}"
        );
    }
}

/// The time `count` held ids take to be committed one by one, as a load's
/// commit does, and the time a hold of `count` ids takes to be dropped.
fn commit_and_drop_times(count: usize) -> (std::time::Duration, std::time::Duration) {
    let mut world = World::new();
    let hold = world.hold_reservations();
    let held: Vec<EntityId> = (0..count).map(|_| hold.reserve()).collect();
    let started = std::time::Instant::now();
    for id in &held {
        assert!(world.commit_held(&hold, *id, true), "{id:?}");
    }
    let committed = started.elapsed();

    let hold = world.hold_reservations();
    for _ in 0..count {
        hold.reserve();
    }
    let started = std::time::Instant::now();
    drop(hold);
    (committed, started.elapsed())
}

/// **Committing or giving back a load's ids costs in proportion to how many
/// there are.** A load of four times the entities takes about four times as
/// long to commit, on the frame, and to give back — not sixteen.
#[test]
fn committing_and_giving_back_held_ids_scales_linearly() {
    let (small_commit, small_drop) = commit_and_drop_times(10_000);
    let (large_commit, large_drop) = commit_and_drop_times(40_000);
    let ratio = |large: std::time::Duration, small: std::time::Duration| {
        large.as_secs_f64() / small.as_secs_f64().max(1e-6)
    };
    let commit = ratio(large_commit, small_commit);
    let give_back = ratio(large_drop, small_drop);
    assert!(
        commit < 8.0 && give_back < 8.0,
        "four times the ids took {commit:.1}x as long to commit ({small_commit:?} → \
         {large_commit:?}) and {give_back:.1}x as long to give back ({small_drop:?} → \
         {large_drop:?})"
    );
}

/// **A spawn never takes an id a worker's hold gave back before the world
/// gave it a slot.** A worker reserves fresh ids under holds and drops them
/// — giving the ids back, ready, past the end of the store — while the
/// world's thread spawns and reserves with no slot freed: every entity it
/// spawns or brings to life is alive, and none twice.
#[test]
fn spawns_racing_given_back_fresh_ids_all_live() {
    for round in 0..200 {
        let mut world = World::new();
        let reserver = world.reserver();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let made = std::thread::scope(|scope| {
            let stop = &stop;
            scope.spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let hold = reserver.hold();
                    for _ in 0..3 {
                        hold.reserve();
                    }
                    drop(hold);
                }
            });
            let mut made = Vec::new();
            for step in 0..300 {
                if step % 2 == 0 {
                    made.push(world.spawn(Transform::identity()));
                } else {
                    let reserved = world.reserve_entity();
                    assert!(
                        world.spawn_reserved(reserved),
                        "round {round}: the reserved {reserved:?} could not be brought to life"
                    );
                    made.push(reserved);
                }
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            made
        });
        assert_eq!(
            made.iter().copied().collect::<HashSet<_>>().len(),
            made.len(),
            "round {round}: an id was made twice"
        );
        for id in &made {
            assert!(world.contains(*id), "round {round}: {id:?} is not alive");
        }
        assert_eq!(world.entity_count(), made.len(), "round {round}");
    }
}
