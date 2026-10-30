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

//! Reading changes: `Changed<T>` keeps the entities whose `T` was written
//! after the query's `since`.
//!
//! Per page of `T`'s column: a page whose summary σ is not past `since` is
//! skipped whole; a column keeping row ticks τ, read at or above its floor
//! φ, keeps exactly the rows whose tick is past `since`; any other column
//! keeps every row. `Changed<T>` requires `T`, yields `()`, and reads alike
//! in a Native, a Transversal and a joined iteration, through `query` and
//! `query_mut`.

use std::any::TypeId;
use std::ops::DerefMut;

use khora_core::ecs::entity::EntityId;

use super::page_ticks::{advance_to, location};
use super::write_stamps::column_of;
use super::{Position, RenderId, Velocity};
use crate::ecs::{Changed, Component, ComponentKey, QueryMode, SemanticDomain, Tick, World};

/// A world storing `Position` and `Velocity` (Spatial) and `RenderId`
/// (Render); `Position` and `RenderId` are watched, `Velocity` is not.
fn world() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world.observe(ComponentKey::of::<Position>());
    world.observe(ComponentKey::of::<RenderId>());
    world
}

/// Sorted, so that two answers compare whatever the page order.
fn sorted(mut entities: Vec<EntityId>) -> Vec<EntityId> {
    entities.sort_by_key(|entity| (entity.index, entity.generation));
    entities
}

/// The entities `(EntityId, Changed<T>)` yields through `query`, since
/// `since`.
fn changed<T: Component>(world: &World, since: u64) -> Vec<EntityId> {
    sorted(
        world
            .query::<(EntityId, Changed<T>)>()
            .since(Tick(since))
            .map(|(entity, ())| entity)
            .collect(),
    )
}

/// The same, through `query_mut`.
fn changed_mut<T: Component>(world: &mut World, since: u64) -> Vec<EntityId> {
    sorted(
        world
            .query_mut::<(EntityId, Changed<T>)>()
            .since(Tick(since))
            .map(|(entity, ())| entity)
            .collect(),
    )
}

/// Writes `value` through a writable item.
fn assign<T>(mut slot: impl DerefMut<Target = T>, value: T) {
    *slot = value;
}

/// Writes `entity`'s `Position` now, through the guard `get_mut` returns.
fn write_position(world: &mut World, entity: EntityId, value: i32) {
    assign(
        world.get_mut::<Position>(entity).expect("a Position"),
        Position(value),
    );
}

// --- the page summary ---

/// **A clean page is not entered.** A page whose `Position` summary is not
/// past `since` is skipped whole — its rows are never read. To see it, the
/// clean page's row ticks are set past `since` while its summary stays at
/// or under it — a state no write can make: a reader that entered the page
/// and read its rows would yield them.
#[test]
fn changed_skips_a_clean_page_without_reading_its_rows() {
    let mut world = world();
    advance_to(&mut world, 3);
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));
    let c = world.spawn((Position(3), Velocity(3)));
    advance_to(&mut world, 10);
    write_position(&mut world, c, 30);

    let clean = location(&world, a, SemanticDomain::Spatial).page_id;
    assert_eq!(
        location(&world, b, SemanticDomain::Spatial).page_id,
        clean,
        "a and b share their page"
    );
    assert_ne!(
        location(&world, c, SemanticDomain::Spatial).page_id,
        clean,
        "c lives in another page"
    );
    let key = ComponentKey::of::<Position>();
    {
        let column = world.storage.pages[clean as usize]
            .ticks
            .get_mut(&key)
            .expect("the clean page has Position ticks");
        assert_eq!(column.summary, Tick(3), "the clean page was written at 3");
        let rows = column
            .rows
            .as_mut()
            .expect("a watched column keeps a tick per row");
        rows.iter_mut().for_each(|tick| *tick = 100);
    }

    assert_eq!(
        changed::<Position>(&world, 3),
        vec![c],
        "since = σ: the clean page is skipped, its row ticks unread"
    );
    assert_eq!(
        changed::<Position>(&world, 5),
        vec![c],
        "since > σ: the clean page is skipped, its row ticks unread"
    );
    assert_eq!(
        changed_mut::<Position>(&mut world, 5),
        vec![c],
        "query_mut skips it alike"
    );
}

// --- exact answers ---

/// **The rows written since, and only those**, across several pages of a
/// watched column, at every `since` — through `query` and `query_mut`.
#[test]
fn changed_returns_exactly_the_rows_written_since() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));
    let c = world.spawn(Position(3));
    let d = world.spawn((Position(4), Velocity(4)));
    let e = world.spawn((Position(5), Velocity(5)));
    advance_to(&mut world, 5);
    write_position(&mut world, b, 20);
    assert!(world.set_component(e, Position(50)));
    advance_to(&mut world, 8);
    for (entity, item) in world.query_mut::<(EntityId, &mut Position)>() {
        if entity == d {
            assign(item, Position(40));
        }
    }
    advance_to(&mut world, 9);

    let everyone = sorted(vec![a, b, c, d, e]);
    let cases: [(u64, Vec<EntityId>); 6] = [
        (1, everyone.clone()),
        (2, sorted(vec![b, d, e])),
        (4, sorted(vec![b, d, e])),
        (5, vec![d]),
        (7, vec![d]),
        (8, vec![]),
    ];
    for (since, expected) in cases {
        assert_eq!(
            changed::<Position>(&world, since),
            expected,
            "query, since {since}"
        );
        assert_eq!(
            changed_mut::<Position>(&mut world, since),
            expected,
            "query_mut, since {since}"
        );
    }
}

/// **A Transversal query reads `Changed` exactly too**: `Changed` on a
/// component of one domain beside a term of the other, in both directions —
/// whichever domain drives, one of the two puts `Changed` in a peer domain.
/// An entity lacking the other term never matches.
#[test]
fn changed_returns_exactly_the_rows_written_since_in_a_transversal_query() {
    let mut world = world();
    assert_eq!(
        world
            .analyze_query(&[TypeId::of::<Position>(), TypeId::of::<RenderId>()])
            .mode,
        QueryMode::Transversal,
        "a Spatial and a Render term make a Transversal query"
    );
    advance_to(&mut world, 2);
    let both: Vec<EntityId> = (0..4)
        .map(|i| world.spawn((Position(i), RenderId(i))))
        .collect();
    let spatial_only = world.spawn(Position(9));
    let render_only = world.spawn(RenderId(9));
    advance_to(&mut world, 6);
    write_position(&mut world, both[1], 10);
    write_position(&mut world, spatial_only, 90);
    assert!(world.set_component(both[2], RenderId(20)));
    assert!(world.set_component(render_only, RenderId(90)));
    advance_to(&mut world, 7);

    let read = |world: &World, since: u64| -> (Vec<EntityId>, Vec<EntityId>) {
        (
            sorted(
                world
                    .query::<(EntityId, &RenderId, Changed<Position>)>()
                    .since(Tick(since))
                    .map(|(entity, _, ())| entity)
                    .collect(),
            ),
            sorted(
                world
                    .query::<(EntityId, &Position, Changed<RenderId>)>()
                    .since(Tick(since))
                    .map(|(entity, _, ())| entity)
                    .collect(),
            ),
        )
    };

    let (position, render) = read(&world, 2);
    assert_eq!(
        position,
        vec![both[1]],
        "Position changed since 2, with a RenderId"
    );
    assert_eq!(
        render,
        vec![both[2]],
        "RenderId changed since 2, with a Position"
    );

    let (position, render) = read(&world, 1);
    assert_eq!(
        position,
        sorted(both.clone()),
        "since 1: every holder of both"
    );
    assert_eq!(
        render,
        sorted(both.clone()),
        "since 1: every holder of both"
    );

    let (position, render) = read(&world, 6);
    assert!(position.is_empty() && render.is_empty(), "since 6: nothing");

    let mutable: Vec<EntityId> = sorted(
        world
            .query_mut::<(EntityId, &mut RenderId, Changed<Position>)>()
            .since(Tick(2))
            .map(|(entity, _, ())| entity)
            .collect(),
    );
    assert_eq!(mutable, vec![both[1]], "query_mut, Transversal, since 2");
}

// --- every row ---

/// **Under the floor, every row.** A column whose history was dropped up to
/// φ cannot tell which of its rows were written before φ, so a reader whose
/// `since` is under φ gets every row of a page written since; at or above φ
/// the row ticks answer exactly. Here `Velocity` is written unwatched up to
/// tick 4, then watched: its next write, at 7, gives the column row ticks,
/// every other row reading 0 under the floor of 4.
#[test]
fn a_reader_under_the_floor_sees_every_row() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Velocity(1));
    let b = world.spawn(Velocity(2));
    let c = world.spawn(Velocity(3));
    let everyone = sorted(vec![a, b, c]);
    advance_to(&mut world, 4);
    assert!(world.set_component(b, Velocity(20)));
    world.observe(ComponentKey::of::<Velocity>());
    advance_to(&mut world, 7);
    assign(
        world.get_mut::<Velocity>(c).expect("a Velocity"),
        Velocity(30),
    );
    let column = column_of(&world, c, ComponentKey::of::<Velocity>());
    assert!(column.rows.is_some(), "a watched column gains row ticks");
    assert_eq!(column.floor, Tick(4), "its history is dropped up to 4");
    assert_eq!(column.summary, Tick(7));

    for since in [0, 1, 3] {
        assert_eq!(
            changed::<Velocity>(&world, since),
            everyone,
            "since {since} < φ: every row"
        );
        assert_eq!(
            changed_mut::<Velocity>(&mut world, since),
            everyone,
            "query_mut, since {since} < φ: every row"
        );
    }
    for since in [4, 5, 6] {
        assert_eq!(
            changed::<Velocity>(&world, since),
            vec![c],
            "since {since} ≥ φ: the row ticks answer"
        );
        assert_eq!(
            changed_mut::<Velocity>(&mut world, since),
            vec![c],
            "query_mut, since {since} ≥ φ: the row ticks answer"
        );
    }
    assert_eq!(changed::<Velocity>(&world, 7), vec![], "since = σ: nothing");
}

/// **A column keeping no row ticks answers with every row** of a page
/// written since — it knows only its summary and its floor — and with none
/// of a page not written since.
#[test]
fn a_column_without_row_ticks_answers_with_every_row_of_a_written_page() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Velocity(1));
    let b = world.spawn(Velocity(2));
    let c = world.spawn(Velocity(3));
    let clean = world.spawn((Velocity(4), Position(4)));
    advance_to(&mut world, 4);
    assert!(world.set_component(b, Velocity(20)));
    let column = column_of(&world, b, ComponentKey::of::<Velocity>());
    assert!(
        column.rows.is_none(),
        "an unwatched column keeps no row ticks"
    );
    assert_eq!(column.summary, Tick(4));
    assert_ne!(
        location(&world, clean, SemanticDomain::Spatial).page_id,
        location(&world, b, SemanticDomain::Spatial).page_id,
        "the clean entity lives in another page"
    );

    for since in [0, 2, 3] {
        assert_eq!(
            changed::<Velocity>(&world, since).contains(&clean),
            since < 2,
            "since {since}: the other page was written at 2 only"
        );
    }
    assert_eq!(
        changed::<Velocity>(&world, 3),
        sorted(vec![a, b, c]),
        "no row ticks: every row of the page written since"
    );
    assert_eq!(
        changed_mut::<Velocity>(&mut world, 3),
        sorted(vec![a, b, c]),
        "query_mut alike"
    );
    assert_eq!(
        changed::<Velocity>(&world, 4),
        vec![],
        "a page not written since is skipped"
    );
}

/// **A query never told `since` sees every row**: the default is `Tick(0)`,
/// and an observer that has seen nothing sees everything — whether the rows
/// were placed or written. `Changed<T>` requires `T`: an entity without it
/// never matches.
#[test]
fn changed_with_no_since_sees_every_row() {
    let mut world = world();
    let a = world.spawn(Position(1));
    let b = world.spawn((Position(2), Velocity(2)));
    let bare = world.spawn(Velocity(3));
    let render = world.spawn(RenderId(4));
    advance_to(&mut world, 5);
    write_position(&mut world, b, 20);
    assert!(world.set_component(bare, Velocity(30)));
    let holders = sorted(vec![a, b]);

    let read: Vec<EntityId> = sorted(
        world
            .query::<(EntityId, Changed<Position>)>()
            .map(|(entity, ())| entity)
            .collect(),
    );
    assert_eq!(read, holders, "query, no since: every holder of Position");
    let read: Vec<EntityId> = sorted(
        world
            .query_mut::<(EntityId, Changed<Position>)>()
            .map(|(entity, ())| entity)
            .collect(),
    );
    assert_eq!(
        read, holders,
        "query_mut, no since: every holder of Position"
    );
    assert_eq!(
        changed::<Position>(&world, 0),
        holders,
        "since 0 is the default"
    );
    assert_eq!(changed::<RenderId>(&world, 0), vec![render]);
}

// --- in a query ---

/// **`Changed` beside writable terms**: `query_mut` over `&mut Velocity`
/// filtered by `Changed<Position>` hands out only the entities whose
/// `Position` changed, and a write through what it hands out reaches those
/// alone.
#[test]
fn changed_in_a_tuple_with_writable_terms() {
    let mut world = world();
    advance_to(&mut world, 2);
    let moved = world.spawn((Position(1), Velocity(1)));
    let still = world.spawn((Position(2), Velocity(2)));
    let also_moved = world.spawn((Position(3), Velocity(3), RenderId(3)));
    advance_to(&mut world, 5);
    write_position(&mut world, moved, 10);
    write_position(&mut world, also_moved, 30);
    advance_to(&mut world, 6);

    let mut handed = Vec::new();
    for (entity, velocity, ()) in world
        .query_mut::<(EntityId, &mut Velocity, Changed<Position>)>()
        .since(Tick(4))
    {
        assign(velocity, Velocity(100));
        handed.push(entity);
    }
    assert_eq!(sorted(handed), sorted(vec![moved, also_moved]));
    assert_eq!(world.get::<Velocity>(moved), Some(&Velocity(100)));
    assert_eq!(world.get::<Velocity>(also_moved), Some(&Velocity(100)));
    assert_eq!(
        world.get::<Velocity>(still),
        Some(&Velocity(2)),
        "an entity whose Position did not change is not handed out"
    );

    let read: Vec<(EntityId, i32)> = world
        .query::<(EntityId, &Velocity, Changed<Position>)>()
        .since(Tick(4))
        .map(|(entity, velocity, ())| (entity, velocity.0))
        .collect();
    assert_eq!(read.len(), 2, "beside a read term too: {read:?}");
    assert!(read.iter().all(|(_, velocity)| *velocity == 100));
}

/// **`Changed` on a component of another domain than the driver**, read
/// through the join: a Native query whose `Option` term names another
/// domain joins each row through the entity's live rows, and a Transversal
/// one does too. The answer is the same as the page-row path's.
#[test]
fn changed_on_a_component_in_another_domain() {
    let mut world = world();
    advance_to(&mut world, 2);
    let _quiet = world.spawn((Position(1), RenderId(1)));
    let moved = world.spawn((Position(2), RenderId(2)));
    let lone = world.spawn(Position(3));
    let shown = world.spawn((Position(4), RenderId(4)));
    advance_to(&mut world, 5);
    write_position(&mut world, moved, 20);
    write_position(&mut world, lone, 30);
    assert!(world.set_component(shown, RenderId(40)));
    advance_to(&mut world, 6);

    // Native over Spatial, joined through `fetch_from_world` for the
    // foreign `Option<&RenderId>`.
    let joined: Vec<(EntityId, Option<i32>)> = {
        let mut read: Vec<(EntityId, Option<i32>)> = world
            .query::<(EntityId, Changed<Position>, Option<&RenderId>)>()
            .since(Tick(4))
            .map(|(entity, (), render)| (entity, render.map(|r| r.0)))
            .collect();
        read.sort_by_key(|(entity, _)| entity.index);
        read
    };
    let mut expected = vec![(moved, Some(2)), (lone, None)];
    expected.sort_by_key(|(entity, _)| entity.index);
    assert_eq!(joined, expected, "joined: Changed<Position> since 4");

    let joined_mut: Vec<EntityId> = sorted(
        world
            .query_mut::<(EntityId, Changed<Position>, Option<&mut RenderId>)>()
            .since(Tick(4))
            .map(|(entity, (), _)| entity)
            .collect(),
    );
    assert_eq!(joined_mut, sorted(vec![moved, lone]), "joined, query_mut");

    // Transversal: `Changed<RenderId>` beside a Spatial term.
    let transversal: Vec<EntityId> = sorted(
        world
            .query::<(EntityId, &Position, Changed<RenderId>)>()
            .since(Tick(4))
            .map(|(entity, _, ())| entity)
            .collect(),
    );
    assert_eq!(
        transversal,
        vec![shown],
        "Transversal: Changed<RenderId> since 4"
    );
}

// --- moves ---

/// **A move is not a change, and an orphan is never read.** A migrated
/// entity is yielded once, from its live row — never from the orphan row it
/// left behind — and it carries the tick of its last write, so a reader who
/// saw that write does not see it again after the move.
#[test]
fn changed_yields_a_migrated_entity_once_and_only_for_its_writes() {
    let mut world = world();
    advance_to(&mut world, 2);
    let mover = world.spawn(Position(1));
    let stay = world.spawn(Position(2));
    advance_to(&mut world, 5);
    write_position(&mut world, mover, 10);
    advance_to(&mut world, 9);
    let left = location(&world, mover, SemanticDomain::Spatial).page_id;
    world
        .add_component(mover, Velocity(1))
        .expect("Velocity attaches");
    assert_ne!(
        location(&world, mover, SemanticDomain::Spatial).page_id,
        left,
        "the entity moved to another page"
    );
    assert!(
        world.storage.pages[left as usize].row_count() >= 2,
        "its old row is left behind as an orphan"
    );
    advance_to(&mut world, 10);

    assert_eq!(
        changed::<Position>(&world, 4),
        vec![mover],
        "written at 5, yielded once, from its live row"
    );
    assert_eq!(
        changed::<Position>(&world, 6),
        vec![],
        "the migration at 9 is not a write of Position"
    );
    assert_eq!(changed::<Position>(&world, 1), sorted(vec![mover, stay]));

    // Written, then despawned: whatever rows it left, none is read.
    advance_to(&mut world, 12);
    write_position(&mut world, mover, 11);
    assert!(world.despawn(mover));
    assert_eq!(
        changed::<Position>(&world, 4),
        vec![],
        "a dead entity is never yielded"
    );
}

// --- rows of one entity in two pages ---

/// **A peer column on the driver page that is not the entity's live one is
/// never judged.** An entity spawned with `(Position, RenderId)` holds both
/// in one page. When one domain is removed and its component added back,
/// the component lands in a page of its own, while the driver page keeps a
/// stale column of it — the one the entity no longer reads. A Transversal
/// `Changed` on that component must read the live row (as its value does,
/// through `fetch_from_world`), never the stale column of the driver page:
/// a write to the live row since `since` is a change, whatever the stale
/// column's summary says. One entity is split each way, so whichever domain
/// drives, one of the two reads goes through the stale column.
#[test]
fn changed_reads_the_live_row_when_the_driver_page_holds_a_stale_column() {
    let mut world = world();
    advance_to(&mut world, 2);
    let render_moved = world.spawn((Position(1), RenderId(1)));
    let spatial_moved = world.spawn((Position(2), RenderId(2)));
    let shared = location(&world, render_moved, SemanticDomain::Spatial).page_id;
    assert_eq!(
        location(&world, render_moved, SemanticDomain::Render).page_id,
        shared,
        "a bundle spanning two domains lands in one page"
    );

    advance_to(&mut world, 3);
    assert!(world
        .remove_component_domain::<RenderId>(render_moved)
        .is_some());
    world
        .add_component(render_moved, RenderId(10))
        .expect("RenderId attaches back");
    assert!(world
        .remove_component_domain::<Position>(spatial_moved)
        .is_some());
    world
        .add_component(spatial_moved, Position(20))
        .expect("Position attaches back");
    assert_eq!(
        location(&world, render_moved, SemanticDomain::Spatial).page_id,
        shared,
        "render_moved still reads its Position from the shared page"
    );
    assert_ne!(
        location(&world, render_moved, SemanticDomain::Render).page_id,
        shared,
        "but its RenderId from a page of its own"
    );
    assert_eq!(
        location(&world, spatial_moved, SemanticDomain::Render).page_id,
        shared,
        "spatial_moved still reads its RenderId from the shared page"
    );

    advance_to(&mut world, 5);
    assert!(world.set_component(render_moved, RenderId(100)));
    assert!(world.set_component(spatial_moved, Position(200)));
    advance_to(&mut world, 6);

    let render_changed: Vec<EntityId> = sorted(
        world
            .query::<(EntityId, &Position, Changed<RenderId>)>()
            .since(Tick(4))
            .map(|(entity, _, ())| entity)
            .collect(),
    );
    let position_changed: Vec<EntityId> = sorted(
        world
            .query::<(EntityId, &RenderId, Changed<Position>)>()
            .since(Tick(4))
            .map(|(entity, _, ())| entity)
            .collect(),
    );
    assert_eq!(
        (render_changed, position_changed),
        (vec![render_moved], vec![spatial_moved]),
        "each entity's live row was written at 5 (driver: {:?})",
        world
            .analyze_query(&[TypeId::of::<Position>(), TypeId::of::<RenderId>()])
            .driver_domain
    );
}

/// **`Changed<T>` beside `&mut T`**: the filter reads `T`'s ticks while the
/// guards it let through write them. Each guard is written as it is handed
/// out and kept, so the filter reads the next rows after a write — one that
/// gave the column its row ticks mid-iteration, `Position` having been
/// watched only after its rows were placed — and the guards are written
/// again, out of order, once the iteration is done. The rows yielded are
/// those that changed before the query began, each once; every write lands
/// and stamps its row.
#[test]
fn changed_beside_a_writable_term_of_the_same_component() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    advance_to(&mut world, 2);
    let entities: Vec<EntityId> = (0..4).map(|i| world.spawn(Position(i))).collect();
    world.observe(ComponentKey::of::<Position>());
    advance_to(&mut world, 4);

    {
        let mut held = Vec::new();
        for (entity, mut position, ()) in world
            .query_mut::<(EntityId, &mut Position, Changed<Position>)>()
            .since(Tick(1))
        {
            position.0 += 10;
            held.push((entity, position));
        }
        assert_eq!(held.len(), 4, "every row was written since 1");
        for (_, position) in held.iter_mut().rev() {
            position.0 += 100;
        }
    }
    for (i, entity) in entities.iter().enumerate() {
        assert_eq!(
            world.get::<Position>(*entity).map(|p| p.0),
            Some(i as i32 + 110),
            "both writes landed"
        );
    }
    advance_to(&mut world, 5);
    assert_eq!(
        changed::<Position>(&world, 3),
        sorted(entities.clone()),
        "every row was stamped at 4"
    );
    assert_eq!(changed::<Position>(&world, 4), vec![]);

    // Exact above the floor now: only the rows written at 5 pass at 4, and
    // writing them through the same query hands each out once.
    write_position(&mut world, entities[1], 1);
    write_position(&mut world, entities[3], 3);
    advance_to(&mut world, 6);
    let mut seen = Vec::new();
    for (entity, mut position, ()) in world
        .query_mut::<(EntityId, &mut Position, Changed<Position>)>()
        .since(Tick(4))
    {
        position.0 = -1;
        seen.push(entity);
    }
    assert_eq!(sorted(seen), sorted(vec![entities[1], entities[3]]));
}
