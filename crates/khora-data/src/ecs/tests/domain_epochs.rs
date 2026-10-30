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

//! The epoch of a domain, made exact: `World::domain_epoch(domain)` is the
//! latest tick at which a value of `domain` was written or a row of a page
//! holding its columns changed place.
//!
//! It is raised to the tick a change carries wherever a column's summary σ
//! rises (a write, a pushed row) or a page's order version ω moves (a
//! despawn, a migration, a removal, a compaction); it stays when writable
//! access is handed out and nothing is written, when only another domain
//! changes, and when nothing happens. It never goes down and never runs
//! ahead of the clock. A compaction changes the order of rows, never a value.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::page_ticks::location;
use super::runtime_components::{declare, field, slot};
use super::{Position, RenderId, RenderTag, Velocity};
use crate::ecs::{Changed, ComponentKey, FieldKind, SemanticDomain, Tick, World};

/// Every domain, in `SemanticDomain::index` order.
const DOMAINS: [SemanticDomain; 6] = [
    SemanticDomain::Spatial,
    SemanticDomain::Render,
    SemanticDomain::Audio,
    SemanticDomain::Physics,
    SemanticDomain::Ui,
    SemanticDomain::Script,
];

/// Every domain's epoch, in `DOMAINS` order.
fn epochs(world: &World) -> Vec<u64> {
    DOMAINS
        .iter()
        .map(|domain| world.domain_epoch(*domain))
        .collect()
}

/// Every domain's epoch but `domain`'s.
fn others(world: &World, domain: SemanticDomain) -> Vec<u64> {
    DOMAINS
        .iter()
        .filter(|other| **other != domain)
        .map(|other| world.domain_epoch(*other))
        .collect()
}

/// A world storing `Position` and `Velocity` (Spatial), `RenderId` and
/// `RenderTag` (Render); `Position` and `RenderId` are watched, so their
/// columns keep row ticks, the other two do not.
fn world() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world.register_component::<RenderTag>(SemanticDomain::Render);
    world.observe(ComponentKey::of::<Position>());
    world.observe(ComponentKey::of::<RenderId>());
    world
}

/// Ends the run in progress: whatever is written next carries a later tick
/// than everything written so far.
fn next_run(world: &mut World) {
    world.begin_run();
    world.end_run();
}

/// `domain`'s epoch rose past `before`, to the tick the change just made
/// carried — the clock's, since nothing ran in between.
#[track_caller]
fn assert_raised(world: &World, domain: SemanticDomain, before: u64, what: &str) {
    let epoch = world.domain_epoch(domain);
    assert!(
        epoch > before,
        "{what}: the {domain:?} epoch must rise past {before}, it reads {epoch}"
    );
    assert_eq!(
        epoch,
        world.current_tick().0,
        "{what}: the {domain:?} epoch is the tick the change carried"
    );
}

/// The same operations on two worlds give the same epochs: an epoch is a tick
/// of the world's own clock, never a count of accesses nor anything a map's
/// iteration order or the world's instance could change.
#[test]
fn domain_epoch_is_the_same_for_two_worlds_built_alike() {
    let build = || {
        let mut world = world();
        let a = world.spawn((Position(1), Velocity(2)));
        let b = world.spawn(Position(3));
        world.add_component(a, RenderId(4)).expect("add");
        world.add_component(b, RenderTag).expect("add");
        next_run(&mut world);
        world.get_mut::<Position>(b).expect("a Position").0 = 30;
        world
    };
    let (first, second) = (build(), build());

    assert_eq!(epochs(&first), epochs(&second));
}

/// **Handing out `&mut` is not a write.** `get_mut`, `query_mut`,
/// `get_many_mut` and `row_mut` taken and only read, a read-only query, and
/// runs that touch nothing leave every domain's epoch as it was.
#[test]
fn domain_epoch_stays_when_nothing_is_written() {
    let mut world = world();
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Spatial,
        vec![field("weight", FieldKind::Float, ScriptValue::Float(0.5))],
    );
    let a = world.spawn((Position(1), Velocity(2)));
    let b = world.spawn((Position(3), Velocity(4)));
    world.add_component(a, RenderId(5)).expect("add");
    world
        .add_runtime_component(a, mark, &ScriptValue::Unit)
        .expect("Mark attaches");
    let weight = slot(&world, mark, "weight").expect("a slot");
    // The clock moves past the setup, so a stamp made by any access below
    // would carry a later tick than every row it finds.
    next_run(&mut world);
    let before = epochs(&world);
    assert_eq!(epochs(&world), before, "reading the epoch changes nothing");

    {
        let position = world.get_mut::<Position>(a).expect("a Position");
        assert_eq!(position.0, 1);
    }
    assert_eq!(epochs(&world), before, "`get_mut` only read");

    let sum: i32 = world
        .query_mut::<(&mut Position, &mut Velocity, Option<&mut RenderId>)>()
        .map(|(p, v, r)| p.0 + v.0 + r.map_or(0, |r| r.0))
        .sum();
    assert_eq!(sum, 1 + 2 + 5 + 3 + 4);
    assert_eq!(epochs(&world), before, "`query_mut` only read");

    let [first, second] = world.get_many_mut::<Position, 2>([a, b]);
    assert_eq!(first.map(|p| p.0), Some(1));
    assert_eq!(second.map(|p| p.0), Some(3));
    assert_eq!(epochs(&world), before, "`get_many_mut` only read");

    {
        let row = world.row_mut(a, mark).expect("a holds Mark");
        assert_eq!(row.field(weight), Some(ScriptValue::Float(0.5)));
    }
    assert_eq!(epochs(&world), before, "`row_mut` only read");

    assert_eq!(world.query::<&Position>().count(), 2);
    assert_eq!(epochs(&world), before, "a read-only query");

    next_run(&mut world);
    next_run(&mut world);
    assert_eq!(epochs(&world), before, "runs that do nothing");
}

/// A write raises its domain's epoch to the write's tick, and no other
/// domain's — whichever path made it.
#[test]
fn domain_epoch_moves_on_a_write_of_its_domain_only() {
    let mut world = world();
    let a = world.spawn((Position(1), Velocity(2)));
    let b = world.spawn((Position(3), Velocity(4)));
    world.add_component(a, RenderId(5)).expect("add");

    // Through a `get_mut` guard.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    world.get_mut::<Position>(a).expect("a Position").0 = 10;
    assert_raised(
        &world,
        SemanticDomain::Spatial,
        spatial,
        "a `get_mut` write",
    );
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // Through a `query_mut` guard, of an unwatched column.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    for mut velocity in world.query_mut::<&mut Velocity>() {
        velocity.0 += 1;
    }
    assert_raised(
        &world,
        SemanticDomain::Spatial,
        spatial,
        "a `query_mut` write",
    );
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // Through `get_many_mut`, one row of two.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    let [_, second] = world.get_many_mut::<Position, 2>([a, b]);
    second.expect("b holds a Position").0 = 30;
    assert_raised(
        &world,
        SemanticDomain::Spatial,
        spatial,
        "a `get_many_mut` write",
    );
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // By value, in another domain.
    next_run(&mut world);
    let (render, rest) = (
        world.domain_epoch(SemanticDomain::Render),
        others(&world, SemanticDomain::Render),
    );
    assert!(world.set_component(a, RenderId(50)));
    assert_raised(&world, SemanticDomain::Render, render, "`set_component`");
    assert_eq!(others(&world, SemanticDomain::Render), rest);

    // Through a `get_mut` guard, in that other domain.
    next_run(&mut world);
    let (render, rest) = (
        world.domain_epoch(SemanticDomain::Render),
        others(&world, SemanticDomain::Render),
    );
    world.get_mut::<RenderId>(a).expect("a RenderId").0 = 51;
    assert_raised(
        &world,
        SemanticDomain::Render,
        render,
        "a Render `get_mut` write",
    );
    assert_eq!(others(&world, SemanticDomain::Render), rest);
}

/// A run-time component's write through `row_mut` raises its domain's epoch
/// alone, to the write's tick.
#[test]
fn domain_epoch_moves_on_a_runtime_field_write_of_its_domain_only() {
    let mut world = world();
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Physics,
        vec![field("weight", FieldKind::Float, ScriptValue::Float(0.5))],
    );
    let a = world.spawn(Position(1));
    world
        .add_runtime_component(a, mark, &ScriptValue::Unit)
        .expect("Mark attaches");
    let weight = slot(&world, mark, "weight").expect("a slot");
    next_run(&mut world);
    let (physics, rest) = (
        world.domain_epoch(SemanticDomain::Physics),
        others(&world, SemanticDomain::Physics),
    );

    world
        .row_mut(a, mark)
        .expect("a holds Mark")
        .set_field(weight, &ScriptValue::Float(2.0))
        .expect("a float fits");

    assert_raised(&world, SemanticDomain::Physics, physics, "`set_field`");
    assert_eq!(others(&world, SemanticDomain::Physics), rest);
}

/// Rows that arrive, leave or change page raise their domain's epoch to the
/// tick they moved at: a spawn into a page that already holds rows, a
/// despawn, a migration and a removal.
#[test]
fn domain_epoch_moves_on_spawn_despawn_and_migration() {
    let mut world = world();
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));

    // A row pushed onto a page that exists.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    let c = world.spawn(Position(3));
    assert_eq!(
        location(&world, c, SemanticDomain::Spatial).page_id,
        location(&world, a, SemanticDomain::Spatial).page_id,
        "the spawn lands on the page already holding Positions"
    );
    assert_raised(&world, SemanticDomain::Spatial, spatial, "a pushed row");
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // A row removed, in the same run as nothing else.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    assert!(world.despawn(a));
    assert_raised(&world, SemanticDomain::Spatial, spatial, "a despawn");
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // A row moved to another page of its domain: a migration.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    let from = location(&world, b, SemanticDomain::Spatial).page_id;
    world.add_component(b, Velocity(20)).expect("add");
    assert_ne!(
        location(&world, b, SemanticDomain::Spatial).page_id,
        from,
        "b migrated"
    );
    assert_raised(&world, SemanticDomain::Spatial, spatial, "a migration");
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // A second migration onto the page `b` now lives in: a row arrives on a
    // page that exists, a row is left behind on another.
    next_run(&mut world);
    let spatial = world.domain_epoch(SemanticDomain::Spatial);
    world.add_component(c, Velocity(30)).expect("add");
    assert_eq!(
        location(&world, c, SemanticDomain::Spatial).page_id,
        location(&world, b, SemanticDomain::Spatial).page_id,
        "c joins b's page"
    );
    assert_raised(
        &world,
        SemanticDomain::Spatial,
        spatial,
        "a migration onto a page",
    );

    // A component removed: its row is left behind.
    next_run(&mut world);
    let (spatial, rest) = (
        world.domain_epoch(SemanticDomain::Spatial),
        others(&world, SemanticDomain::Spatial),
    );
    world.remove_component::<Velocity>(c).expect("remove");
    assert_raised(&world, SemanticDomain::Spatial, spatial, "a removal");
    assert_eq!(others(&world, SemanticDomain::Spatial), rest);

    // A spawn of another domain moves that domain alone.
    next_run(&mut world);
    let (render, rest) = (
        world.domain_epoch(SemanticDomain::Render),
        others(&world, SemanticDomain::Render),
    );
    world.spawn(RenderId(9));
    assert_raised(&world, SemanticDomain::Render, render, "a Render spawn");
    assert_eq!(others(&world, SemanticDomain::Render), rest);
}

/// The summary σ of every column of every page, keyed by page and key.
fn summaries(world: &World) -> Vec<(usize, ComponentKey, Tick)> {
    let mut all: Vec<(usize, ComponentKey, Tick)> = world
        .storage
        .pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| {
            page.ticks
                .iter()
                .map(move |(key, column)| (index, *key, column.summary))
        })
        .collect();
    all.sort_by_key(|(index, key, _)| (*index, *key));
    all
}

/// The entities `(EntityId, Changed<T>)` yields since `since`, sorted.
fn changed<T: crate::ecs::Component>(world: &World, since: Tick) -> Vec<EntityId> {
    let mut entities: Vec<EntityId> = world
        .query::<(EntityId, Changed<T>)>()
        .since(since)
        .map(|(entity, ())| entity)
        .collect();
    entities.sort_by_key(|entity| entity.index);
    entities
}

/// **A compaction changes order, not values.** Reclaiming a row left behind
/// moves the surviving rows within their page: the page's order version ω
/// moves and its domain's epoch rises to the compaction's tick, no column's
/// summary σ does, and a reader
/// that has seen everything up to the compaction finds nothing changed —
/// whether its column keeps row ticks (`RenderId`) or not (`Velocity`).
#[test]
fn compaction_changes_order_not_values() {
    let mut world = world();
    // Three rows on a Render page and three on a Spatial page; the first of
    // each migrates away, leaving its row behind at the front, so reclaiming
    // it moves the page's last row into it.
    let renders: Vec<EntityId> = (1..=3).map(|i| world.spawn(RenderId(i))).collect();
    let spatials: Vec<EntityId> = (1..=3).map(|i| world.spawn(Velocity(i))).collect();
    let render_page = location(&world, renders[0], SemanticDomain::Render).page_id;
    let spatial_page = location(&world, spatials[0], SemanticDomain::Spatial).page_id;
    world.add_component(renders[0], RenderTag).expect("add");
    world.add_component(spatials[0], Position(1)).expect("add");
    assert_ne!(
        location(&world, renders[0], SemanticDomain::Render).page_id,
        render_page
    );
    assert_ne!(
        location(&world, spatials[0], SemanticDomain::Spatial).page_id,
        spatial_page
    );

    // Everything so far has been seen at `seen`; the compaction runs later.
    let seen = world.current_tick();
    next_run(&mut world);
    let order = (
        world.storage.pages[render_page as usize].order(),
        world.storage.pages[spatial_page as usize].order(),
    );
    let sums = summaries(&world);
    let (render, spatial) = (
        world.domain_epoch(SemanticDomain::Render),
        world.domain_epoch(SemanticDomain::Spatial),
    );
    let rest: Vec<u64> = DOMAINS[2..]
        .iter()
        .map(|d| world.domain_epoch(*d))
        .collect();

    assert!(
        world.run_compaction(16) >= 2,
        "both dirty pages are compacted"
    );
    assert_eq!(
        location(&world, renders[2], SemanticDomain::Render),
        crate::ecs::PageIndex {
            page_id: render_page,
            row_index: 0,
        },
        "the last Render row moved into the reclaimed one"
    );
    assert_eq!(
        location(&world, spatials[2], SemanticDomain::Spatial),
        crate::ecs::PageIndex {
            page_id: spatial_page,
            row_index: 0,
        },
        "the last Spatial row moved into the reclaimed one"
    );

    assert_ne!(
        world.storage.pages[render_page as usize].order(),
        order.0,
        "the Render page's rows changed place"
    );
    assert_ne!(
        world.storage.pages[spatial_page as usize].order(),
        order.1,
        "the Spatial page's rows changed place"
    );
    assert_eq!(summaries(&world), sums, "no column was written");
    assert_raised(&world, SemanticDomain::Render, render, "a compaction");
    assert_raised(&world, SemanticDomain::Spatial, spatial, "a compaction");
    assert_eq!(
        DOMAINS[2..]
            .iter()
            .map(|d| world.domain_epoch(*d))
            .collect::<Vec<_>>(),
        rest,
        "a domain with no compacted page keeps its epoch"
    );
    assert!(
        changed::<RenderId>(&world, seen).is_empty(),
        "a watched column carries its rows' ticks through the compaction"
    );
    assert!(
        changed::<Velocity>(&world, seen).is_empty(),
        "an unwatched column's summary is not raised by the compaction"
    );
    for (i, entity) in renders.iter().enumerate() {
        assert_eq!(
            world.get::<RenderId>(*entity).copied(),
            Some(RenderId(i as i32 + 1))
        );
    }
    for (i, entity) in spatials.iter().enumerate() {
        assert_eq!(
            world.get::<Velocity>(*entity).copied(),
            Some(Velocity(i as i32 + 1))
        );
    }
}

/// **An epoch never goes down, nor runs ahead of the clock.** Over a
/// sequence of writes, spawns, despawns, migrations, removals, accesses that
/// write nothing, compactions and row-tick frees, run after run, every
/// domain's epoch is at least what it was and at most the current tick.
#[test]
fn domain_epoch_never_goes_down() {
    let mut world = world();
    let mut last = epochs(&world);
    let mut check = |world: &World, step: &str| {
        let now = epochs(world);
        for ((domain, before), after) in DOMAINS.iter().zip(&last).zip(&now) {
            assert!(
                after >= before,
                "{step}: the {domain:?} epoch went down, {before} -> {after}"
            );
            assert!(
                *after <= world.current_tick().0,
                "{step}: the {domain:?} epoch {after} is past the clock {:?}",
                world.current_tick()
            );
        }
        last = now;
    };

    let mut alive: Vec<EntityId> = Vec::new();
    for step in 0..60_i32 {
        match step % 8 {
            0 => alive.push(world.spawn((Position(step), RenderId(step)))),
            1 => alive.push(world.spawn(RenderId(step))),
            2 => {
                if let Some(&entity) = alive.first() {
                    if let Some(mut position) = world.get_mut::<Position>(entity) {
                        position.0 += 1;
                    }
                }
            }
            3 => {
                if let Some(&entity) = alive.last() {
                    let _ = world.add_component(entity, RenderTag);
                }
            }
            4 => {
                let _ = world.query_mut::<&mut Position>().count();
                let _ = world.query_mut::<&mut RenderId>().count();
            }
            5 => {
                if alive.len() > 2 {
                    let entity = alive.remove(1);
                    world.despawn(entity);
                }
            }
            6 => {
                if let Some(&entity) = alive.first() {
                    let _ = world.remove_component::<Velocity>(entity);
                }
                world.run_compaction(16);
            }
            _ => {
                world.free_quiet_row_ticks(0);
            }
        }
        check(&world, &format!("step {step}"));
        next_run(&mut world);
        check(&world, &format!("after the run of step {step}"));
    }
}

/// The entities `query::<(EntityId, &RenderId)>` yields, in its order.
fn render_order(world: &World) -> Vec<EntityId> {
    world
        .query::<(EntityId, &RenderId)>()
        .map(|(entity, _)| entity)
        .collect()
}

/// **A migration moves every domain its row carries.** A mixed bundle's row
/// holds the columns of two domains in one page; a component added in one of
/// them moves the whole row to another page, so the other domain's rows change
/// place — and order — too. Its epoch must rise with them, or a view keyed on
/// it is served in the old order.
#[test]
fn a_migration_raises_the_epoch_of_every_domain_its_row_carries() {
    let mut world = world();
    let a = world.spawn((Position(1), RenderId(1)));
    let b = world.spawn(RenderId(2));
    next_run(&mut world);
    let (order, render) = (
        render_order(&world),
        world.domain_epoch(SemanticDomain::Render),
    );
    let from = location(&world, a, SemanticDomain::Render).page_id;

    // A Spatial component: the row holding a's RenderId moves with it.
    world.add_component(a, Velocity(3)).expect("add");

    assert_ne!(
        location(&world, a, SemanticDomain::Render).page_id,
        from,
        "a's Render row changed page"
    );
    assert_ne!(render_order(&world), order, "the Render rows changed order");
    assert_eq!(order, vec![a, b]);
    assert!(
        world.domain_epoch(SemanticDomain::Render) > render,
        "the Render rows changed order, its epoch must rise past {render}, it reads {}",
        world.domain_epoch(SemanticDomain::Render)
    );
}

/// **A removal moves every domain its row carries**, as an addition does.
#[test]
fn a_removal_raises_the_epoch_of_every_domain_its_row_carries() {
    let mut world = world();
    let a = world.spawn((Position(1), Velocity(1), RenderId(1)));
    let b = world.spawn(RenderId(2));
    next_run(&mut world);
    let (order, render) = (
        render_order(&world),
        world.domain_epoch(SemanticDomain::Render),
    );

    // A Spatial component: the row holding a's RenderId moves with it.
    world.remove_component::<Velocity>(a).expect("remove");

    assert_ne!(render_order(&world), order, "the Render rows changed order");
    assert_eq!(order, vec![a, b]);
    assert!(
        world.domain_epoch(SemanticDomain::Render) > render,
        "the Render rows changed order, its epoch must rise past {render}, it reads {}",
        world.domain_epoch(SemanticDomain::Render)
    );
}

/// **A despawn moves every domain whose rows its swap-remove moves.** An
/// entity that left the Render domain (`remove_component_domain`) still holds
/// its row of a mixed page for the Spatial one; despawning it swap-removes
/// that row, and the page's last row — a live Render row of another entity —
/// takes its place.
#[test]
fn a_despawn_raises_the_epoch_of_every_domain_whose_rows_it_moves() {
    let mut world = world();
    let a = world.spawn((Position(1), RenderId(1)));
    let b = world.spawn((Position(2), RenderId(2)));
    let c = world.spawn((Position(3), RenderId(3)));
    world
        .remove_component_domain::<RenderId>(a)
        .expect("a held Render components");
    next_run(&mut world);
    let (order, render) = (
        render_order(&world),
        world.domain_epoch(SemanticDomain::Render),
    );

    assert!(world.despawn(a));

    assert_eq!(order, vec![b, c]);
    assert_ne!(render_order(&world), order, "the Render rows changed order");
    assert!(
        world.domain_epoch(SemanticDomain::Render) > render,
        "the Render rows changed order, its epoch must rise past {render}, it reads {}",
        world.domain_epoch(SemanticDomain::Render)
    );
}

/// A write through the optional term of a joined `query_mut` — a component of
/// another domain than the driver's — raises that component's domain.
#[test]
fn a_write_through_a_joined_optional_term_raises_its_own_domain() {
    let mut world = world();
    let a = world.spawn(Position(1));
    world.add_component(a, RenderId(1)).expect("add");
    world.spawn(Position(2));
    next_run(&mut world);
    let (render, spatial) = (
        world.domain_epoch(SemanticDomain::Render),
        world.domain_epoch(SemanticDomain::Spatial),
    );

    for (_, render_id) in world.query_mut::<(&Position, Option<&mut RenderId>)>() {
        if let Some(mut render_id) = render_id {
            render_id.0 = 10;
        }
    }

    assert_eq!(world.get::<RenderId>(a), Some(&RenderId(10)));
    assert_raised(
        &world,
        SemanticDomain::Render,
        render,
        "an optional term's write",
    );
    assert_eq!(world.domain_epoch(SemanticDomain::Spatial), spatial);
}

/// Guards of two domains held together and of one `get_many_mut`, written in
/// any order, with further `next` calls between the writes: each write raises
/// its own domain (and Miri finds no aliasing violation).
#[test]
fn guards_held_together_raise_their_domains_in_any_order() {
    let mut world = world();
    let a = world.spawn(Position(1));
    world.add_component(a, RenderId(1)).expect("add");
    let b = world.spawn(Position(2));
    world.add_component(b, RenderId(2)).expect("add");
    let c = world.spawn(Position(3));
    world.add_component(c, RenderId(3)).expect("add");
    next_run(&mut world);
    let render = world.domain_epoch(SemanticDomain::Render);

    {
        let mut query = world.query_mut::<(&mut Position, &mut RenderId)>();
        let (mut p0, mut r0) = query.next().expect("a row");
        let (mut p1, _unwritten) = query.next().expect("a row");
        r0.0 += 10;
        let third = query.next();
        p1.0 += 10;
        p0.0 += 10;
        if let Some((mut p2, mut r2)) = third {
            r2.0 += 10;
            p2.0 += 10;
        }
        assert!(query.next().is_none());
    }
    assert_raised(
        &world,
        SemanticDomain::Render,
        render,
        "a joined query's guard",
    );

    next_run(&mut world);
    let spatial = world.domain_epoch(SemanticDomain::Spatial);
    {
        let [first, second, third] = world.get_many_mut::<Position, 3>([a, b, c]);
        let (mut first, mut second, mut third) =
            (first.expect("a"), second.expect("b"), third.expect("c"));
        third.0 += 1;
        first.0 += 1;
        second.0 += 1;
    }
    assert_raised(
        &world,
        SemanticDomain::Spatial,
        spatial,
        "`get_many_mut` guards",
    );
    assert_eq!(world.get::<Position>(a), Some(&Position(12)));
}

/// What a reader of `domain` sees: every row of its components, in the order
/// queries yield them.
fn seen(world: &World, domain: SemanticDomain, mark: ComponentKey) -> String {
    let declared = if world.components().vtable(mark).map(|v| v.domain) == Some(domain) {
        format!("{:?}", world.declared_rows(mark))
    } else {
        String::new()
    };
    declared
        + &match domain {
            SemanticDomain::Spatial => format!(
                "{:?} {:?}",
                world
                    .query::<(EntityId, &Position)>()
                    .map(|(e, p)| (e, *p))
                    .collect::<Vec<_>>(),
                world
                    .query::<(EntityId, &Velocity)>()
                    .map(|(e, v)| (e, *v))
                    .collect::<Vec<_>>()
            ),
            SemanticDomain::Render => format!(
                "{:?} {:?}",
                world
                    .query::<(EntityId, &RenderId)>()
                    .map(|(e, r)| (e, *r))
                    .collect::<Vec<_>>(),
                world
                    .query::<(EntityId, &RenderTag)>()
                    .map(|(e, _)| e)
                    .collect::<Vec<_>>()
            ),
            _ => String::new(),
        }
}

/// A small deterministic generator.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % below.max(1)
    }
}

/// **Equal epochs mean an unchanged domain**, over random sequences of every
/// write and move a world knows: a reader of a domain that runs at a tick of
/// its own and finds the epoch it last saw finds the rows it last saw, in the
/// same order.
#[test]
fn equal_epochs_mean_an_unchanged_domain() {
    let checked = [
        SemanticDomain::Spatial,
        SemanticDomain::Render,
        SemanticDomain::Physics,
    ];
    for seed in 0..64_u64 {
        let mut rng = Lcg(seed);
        let mut world = world();
        let mark = declare(
            &mut world,
            "Mark",
            SemanticDomain::Physics,
            vec![field("weight", FieldKind::Int, ScriptValue::Int(0))],
        );
        let mut alive: Vec<EntityId> = Vec::new();
        let mut last: Vec<(u64, String)> = checked
            .iter()
            .map(|d| (world.domain_epoch(*d), seen(&world, *d, mark)))
            .collect();
        let mut log: Vec<String> = Vec::new();
        for step in 0..200_i32 {
            let pick =
                |rng: &mut Lcg, alive: &[EntityId]| alive.get(rng.next(alive.len())).copied();
            let op = rng.next(16);
            let target = pick(&mut rng, &alive);
            let what = format!("{op} on {target:?}");
            match op {
                0 => alive.push(world.spawn((Position(step), RenderId(step)))),
                1 => alive.push(world.spawn(Position(step))),
                2 => alive.push(world.spawn((RenderId(step), RenderTag))),
                3 => {
                    if let Some(e) = target {
                        if let Some(mut p) = world.get_mut::<Position>(e) {
                            p.0 += 1;
                        }
                    }
                }
                4 => {
                    if let Some(e) = target {
                        world.set_component(e, RenderId(step));
                    }
                }
                5 => {
                    if let Some(e) = target {
                        let _ = world.add_component(e, Velocity(step));
                    }
                }
                6 => {
                    if let Some(e) = target {
                        let _ = world.remove_component::<Velocity>(e);
                    }
                }
                7 => {
                    if let Some(e) = target {
                        let _ = world.add_component(e, RenderTag);
                    }
                }
                8 => {
                    if let Some(e) = target {
                        world.despawn(e);
                        alive.retain(|a| *a != e);
                    }
                }
                9 => {
                    world.run_compaction(64);
                }
                10 => {
                    for (mut p, r) in world.query_mut::<(&mut Position, Option<&mut RenderId>)>() {
                        p.set_if_neq(*p);
                        if let Some(mut r) = r {
                            if step % 3 == 0 {
                                r.0 += 1;
                            }
                        }
                    }
                }
                11 => {
                    if let Some(e) = target {
                        let _ = world.add_runtime_component(
                            e,
                            mark,
                            &ScriptValue::Struct(vec![(
                                "weight".into(),
                                ScriptValue::Int(step as i64),
                            )]),
                        );
                    }
                }
                12 => {
                    if let Some(e) = target {
                        if let Some(slot) = slot(&world, mark, "weight") {
                            if let Some(mut row) = world.row_mut(e, mark) {
                                let _ = row.set_field(slot, &ScriptValue::Int(step as i64 * 7));
                            }
                        }
                    }
                }
                13 => {
                    if let Some(e) = target {
                        let _ = world.remove_component_by_key(e, mark);
                    }
                }
                14 => {
                    world.free_quiet_row_ticks(0);
                }
                _ => {
                    let to = if world.components().vtable(mark).map(|v| v.domain)
                        == Some(SemanticDomain::Physics)
                    {
                        SemanticDomain::Render
                    } else {
                        SemanticDomain::Physics
                    };
                    world
                        .move_component_domain(mark, to)
                        .expect("a declared component moves");
                }
            }
            log.push(what);
            for (i, domain) in checked.iter().enumerate() {
                let now = (world.domain_epoch(*domain), seen(&world, *domain, mark));
                if now.0 == last[i].0 {
                    assert_eq!(
                        now.1,
                        last[i].1,
                        "seed {seed}, step {step}: the {domain:?} epoch stayed at {} but the \
                         domain changed; ops: {:?}",
                        now.0,
                        &log[log.len().saturating_sub(4)..]
                    );
                }
                last[i] = now;
            }
            next_run(&mut world);
        }
    }
}
