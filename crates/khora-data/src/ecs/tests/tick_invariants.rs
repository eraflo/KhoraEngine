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

//! The page's ticks never lose a write, whatever happens to the rows.
//!
//! Random sequences of spawns, despawns, components added and removed,
//! compactions, clock runs and keys newly watched, checked after every
//! operation against a model that remembers, for each live component, the
//! tick it was last placed at — `last(c, r)`:
//!
//! - **I1** `last(c, r) ≤ σ(c)`;
//! - **I2** `last(c, r) ≤ max(τ(c, r), φ(c))`, τ read as 0 when the column
//!   keeps no row ticks;
//!
//! and beside them what the moves promise: a column's tick array has one
//! tick per row, no summary is ahead of the clock, a despawn or a compaction
//! raises no summary, and a page that lost a row moved its order version.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;

use super::page_ticks::{location, page_of, row_tick, ticks};
use super::{Position, RenderId, RenderTag, Velocity};
use crate::ecs::{ComponentKey, SemanticDomain, World};

/// A seeded xorshift: the same seed, the same sequence, every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Never zero: xorshift stays at zero forever.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The components the sequences use: two Spatial, two Render; which of
/// them are watched changes along the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Position,
    Velocity,
    RenderId,
    RenderTag,
}

const PARTS: [Part; 4] = [
    Part::Position,
    Part::Velocity,
    Part::RenderId,
    Part::RenderTag,
];

impl Part {
    fn key(self) -> ComponentKey {
        match self {
            Part::Position => ComponentKey::of::<Position>(),
            Part::Velocity => ComponentKey::of::<Velocity>(),
            Part::RenderId => ComponentKey::of::<RenderId>(),
            Part::RenderTag => ComponentKey::of::<RenderTag>(),
        }
    }

    fn domain(self) -> SemanticDomain {
        match self {
            Part::Position | Part::Velocity => SemanticDomain::Spatial,
            Part::RenderId | Part::RenderTag => SemanticDomain::Render,
        }
    }

    /// Adds this component to `entity`; whether it was added.
    fn add(self, world: &mut World, entity: EntityId) -> bool {
        match self {
            Part::Position => world.add_component(entity, Position(7)).is_ok(),
            Part::Velocity => world.add_component(entity, Velocity(7)).is_ok(),
            Part::RenderId => world.add_component(entity, RenderId(7)).is_ok(),
            Part::RenderTag => world.add_component(entity, RenderTag).is_ok(),
        }
    }

    /// Removes this component from `entity`; whether it was removed.
    fn remove(self, world: &mut World, entity: EntityId) -> bool {
        match self {
            Part::Position => world.remove_component::<Position>(entity).is_ok(),
            Part::Velocity => world.remove_component::<Velocity>(entity).is_ok(),
            Part::RenderId => world.remove_component::<RenderId>(entity).is_ok(),
            Part::RenderTag => world.remove_component::<RenderTag>(entity).is_ok(),
        }
    }
}

/// Spawns bundle number `which` (of 7) on `world`; its entity and parts.
fn spawn_bundle(world: &mut World, which: usize, value: i32) -> (EntityId, Vec<Part>) {
    match which {
        0 => (world.spawn(Position(value)), vec![Part::Position]),
        1 => (world.spawn(Velocity(value)), vec![Part::Velocity]),
        2 => (
            world.spawn((Position(value), Velocity(value))),
            vec![Part::Position, Part::Velocity],
        ),
        3 => (
            world.spawn((Position(value), RenderId(value))),
            vec![Part::Position, Part::RenderId],
        ),
        4 => (world.spawn(RenderId(value)), vec![Part::RenderId]),
        5 => (
            world.spawn((Velocity(value), RenderTag)),
            vec![Part::Velocity, Part::RenderTag],
        ),
        _ => (
            world.spawn((Position(value), Velocity(value), RenderId(value))),
            vec![Part::Position, Part::Velocity, Part::RenderId],
        ),
    }
}

/// What the test remembers: each live entity, and the tick each of its
/// components was last placed at.
type Model = HashMap<EntityId, HashMap<Part, u64>>;

/// I1, I2 and the shape of every page's ticks, after operation `step`.
fn check(world: &World, model: &Model, seed: u64, step: usize, op: &str) {
    let now = world.current_tick();
    for (page_id, page) in world.storage.pages.iter().enumerate() {
        for &key in &page.keys {
            let Some(column) = page.column_ticks(key) else {
                // A page with no row yet may have no ticks yet either.
                assert_eq!(
                    page.row_count(),
                    0,
                    "seed {seed} step {step} ({op}): page {page_id} holds rows but no ticks for {key:?}"
                );
                continue;
            };
            assert!(
                column.summary <= now,
                "seed {seed} step {step} ({op}): page {page_id} {key:?} summary {:?} ahead of the clock {now:?}",
                column.summary
            );
            if let Some(rows) = &column.rows {
                assert_eq!(
                    rows.len(),
                    page.row_count(),
                    "seed {seed} step {step} ({op}): page {page_id} {key:?}: one tick per row"
                );
            }
        }
    }
    for (&entity, parts) in model {
        for (&part, &last) in parts {
            let (page, row) = page_of(world, entity, part.domain());
            assert!(
                page.keys.contains(&part.key()),
                "seed {seed} step {step} ({op}): {entity:?} lost {part:?}"
            );
            let column = ticks(page, part.key());
            assert!(
                last <= column.summary.0,
                "seed {seed} step {step} ({op}): I1 broken for {entity:?} {part:?}: last {last} > σ {:?}",
                column.summary
            );
            let covered = row_tick(column, row).max(column.floor.0);
            assert!(
                last <= covered,
                "seed {seed} step {step} ({op}): I2 broken for {entity:?} {part:?}: last {last} > max(τ {}, φ {:?})",
                row_tick(column, row),
                column.floor
            );
        }
    }
}

/// A page's summaries by key, its order version and its row count.
type PageState = (HashMap<ComponentKey, u64>, u64, usize);

/// Every page's state, by page id.
fn page_state(world: &World) -> Vec<PageState> {
    world
        .storage
        .pages
        .iter()
        .map(|page| {
            let summaries = page
                .keys
                .iter()
                .filter_map(|&key| Some((key, page.column_ticks(key)?.summary.0)))
                .collect();
            (summaries, page.order(), page.row_count())
        })
        .collect()
}

/// A despawn and a compaction move rows and write nothing: no summary of a
/// page that existed before is raised, and a page that lost rows moved its
/// order version.
fn check_a_move_is_not_a_change(
    before: &[PageState],
    world: &World,
    seed: u64,
    step: usize,
    op: &str,
) {
    let after = page_state(world);
    for (page_id, (summaries, order, rows)) in before.iter().enumerate() {
        let Some((summaries_now, order_now, rows_now)) = after.get(page_id) else {
            continue;
        };
        for (key, &summary) in summaries {
            if let Some(&now) = summaries_now.get(key) {
                assert!(
                    now <= summary,
                    "seed {seed} step {step} ({op}): page {page_id} {key:?} summary raised {summary} -> {now} by a move"
                );
            }
        }
        if rows_now < rows {
            assert!(
                order_now > order,
                "seed {seed} step {step} ({op}): page {page_id} lost rows ({rows} -> {rows_now}) but its order stayed {order}"
            );
        }
    }
}

/// One random sequence of `steps` operations from `seed`.
fn run_sequence(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world.register_component::<RenderTag>(SemanticDomain::Render);
    world.observe(ComponentKey::of::<Position>());
    world.observe(ComponentKey::of::<RenderId>());

    let mut model: Model = HashMap::new();
    let mut live: Vec<EntityId> = Vec::new();

    for step in 0..steps {
        let now = world.current_tick().0;
        let op = match rng.below(10) {
            0..=2 => {
                let which = rng.below(7);
                let (entity, parts) = spawn_bundle(&mut world, which, step as i32);
                model.insert(entity, parts.into_iter().map(|p| (p, now)).collect());
                live.push(entity);
                "spawn"
            }
            3 if !live.is_empty() => {
                let entity = live.swap_remove(rng.below(live.len()));
                let before = page_state(&world);
                let mut pages: Vec<u32> = PARTS
                    .iter()
                    .filter(|part| model[&entity].contains_key(part))
                    .map(|part| location(&world, entity, part.domain()).page_id)
                    .collect();
                pages.sort_unstable();
                pages.dedup();
                let orders: Vec<u64> = pages
                    .iter()
                    .map(|&id| world.storage.pages[id as usize].order())
                    .collect();
                assert!(world.despawn(entity));
                model.remove(&entity);
                for (&id, &order) in pages.iter().zip(&orders) {
                    assert!(
                        world.storage.pages[id as usize].order() > order,
                        "seed {seed} step {step}: a despawn moves its page's order version"
                    );
                }
                check_a_move_is_not_a_change(&before, &world, seed, step, "despawn");
                "despawn"
            }
            4 | 5 if !live.is_empty() => {
                let entity = live[rng.below(live.len())];
                let part = PARTS[rng.below(PARTS.len())];
                let had = model[&entity].contains_key(&part);
                let added = part.add(&mut world, entity);
                assert_eq!(added, !had, "seed {seed} step {step}: add {part:?}");
                if let (true, Some(parts)) = (added, model.get_mut(&entity)) {
                    parts.insert(part, now);
                }
                "add"
            }
            6 if !live.is_empty() => {
                let entity = live[rng.below(live.len())];
                let part = PARTS[rng.below(PARTS.len())];
                let had = model[&entity].contains_key(&part);
                let removed = part.remove(&mut world, entity);
                assert_eq!(removed, had, "seed {seed} step {step}: remove {part:?}");
                if let (true, Some(parts)) = (removed, model.get_mut(&entity)) {
                    parts.remove(&part);
                }
                "remove"
            }
            7 => {
                let before = page_state(&world);
                let budget = if rng.below(2) == 0 { 1 } else { usize::MAX };
                world.run_compaction(budget);
                check_a_move_is_not_a_change(&before, &world, seed, step, "compaction");
                "compaction"
            }
            8 => {
                for _ in 0..=rng.below(3) {
                    world.begin_run();
                    world.end_run();
                }
                "runs"
            }
            _ => {
                world.observe(PARTS[rng.below(PARTS.len())].key());
                "observe"
            }
        };
        check(&world, &model, seed, step, op);
    }
}

/// I1 and I2 hold after every operation of many random sequences.
#[test]
fn no_operation_sequence_breaks_i1_or_i2() {
    for seed in 1..=48u64 {
        run_sequence(seed, 250);
    }
}

/// A long sequence, where pages are recycled and columns go from unwatched
/// to watched halfway through their life.
#[test]
fn a_long_operation_sequence_keeps_i1_and_i2() {
    run_sequence(0xC0FF_EE00, 3000);
}
