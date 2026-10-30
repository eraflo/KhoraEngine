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

//! Every write stamps the rows it wrote, through every path the `World`
//! writes by.
//!
//! A write made at the clock's tick — inside a run, the run's; between runs,
//! `current_tick()` — sets the written row's τ to it when the column keeps
//! row ticks, and raises the column's summary σ; a column no observer
//! watches keeps no row ticks, so its floor φ follows σ. A row read but not
//! written is not stamped, a value refused is not a write, and no write moves
//! a page's order version ω.
//!
//! The writable items are handled through `Deref`/`DerefMut` alone (see
//! [`assign`]), so these tests read the same whether the `World` hands out
//! `&mut T` or the `Mut<T>` guard.

use std::ops::DerefMut;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::page_ticks::{advance_to, page_of, row_tick, ticks};
use super::runtime_components::{declare, field, layout, patch, slot};
use super::{Position, Velocity};
use crate::ecs::{
    ColumnTicks, ColumnWriteReport, ComponentKey, FieldKind, Observer, SemanticDomain, Tick, World,
};

/// A Physics component with a field a column write reaches by slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, khora_macros::Component)]
#[component(domain = Physics)]
pub(super) struct Voltage {
    pub(super) level: i64,
}

/// A field-SoA component, written in bulk by `for_each_soa_column_mut`.
#[derive(Debug, Clone, Copy, Default, PartialEq, khora_macros::Component)]
#[component(no_serializable, layout = "soa")]
pub(super) struct Glide {
    pub(super) x: f32,
}

/// Writes `value` through a writable item — a `&mut T` or a `Mut<T>`.
pub(super) fn assign<T>(mut slot: impl DerefMut<Target = T>, value: T) {
    *slot = value;
}

/// The ticks of column `key` in the page holding `entity`'s row of it.
pub(super) fn column_of(world: &World, entity: EntityId, key: ComponentKey) -> ColumnTicks {
    let domain = world.components().vtable(key).expect("registered").domain;
    let (page, _) = page_of(world, entity, domain);
    ticks(page, key).clone()
}

/// τ of `entity`'s row of `key`: 0 when its column keeps no row ticks.
pub(super) fn tau(world: &World, entity: EntityId, key: ComponentKey) -> u64 {
    let domain = world.components().vtable(key).expect("registered").domain;
    let (page, row) = page_of(world, entity, domain);
    row_tick(ticks(page, key), row)
}

fn position() -> ComponentKey {
    ComponentKey::of::<Position>()
}

/// A world storing `Position` and `Velocity` (Spatial), `Voltage` and
/// `Glide` (Physics), where `Position`, `Voltage` and `Glide` are watched and
/// `Velocity` is not.
fn world() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<Voltage>(SemanticDomain::Physics);
    world.register_component::<Glide>(SemanticDomain::Physics);
    world.observe(position());
    world.observe(ComponentKey::of::<Voltage>());
    world.observe(ComponentKey::of::<Glide>());
    world
}

/// `count` entities holding a `Position` whose value is their index, placed
/// at tick 3; the clock then reads 8.
fn positions(count: i32) -> (World, Vec<EntityId>) {
    let mut world = world();
    advance_to(&mut world, 3);
    let entities = (0..count).map(|i| world.spawn(Position(i))).collect();
    advance_to(&mut world, 8);
    (world, entities)
}

/// The declared Script component `Ember { amount: int }`, watched, on two
/// entities placed at tick 3 in two different pages (the second also holds
/// `Ash`, which nobody watches); a third entity holds `Ash` alone. The
/// clock then reads 8.
fn embers() -> (World, ComponentKey, ComponentKey, [EntityId; 3]) {
    let mut world = World::new();
    let ember = declare(
        &mut world,
        "Ember",
        SemanticDomain::Script,
        vec![field("amount", FieldKind::Int, ScriptValue::Int(0))],
    );
    let ash = declare(
        &mut world,
        "Ash",
        SemanticDomain::Script,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );
    world.observe(ember);
    advance_to(&mut world, 3);
    let alone = world.spawn(());
    world
        .add_runtime_component(alone, ember, &patch(&[("amount", ScriptValue::Int(1))]))
        .expect("Ember attaches");
    let paired = world.spawn(());
    world
        .add_runtime_component(paired, ash, &ScriptValue::Unit)
        .expect("Ash attaches");
    world
        .add_runtime_component(paired, ember, &patch(&[("amount", ScriptValue::Int(2))]))
        .expect("Ember attaches");
    let ashes = world.spawn(());
    world
        .add_runtime_component(ashes, ash, &ScriptValue::Unit)
        .expect("Ash attaches");
    advance_to(&mut world, 8);
    (world, ember, ash, [alone, paired, ashes])
}

fn amount_slot(world: &World, ember: ComponentKey) -> usize {
    slot(world, ember, "amount").expect("Ember has `amount`")
}

// --- the guard, through `get_mut`, `get_many_mut` and `query_mut` ---

/// A row only read through the writable access `get_mut` hands out is not
/// stamped; the row written beside it is.
#[test]
fn reading_through_the_guard_stamps_nothing() {
    let (mut world, e) = positions(2);
    {
        let read = world.get_mut::<Position>(e[0]).expect("a Position");
        assert_eq!(read.0, 0);
    }
    world.get_mut::<Position>(e[1]).expect("a Position").0 = 5;

    assert_eq!(tau(&world, e[0], position()), 3, "a read is not a change");
    assert_eq!(tau(&world, e[1], position()), 8, "the write beside it is");
}

/// A write through `get_mut` stamps that row with the clock's tick and
/// raises the column's summary; the other rows and the floor stay.
#[test]
fn writing_through_the_guard_stamps_its_row() {
    let (mut world, e) = positions(3);
    let floor = column_of(&world, e[0], position()).floor;

    world.get_mut::<Position>(e[1]).expect("a Position").0 = 9;

    assert_eq!(world.get::<Position>(e[1]), Some(&Position(9)));
    let column = column_of(&world, e[0], position());
    assert_eq!(column.rows, Some(vec![3, 8, 3]));
    assert_eq!(column.summary, Tick(8));
    assert_eq!(column.floor, floor, "a write drops no history");
}

/// `get_many_mut` hands out one guard per entity: the rows written through
/// theirs are stamped, the row only read is not.
#[test]
fn get_many_mut_stamps_each_row_written() {
    let (mut world, e) = positions(4);

    let [first, second, fourth] = world.get_many_mut::<Position, 3>([e[0], e[1], e[3]]);
    assign(first.expect("a Position"), Position(10));
    assert_eq!(second.expect("a Position").0, 1);
    assign(fourth.expect("a Position"), Position(13));

    let column = column_of(&world, e[0], position());
    assert_eq!(column.rows, Some(vec![8, 3, 3, 8]));
    assert_eq!(column.summary, Tick(8));
}

/// A `query_mut` over many rows that writes a few: only those rows' τ move,
/// the page's summary is the run's tick, and a page the query wrote nothing
/// in keeps its summary.
#[test]
fn query_mut_stamps_only_the_rows_written() {
    let mut world = world();
    advance_to(&mut world, 3);
    let lone: Vec<EntityId> = (0..12).map(|i| world.spawn(Position(i))).collect();
    let paired: Vec<EntityId> = (0..4)
        .map(|i| world.spawn((Position(100 + i), Velocity(i))))
        .collect();
    advance_to(&mut world, 8);

    let run = world.begin_run();
    for (_, position) in world.query_mut::<(EntityId, &mut Position)>() {
        if position.0 < 12 && position.0 % 4 == 0 {
            let value = Position(-position.0);
            assign(position, value);
        }
    }
    world.end_run();

    assert_eq!(run, Tick(8));
    for (i, &entity) in lone.iter().enumerate() {
        let expected = if i % 4 == 0 { 8 } else { 3 };
        assert_eq!(tau(&world, entity, position()), expected, "row {i}");
    }
    assert_eq!(column_of(&world, lone[0], position()).summary, run);
    let untouched = column_of(&world, paired[0], position());
    assert_eq!(
        untouched.summary,
        Tick(3),
        "nothing was written in this page"
    );
    assert_eq!(untouched.rows, Some(vec![3, 3, 3, 3]));
}

/// `for_each_soa_column_mut` hands out whole columns: every row of every
/// page it hands out is stamped; the other columns of those pages are not.
#[test]
fn for_each_soa_column_mut_stamps_every_row_it_hands_out() {
    let mut world = world();
    advance_to(&mut world, 3);
    let lone: Vec<EntityId> = (0..3).map(|i| world.spawn(Glide { x: i as f32 })).collect();
    let paired: Vec<EntityId> = (0..2)
        .map(|i| world.spawn((Glide { x: i as f32 }, Voltage { level: i })))
        .collect();
    advance_to(&mut world, 8);

    world.for_each_soa_column_mut::<Glide>(|column| {
        for x in column.field_mut(0) {
            *x *= 2.0;
        }
    });

    let glide = ComponentKey::of::<Glide>();
    for &entity in lone.iter().chain(&paired) {
        assert_eq!(tau(&world, entity, glide), 8, "{entity:?}");
    }
    assert_eq!(column_of(&world, lone[0], glide).summary, Tick(8));
    assert_eq!(column_of(&world, paired[0], glide).summary, Tick(8));
    let charge = column_of(&world, paired[0], ComponentKey::of::<Voltage>());
    assert_eq!(
        charge.summary,
        Tick(3),
        "the bulk write reached Glide alone"
    );
    assert_eq!(charge.rows, Some(vec![3, 3]));
}

// --- the by-value paths ---

/// `set_component` stamps the row it wrote; a write to an entity without the
/// component writes, and stamps, nothing.
#[test]
fn set_component_stamps_its_row() {
    let (mut world, e) = positions(3);
    let bare = world.spawn(Velocity(1));

    assert!(world.set_component(e[1], Position(9)));
    assert!(!world.set_component(bare, Position(9)));

    let column = column_of(&world, e[0], position());
    assert_eq!(column.rows, Some(vec![3, 8, 3]));
    assert_eq!(column.summary, Tick(8));
}

/// `write_column` stamps each row it wrote, page by page; a row a field
/// refused is not written, so not stamped.
#[test]
fn write_column_stamps_the_rows_it_wrote() {
    let mut world = world();
    let charge = ComponentKey::of::<Voltage>();
    advance_to(&mut world, 3);
    let lone: Vec<EntityId> = (0..3).map(|i| world.spawn(Voltage { level: i })).collect();
    let paired: Vec<EntityId> = (0..2)
        .map(|i| world.spawn((Voltage { level: i }, Glide { x: 0.0 })))
        .collect();
    advance_to(&mut world, 8);
    let level = world
        .components()
        .vtable(charge)
        .expect("registered")
        .columns
        .field_slot("level")
        .expect("Voltage has `level`");

    let rows = [
        (lone[0], vec![ScriptValue::Int(10)]),
        (lone[2], vec![ScriptValue::Str("ten".to_owned())]),
        (paired[1], vec![ScriptValue::Int(14)]),
    ];
    let report = world.write_column(
        charge,
        &[level],
        rows.iter()
            .map(|(entity, values)| (*entity, values.as_slice())),
    );

    assert_eq!(
        report,
        ColumnWriteReport {
            written: 2,
            missing: 0,
            refused: 1,
        }
    );
    assert_eq!(column_of(&world, lone[0], charge).rows, Some(vec![8, 3, 3]));
    assert_eq!(column_of(&world, lone[0], charge).summary, Tick(8));
    assert_eq!(column_of(&world, paired[0], charge).rows, Some(vec![3, 8]));
    assert_eq!(column_of(&world, paired[0], charge).summary, Tick(8));
}

/// `RowMut::write_fields` stamps its row once a patch is accepted; a patch
/// refused writes nothing and stamps nothing.
#[test]
fn row_mut_write_fields_stamps_its_row() {
    let (mut world, ember, _, [alone, paired, _]) = embers();
    let amount = amount_slot(&world, ember);

    let refused = ScriptValue::Str("lots".to_owned());
    assert!(world
        .row_mut(alone, ember)
        .expect("an Ember row")
        .write_fields(&[(amount, &refused)])
        .is_err());
    assert_eq!(
        tau(&world, alone, ember),
        3,
        "a refused patch is not a write"
    );

    let accepted = ScriptValue::Int(5);
    world
        .row_mut(alone, ember)
        .expect("an Ember row")
        .write_fields(&[(amount, &accepted)])
        .expect("an int field takes an int");

    assert_eq!(tau(&world, alone, ember), 8);
    assert_eq!(column_of(&world, alone, ember).summary, Tick(8));
    assert_eq!(tau(&world, paired, ember), 3, "the other row stays");
}

/// `RowMut::set_field` stamps its row once a value is accepted; taking the
/// row and reading it, or offering a value refused, stamps nothing.
#[test]
fn row_mut_set_field_stamps_its_row() {
    let (mut world, ember, _, [alone, paired, _]) = embers();
    let amount = amount_slot(&world, ember);

    {
        let row = world.row_mut(alone, ember).expect("an Ember row");
        assert_eq!(row.field(amount), Some(ScriptValue::Int(1)));
    }
    assert_eq!(tau(&world, alone, ember), 3, "taking a row is not a write");

    assert!(world
        .row_mut(alone, ember)
        .expect("an Ember row")
        .set_field(amount, &ScriptValue::Str("lots".to_owned()))
        .is_err());
    assert_eq!(
        tau(&world, alone, ember),
        3,
        "a refused value is not a write"
    );

    world
        .row_mut(alone, ember)
        .expect("an Ember row")
        .set_field(amount, &ScriptValue::Int(5))
        .expect("an int field takes an int");

    assert_eq!(tau(&world, alone, ember), 8);
    assert_eq!(column_of(&world, alone, ember).summary, Tick(8));
    assert_eq!(tau(&world, paired, ember), 3, "the other row stays");
}

/// `relayout` rebuilds every row of the component: every row of it, in every
/// page, is stamped; the other components of those pages are not.
#[test]
fn relayout_stamps_every_row_of_the_component() {
    let (mut world, ember, ash, [alone, paired, ashes]) = embers();

    world
        .relayout(
            ember,
            layout(vec![
                field("amount", FieldKind::Int, ScriptValue::Int(0)),
                field("heat", FieldKind::Float, ScriptValue::Float(0.0)),
            ]),
        )
        .expect("Ember relays out");

    for entity in [alone, paired] {
        let column = column_of(&world, entity, ember);
        assert_eq!(column.summary, Tick(8), "{entity:?}");
        assert!(
            column
                .rows
                .as_ref()
                .is_some_and(|rows| rows.iter().all(|&t| t == 8)),
            "{entity:?}: every row of Ember is stamped, got {:?}",
            column.rows
        );
    }
    for entity in [paired, ashes] {
        assert_eq!(
            column_of(&world, entity, ash).summary,
            Tick(3),
            "{entity:?}: Ash was not relaid out"
        );
    }
}

/// `relayout` of a component nobody watches raises its summary and floor:
/// every row stays covered with no row ticks.
#[test]
fn relayout_of_an_unwatched_component_raises_its_summary_and_floor() {
    let (mut world, _, ash, [_, paired, ashes]) = embers();

    world
        .relayout(
            ash,
            layout(vec![
                field("n", FieldKind::Int, ScriptValue::Int(0)),
                field("m", FieldKind::Int, ScriptValue::Int(0)),
            ]),
        )
        .expect("Ash relays out");

    for entity in [paired, ashes] {
        assert_eq!(
            column_of(&world, entity, ash),
            ColumnTicks {
                summary: Tick(8),
                floor: Tick(8),
                rows: None,
            },
            "{entity:?}"
        );
    }
}

// --- columns nobody watches, columns newly watched ---

/// A column no observer watches keeps no row ticks: a write raises its
/// summary and its floor with it, through `set_component` as through the
/// guard.
#[test]
fn a_write_to_an_unobserved_column_raises_its_summary_and_floor() {
    let mut world = world();
    let velocity = ComponentKey::of::<Velocity>();
    advance_to(&mut world, 3);
    let first = world.spawn(Velocity(1));
    let second = world.spawn(Velocity(2));
    advance_to(&mut world, 8);

    assert!(world.set_component(first, Velocity(10)));
    assert_eq!(
        column_of(&world, first, velocity),
        ColumnTicks {
            summary: Tick(8),
            floor: Tick(8),
            rows: None,
        }
    );

    advance_to(&mut world, 12);
    world.get_mut::<Velocity>(second).expect("a Velocity").0 = 20;
    assert_eq!(
        column_of(&world, second, velocity),
        ColumnTicks {
            summary: Tick(12),
            floor: Tick(12),
            rows: None,
        }
    );
}

/// A column watched once its rows exist has no row ticks yet; its first
/// write gives it some — zeroed, under the floor its rows were placed at —
/// with the written row stamped.
#[test]
fn an_observed_column_without_row_ticks_gains_them_on_its_first_write() {
    let mut world = world();
    let velocity = ComponentKey::of::<Velocity>();
    advance_to(&mut world, 3);
    let e: Vec<EntityId> = (0..3).map(|i| world.spawn(Velocity(i))).collect();
    world.observe(velocity);
    advance_to(&mut world, 8);
    assert_eq!(column_of(&world, e[0], velocity).rows, None);

    assert!(world.set_component(e[1], Velocity(10)));

    assert_eq!(
        column_of(&world, e[0], velocity),
        ColumnTicks {
            summary: Tick(8),
            floor: Tick(3),
            rows: Some(vec![0, 8, 0]),
        }
    );
}

// --- the tick a write carries ---

/// Every write of one run — by value, through `get_mut`, through
/// `query_mut` — carries the run's tick; a write after the run carries the
/// next.
#[test]
fn every_write_of_a_run_carries_the_run_tick() {
    let (mut world, e) = positions(4);

    let run = world.begin_run();
    assert!(world.set_component(e[0], Position(10)));
    world.get_mut::<Position>(e[1]).expect("a Position").0 = 11;
    for (entity, position) in world.query_mut::<(EntityId, &mut Position)>() {
        if entity == e[2] {
            assign(position, Position(12));
        }
    }
    world.end_run();
    assert!(world.set_component(e[3], Position(13)));

    assert_eq!(run, Tick(8));
    assert_eq!(world.current_tick(), Tick(9));
    assert_eq!(
        column_of(&world, e[0], position()).rows,
        Some(vec![8, 8, 8, 9])
    );
}

/// I0: a write between runs — an app update, an editor edit — carries
/// `current_tick()`, which no observer has seen.
#[test]
fn a_write_between_runs_carries_a_tick_no_observer_has_seen() {
    let (mut world, e) = positions(2);
    let watcher = Observer::DataSystem("watcher");
    let run = world.begin_run();
    world.note_observer_run(watcher, run);
    world.end_run();

    world.get_mut::<Position>(e[0]).expect("a Position").0 = 7;

    let stamped = tau(&world, e[0], position());
    assert_eq!(stamped, world.current_tick().0);
    assert!(
        stamped > world.observer_last_seen(watcher).0,
        "a write between runs carries a tick the watcher has not seen"
    );
}

/// A write moves a column's summary, never its page's order version: no row
/// changed place — whatever path wrote.
#[test]
fn a_write_raises_the_summary_and_leaves_the_order_alone() {
    let (mut world, e) = positions(3);
    let charged = world.spawn(Voltage { level: 1 });
    world.spawn(Glide { x: 1.0 });
    let ember = declare(
        &mut world,
        "Ember",
        SemanticDomain::Script,
        vec![field("amount", FieldKind::Int, ScriptValue::Int(0))],
    );
    let burning = world.spawn(());
    world
        .add_runtime_component(burning, ember, &ScriptValue::Unit)
        .expect("Ember attaches");
    let orders_before: Vec<u64> = world.storage.pages.iter().map(|p| p.order()).collect();

    assert!(world.set_component(e[0], Position(10)));
    world.get_mut::<Position>(e[1]).expect("a Position").0 = 11;
    let [third] = world.get_many_mut::<Position, 1>([e[2]]);
    assign(third.expect("a Position"), Position(12));
    for (_, position) in world.query_mut::<(EntityId, &mut Position)>() {
        assign(position, Position(0));
    }
    let level = world
        .components()
        .vtable(ComponentKey::of::<Voltage>())
        .expect("registered")
        .columns
        .field_slot("level")
        .expect("Voltage has `level`");
    let rows = [(charged, vec![ScriptValue::Int(4)])];
    world.write_column(
        ComponentKey::of::<Voltage>(),
        &[level],
        rows.iter()
            .map(|(entity, values)| (*entity, values.as_slice())),
    );
    world.for_each_soa_column_mut::<Glide>(|column| {
        for x in column.field_mut(0) {
            *x += 1.0;
        }
    });
    let amount = amount_slot(&world, ember);
    {
        let mut row = world.row_mut(burning, ember).expect("an Ember row");
        row.set_field(amount, &ScriptValue::Int(3))
            .expect("an int field takes an int");
        row.write_fields(&[(amount, &ScriptValue::Int(4))])
            .expect("an int field takes an int");
    }
    world
        .relayout(
            ember,
            layout(vec![
                field("amount", FieldKind::Int, ScriptValue::Int(0)),
                field("heat", FieldKind::Float, ScriptValue::Float(0.0)),
            ]),
        )
        .expect("Ember relays out");

    let orders_after: Vec<u64> = world.storage.pages.iter().map(|p| p.order()).collect();
    assert_eq!(orders_after, orders_before, "no write moves a row");
    assert_eq!(
        column_of(&world, e[0], position()).summary,
        Tick(8),
        "the writes moved the summary"
    );
}
