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

//! Freeing quiet columns: a column's row ticks are dropped once it has been
//! silent long enough and every observer that ran on the world has seen its
//! last write.
//!
//! `World::free_quiet_row_ticks(quiet)` frees every page column keeping row
//! ticks whose summary σ is at least `quiet` ticks old and that no observer
//! of the ledger has yet to see (`last_seen ≥ σ`). A freed column keeps σ,
//! raises its floor φ to σ and keeps no row ticks; a page's order version ω
//! never moves; a column without row ticks is never touched. The next write
//! of an observed key gives the column row ticks again, zeroed under the
//! floor, so that only the row written is known as changed — to a reader at
//! or above the floor. A reader under it sees every row. `ecs_maintenance`
//! frees after `QUIET_ROW_TICKS` of silence, every run.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use super::no_change_missed::run_freeing_sequence;
use super::page_ticks::{advance_to, location, page_of, row_tick, ticks};
use super::write_stamps::column_of;
use super::{Position, RenderId, RenderTag, Velocity};
use crate::ecs::{
    Changed, ColumnTicks, Component, ComponentKey, DataSystemRegistration, Observer,
    SemanticDomain, SystemContext, Tick, World, QUIET_ROW_TICKS,
};

/// An observer whose runs the tests make by hand.
const WATCHER: Observer = Observer::DataSystem("watcher");

/// A second observer, of the other kind.
const OTHER: Observer = Observer::Flow("other watcher");

/// A world storing `Position` and `Velocity` (Spatial), `RenderId` and
/// `RenderTag` (Render); `Position` and `RenderId` are watched, the other two
/// are not.
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

fn position() -> ComponentKey {
    ComponentKey::of::<Position>()
}

/// Runs `observer` at tick `tick`, as a runner does: it then has seen every
/// write up to `tick`, and the clock moves past it.
fn run_observer(world: &mut World, observer: Observer, tick: u64) {
    advance_to(world, tick);
    let run = world.begin_run();
    assert_eq!(run, Tick(tick));
    world.note_observer_run(observer, run);
    world.end_run();
}

/// Writes `entity`'s `Position` now, through the guard `get_mut` returns.
fn write_position(world: &mut World, entity: EntityId, value: i32) {
    world.get_mut::<Position>(entity).expect("a Position").0 = value;
}

/// Sorted, so that two answers compare whatever the page order.
fn sorted(mut entities: Vec<EntityId>) -> Vec<EntityId> {
    entities.sort_by_key(|entity| (entity.index, entity.generation));
    entities
}

/// The entities `(EntityId, Changed<T>)` yields since `since`.
fn changed<T: Component>(world: &World, since: u64) -> Vec<EntityId> {
    sorted(
        world
            .query::<(EntityId, Changed<T>)>()
            .since(Tick(since))
            .map(|(entity, ())| entity)
            .collect(),
    )
}

/// The registered `ecs_maintenance` DataSystem.
fn ecs_maintenance() -> fn(&mut World, &Runtime, &mut OutputDeck, SystemContext) {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "ecs_maintenance")
        .expect("`ecs_maintenance` is registered")
        .run
}

/// Runs `ecs_maintenance` once, directly, with an empty `Runtime` — no
/// `EcsMaintenance` resource in it.
fn run_ecs_maintenance(world: &mut World) {
    let runtime = Runtime::default();
    let mut deck = OutputDeck::new();
    ecs_maintenance()(world, &runtime, &mut deck, SystemContext { since: Tick(0) });
}

// --- when a column is freed ---

/// **Freed once every observer has seen it.** A column silent for long is
/// kept while one observer of the ledger last ran before its latest write;
/// once that observer runs again, the column is freed: no row ticks, its
/// floor at its summary.
#[test]
fn a_quiet_column_is_freed_once_every_observer_has_seen_it() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));
    run_observer(&mut world, OTHER, 2);
    advance_to(&mut world, 3);
    write_position(&mut world, b, 20);
    run_observer(&mut world, WATCHER, 5);
    advance_to(&mut world, 40);

    assert_eq!(
        world.free_quiet_row_ticks(10),
        0,
        "`other watcher` last ran at 2, before the write at 3: the column is kept"
    );
    let column = column_of(&world, a, position());
    assert!(column.rows.is_some(), "kept: its row ticks stay");
    assert_eq!(column.floor, Tick(0), "kept: its floor stays");

    run_observer(&mut world, OTHER, 41);
    assert_eq!(
        world.free_quiet_row_ticks(10),
        1,
        "every observer has seen the write at 3, and it is 39 ticks old"
    );
    let column = column_of(&world, a, position());
    assert!(
        column.rows.is_none(),
        "freed: the column keeps no row ticks: {:?}",
        column.rows
    );
    assert_eq!(column.summary, Tick(3), "the summary stays");
    assert_eq!(column.floor, Tick(3), "the floor rises to the summary");
}

/// **A column an observer has not seen is kept.** The observer ran at 3, the
/// column was written at 4: however long the silence, and whatever the
/// `quiet` asked, its row ticks stay — the observer's next read is still
/// exact.
#[test]
fn a_column_an_observer_has_not_seen_is_kept() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));
    run_observer(&mut world, WATCHER, 3);
    advance_to(&mut world, 4);
    write_position(&mut world, b, 20);
    advance_to(&mut world, 10_000);

    assert_eq!(world.free_quiet_row_ticks(10), 0, "s = 3 < σ = 4: kept");
    assert_eq!(
        world.free_quiet_row_ticks(0),
        0,
        "even with no silence asked"
    );

    let column = column_of(&world, b, position());
    assert!(column.rows.is_some(), "the row ticks stay");
    assert_eq!(column.summary, Tick(4));
    assert_eq!(column.floor, Tick(0), "the floor stays");
    let (page, row_b) = page_of(&world, b, SemanticDomain::Spatial);
    let (_, row_a) = page_of(&world, a, SemanticDomain::Spatial);
    let column = ticks(page, position());
    assert_eq!(row_tick(column, row_b), 4, "b keeps its tick");
    assert_eq!(row_tick(column, row_a), 2, "a keeps its tick");
    assert_eq!(
        changed::<Position>(&world, 3),
        vec![b],
        "the observer's read since 3 is still exact"
    );
}

/// **An observer that never ran does not hold a column back.** Only the
/// observers that ran on this world are asked; one that never did reads
/// `Tick(0)` and is not among them. A world on which no observer ever ran
/// frees its quiet columns too.
#[test]
fn an_observer_that_never_ran_does_not_hold_a_column_back() {
    let never = Observer::Flow("never ran");

    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    run_observer(&mut world, WATCHER, 3);
    advance_to(&mut world, 20);
    assert_eq!(world.observer_last_seen(never), Tick(0));
    assert_eq!(
        world.free_quiet_row_ticks(10),
        1,
        "`never ran` has seen nothing, but it never ran here: the column is freed"
    );
    assert!(column_of(&world, a, position()).rows.is_none());

    let mut lonely = world_with_no_observer_run();
    let b = lonely.spawn(Position(1));
    advance_to(&mut lonely, 30);
    assert_eq!(
        lonely.free_quiet_row_ticks(10),
        1,
        "no observer ever ran on this world: nothing holds the column back"
    );
    assert!(column_of(&lonely, b, position()).rows.is_none());
}

fn world_with_no_observer_run() -> World {
    let mut world = world();
    advance_to(&mut world, 2);
    world
}

/// **A column written often is never freed**: each run writes it, so it is
/// never `quiet` ticks old, even though every observer has seen every write.
/// A quiet column of another page, in the same world, is freed once, when
/// its silence reaches `quiet`.
#[test]
fn a_column_written_often_is_never_freed() {
    let mut world = world();
    advance_to(&mut world, 2);
    let hot = world.spawn(Position(0));
    let cold = world.spawn((Position(0), Velocity(0)));
    assert_ne!(
        location(&world, hot, SemanticDomain::Spatial).page_id,
        location(&world, cold, SemanticDomain::Spatial).page_id,
        "the hot and the cold rows live in two pages"
    );

    let mut freed = 0;
    let mut last_run = Tick(0);
    for value in 0..200 {
        let run = world.begin_run();
        write_position(&mut world, hot, value);
        world.note_observer_run(WATCHER, run);
        world.end_run();
        last_run = run;

        freed += world.free_quiet_row_ticks(10);
        let column = column_of(&world, hot, position());
        assert!(
            column.rows.is_some(),
            "run {run:?}: the column written every run keeps its row ticks"
        );
        assert_eq!(column.floor, Tick(0), "run {run:?}: its floor never rises");
    }
    assert_eq!(freed, 1, "only the cold column was freed, once");
    assert!(column_of(&world, cold, position()).rows.is_none());
    assert_eq!(
        changed::<Position>(&world, last_run.0 - 1),
        vec![hot],
        "the hot column still answers exactly"
    );
}

// --- what freeing changes ---

/// **Freeing keeps σ and ω and raises φ to σ.** A freed column keeps its
/// summary — what was written is not forgotten, only which row — its floor
/// becomes the summary, and its page's order version does not move: no row
/// changed place. The page's column without row ticks is left exactly as it
/// was.
#[test]
fn freeing_keeps_the_summary_and_the_order_and_raises_the_floor_to_the_summary() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn((Position(1), Velocity(1)));
    advance_to(&mut world, 3);
    world.spawn((Position(2), Velocity(2)));
    let c = world.spawn((Position(3), Velocity(3)));
    assert!(world.despawn(c), "a despawn moves the order version first");
    advance_to(&mut world, 4);
    write_position(&mut world, a, 10);

    let (page, _) = page_of(&world, a, SemanticDomain::Spatial);
    let order_before = page.order();
    let position_before = ticks(page, position()).clone();
    let velocity_before = ticks(page, ComponentKey::of::<Velocity>()).clone();
    assert!(
        position_before.rows.is_some(),
        "the watched column keeps rows"
    );
    assert!(
        velocity_before.rows.is_none(),
        "the unwatched column keeps none"
    );
    assert_eq!(position_before.summary, Tick(4));

    run_observer(&mut world, WATCHER, 6);
    advance_to(&mut world, 30);
    assert_eq!(
        world.free_quiet_row_ticks(10),
        1,
        "the watched column alone is freed"
    );

    let (page, _) = page_of(&world, a, SemanticDomain::Spatial);
    let freed: &ColumnTicks = ticks(page, position());
    assert!(freed.rows.is_none(), "no row ticks: {:?}", freed.rows);
    assert_eq!(freed.summary, position_before.summary, "σ stays");
    assert_eq!(freed.floor, position_before.summary, "φ := σ");
    assert_eq!(page.order(), order_before, "ω does not move");
    assert_eq!(
        ticks(page, ComponentKey::of::<Velocity>()),
        &velocity_before,
        "a column without row ticks is not touched"
    );
}

/// **Freeing returns how many columns it dropped**: one per page column
/// freed, a column written within the silence asked not among them, a
/// column without row ticks never; nothing the second time.
#[test]
fn freeing_returns_how_many_columns_it_dropped() {
    let mut world = world();
    advance_to(&mut world, 2);
    world.spawn(Position(1));
    world.spawn(Position(2));
    world.spawn((Position(3), Velocity(3)));
    world.spawn(RenderId(4));
    let busy = world.spawn((RenderId(5), RenderTag));
    advance_to(&mut world, 30);
    assert!(world.set_component(busy, RenderId(50)));
    run_observer(&mut world, WATCHER, 31);
    advance_to(&mut world, 35);

    assert_eq!(
        world.free_quiet_row_ticks(10),
        3,
        "Position in two pages and the quiet RenderId page; the RenderId written at 30 is 5 ticks old"
    );
    assert_eq!(world.free_quiet_row_ticks(10), 0, "nothing left to free");

    advance_to(&mut world, 40);
    assert_eq!(
        world.free_quiet_row_ticks(10),
        1,
        "the busy RenderId column is now 10 ticks old"
    );
    assert_eq!(world.free_quiet_row_ticks(0), 0, "nothing left to free");
}

// --- after a free ---

/// **A freed column recreated by a write reports only that row** — whatever
/// path wrote it. The write gives the column row ticks again, one per row:
/// the row written at its tick, every other row at 0, under the floor. A
/// reader at or above the floor — the observer, here — sees that row alone.
#[test]
fn a_freed_column_recreated_by_a_write_reports_only_that_row() {
    for path in ["get_mut", "set_component", "query_mut"] {
        let mut world = world();
        advance_to(&mut world, 2);
        let a = world.spawn(Position(1));
        let b = world.spawn(Position(2));
        let c = world.spawn(Position(3));
        run_observer(&mut world, WATCHER, 5);
        advance_to(&mut world, 20);
        assert_eq!(world.free_quiet_row_ticks(10), 1, "{path}: freed");

        advance_to(&mut world, 21);
        match path {
            "get_mut" => write_position(&mut world, b, 20),
            "set_component" => assert!(world.set_component(b, Position(20))),
            _ => {
                for (entity, mut item) in world.query_mut::<(EntityId, &mut Position)>() {
                    if entity == b {
                        item.0 = 20;
                    }
                }
            }
        }

        let (page, row_b) = page_of(&world, b, SemanticDomain::Spatial);
        let column = ticks(page, position());
        let rows = column
            .rows
            .as_ref()
            .unwrap_or_else(|| panic!("{path}: the write gives the column row ticks again"));
        assert_eq!(rows.len(), page.row_count(), "{path}: one tick per row");
        assert_eq!(rows[row_b], 21, "{path}: the row written reads its tick");
        assert_eq!(column.summary, Tick(21), "{path}: σ is the write");
        assert_eq!(column.floor, Tick(2), "{path}: φ stays at the old σ");
        for other in [a, c] {
            let (_, row) = page_of(&world, other, SemanticDomain::Spatial);
            assert_eq!(rows[row], 0, "{path}: {other:?} reads 0, under the floor");
        }

        assert_eq!(
            changed::<Position>(&world, 5),
            vec![b],
            "{path}: the observer, since 5, sees b alone"
        );
        assert_eq!(
            changed::<Position>(&world, 2),
            vec![b],
            "{path}: since = φ, b alone"
        );
    }
}

/// **A late reader after a free sees the whole column.** A reader whose
/// `since` is under the floor cannot be told which rows were written: it
/// gets every row — before the column is written again and after.
#[test]
fn a_late_reader_after_a_free_sees_the_whole_column() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    let b = world.spawn(Position(2));
    let c = world.spawn(Position(3));
    let everyone = sorted(vec![a, b, c]);
    advance_to(&mut world, 3);
    write_position(&mut world, b, 20);
    assert_eq!(changed::<Position>(&world, 2), vec![b], "before the free");

    run_observer(&mut world, WATCHER, 5);
    advance_to(&mut world, 20);
    assert_eq!(world.free_quiet_row_ticks(10), 1);
    assert_eq!(column_of(&world, a, position()).floor, Tick(3));

    for since in [0, 1, 2] {
        assert_eq!(
            changed::<Position>(&world, since),
            everyone,
            "freed, since {since} < σ: every row"
        );
    }
    assert_eq!(changed::<Position>(&world, 3), vec![], "since = σ: nothing");

    advance_to(&mut world, 21);
    write_position(&mut world, c, 30);
    for since in [0, 1, 2] {
        assert_eq!(
            changed::<Position>(&world, since),
            everyone,
            "recreated, since {since} < φ: every row"
        );
    }
    for since in [3, 5, 20] {
        assert_eq!(
            changed::<Position>(&world, since),
            vec![c],
            "recreated, since {since} ≥ φ: the row ticks answer"
        );
    }
}

// --- the Maintenance phase ---

/// **`ecs_maintenance` frees quiet columns**, every run, with no
/// `EcsMaintenance` resource present: after more than `QUIET_ROW_TICKS` of
/// silence, a column every observer has seen keeps no row ticks.
#[test]
fn ecs_maintenance_frees_quiet_columns_without_an_ecs_maintenance_resource() {
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    run_observer(&mut world, WATCHER, 2);
    advance_to(&mut world, 2 + QUIET_ROW_TICKS + 1);

    run_ecs_maintenance(&mut world);

    let column = column_of(&world, a, position());
    assert!(
        column.rows.is_none(),
        "silent {} ticks, more than QUIET_ROW_TICKS ({QUIET_ROW_TICKS}): freed",
        world.current_tick().0 - 2
    );
    assert_eq!(column.floor, Tick(2), "φ := σ");
}

/// **`ecs_maintenance` waits `QUIET_ROW_TICKS`**: a column silent one tick
/// less is kept; silent exactly that long, it is freed. The silence is a
/// real window — some seconds of frames — not zero.
#[test]
fn ecs_maintenance_keeps_a_column_until_it_has_been_quiet_long_enough() {
    const {
        assert!(
            QUIET_ROW_TICKS > 1,
            "QUIET_ROW_TICKS is a silence of some seconds of frames, not zero or one tick"
        )
    };
    let mut world = world();
    advance_to(&mut world, 2);
    let a = world.spawn(Position(1));
    run_observer(&mut world, WATCHER, 2);

    advance_to(&mut world, 2 + QUIET_ROW_TICKS - 1);
    run_ecs_maintenance(&mut world);
    assert!(
        column_of(&world, a, position()).rows.is_some(),
        "silent QUIET_ROW_TICKS − 1 ticks: kept"
    );

    advance_to(&mut world, 2 + QUIET_ROW_TICKS);
    run_ecs_maintenance(&mut world);
    assert!(
        column_of(&world, a, position()).rows.is_none(),
        "silent QUIET_ROW_TICKS ticks: freed"
    );
}

// --- the theorem, with frees ---

/// **No change is ever missed when quiet columns are freed**: the random
/// sequences of `no_change_missed`, observers noted in the world's ledger,
/// with `free_quiet_row_ticks` at a small random `quiet` among the other
/// operations. Each free is checked against its rule; every read keeps the
/// theorem, and is exact where the column keeps row ticks and `since ≥ φ`.
#[test]
fn no_change_is_missed_when_quiet_columns_are_freed() {
    let (mut exact, mut caught, mut freed) = (0, 0, 0);
    for seed in 1..=48u64 {
        let (e, c, f) = run_freeing_sequence(seed, 250);
        exact += e;
        caught += c;
        freed += f;
    }
    assert!(exact > 500, "the exact case was checked {exact} times only");
    assert!(caught > 500, "the theorem was checked {caught} times only");
    assert!(freed > 100, "only {freed} columns were freed");
}

/// A long sequence with frees: columns freed, recreated, freed again, pages
/// recycled under them.
#[test]
fn no_change_is_missed_over_a_long_sequence_with_frees() {
    let (exact, caught, freed) = run_freeing_sequence(0x0F2E_E5ED, 3000);
    assert!(
        exact > 0 && caught > 0 && freed > 0,
        "exact {exact}, caught {caught}, freed {freed}"
    );
}
