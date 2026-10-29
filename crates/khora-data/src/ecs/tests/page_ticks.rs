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

//! The ticks a page keeps beside its columns.
//!
//! Each column of a page carries a summary σ (the latest tick written), a
//! floor φ (the tick its row history was dropped at) and — for a component
//! some observer watches — one tick τ per row. A row placed is written at the
//! clock's tick; a row moved (a swap-remove, a migration, a compaction)
//! carries its tick and is not a change; a row moved within a page moves the
//! page's order version ω. Everything is read with the page and a key alone,
//! never through the `World`.

use khora_core::ecs::entity::EntityId;

use super::{Position, RenderId, Velocity};
use crate::ecs::{
    ColumnTicks, ComponentKey, ComponentPage, PageIndex, SemanticDomain, Tick, World,
};

/// Moves `world`'s clock forward, one complete run at a time, until it reads
/// `tick`.
pub(super) fn advance_to(world: &mut World, tick: u64) {
    assert!(
        world.current_tick().0 <= tick,
        "the clock never goes back: it reads {:?}, asked {tick}",
        world.current_tick()
    );
    while world.current_tick().0 < tick {
        world.begin_run();
        world.end_run();
    }
}

/// A world storing `Position` and `Velocity` (Spatial) and `RenderId`
/// (Render), where `Position` is watched by an observer and the other two
/// are not.
fn world_watching_position() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world.observe(ComponentKey::of::<Position>());
    world
}

/// `entity`'s location in `domain`.
pub(super) fn location(world: &World, entity: EntityId, domain: SemanticDomain) -> PageIndex {
    let (id, metadata) = world
        .entities
        .get(entity.index as usize)
        .unwrap_or_else(|| panic!("{entity:?} has no slot"));
    assert_eq!(*id, entity, "{entity:?} is not alive");
    *metadata
        .as_ref()
        .and_then(|metadata| metadata.locations.get(&domain))
        .unwrap_or_else(|| panic!("{entity:?} holds nothing in {domain:?}"))
}

/// The page holding `entity`'s components of `domain`, and its row there.
pub(super) fn page_of(
    world: &World,
    entity: EntityId,
    domain: SemanticDomain,
) -> (&ComponentPage, usize) {
    let at = location(world, entity, domain);
    (
        &world.storage.pages[at.page_id as usize],
        at.row_index as usize,
    )
}

/// The ticks of column `key` of `page` — which must hold that column.
pub(super) fn ticks(page: &ComponentPage, key: ComponentKey) -> &ColumnTicks {
    page.column_ticks(key)
        .unwrap_or_else(|| panic!("the page holds {key:?} but has no ticks for it"))
}

/// τ of row `row`: its tick when the column keeps rows, 0 when it does not.
pub(super) fn row_tick(ticks: &ColumnTicks, row: usize) -> u64 {
    ticks.rows.as_ref().map_or(0, |rows| rows[row])
}

// --- reading ---

/// A page answers for its ticks with itself and a key: here the `World` is
/// gone, its pages taken out of it, and each column of the page still says
/// when it was written — the founding principle that a page is readable
/// without its world.
#[test]
fn a_page_reads_its_ticks_with_itself_and_a_key() {
    let mut world = world_watching_position();
    advance_to(&mut world, 4);
    let entity = world.spawn((Position(1), Velocity(2)));
    let at = location(&world, entity, SemanticDomain::Spatial);

    let pages = std::mem::take(&mut world.storage.pages);
    drop(world);
    let page = &pages[at.page_id as usize];
    let row = at.row_index as usize;

    for key in page.keys.clone() {
        let column = ticks(page, key);
        assert_eq!(
            column.summary,
            Tick(4),
            "{key:?}: a row placed at tick 4 is a write at tick 4"
        );
    }
    let position = ticks(page, ComponentKey::of::<Position>());
    let rows = position
        .rows
        .as_ref()
        .expect("a watched column keeps a tick per row once written");
    assert_eq!(rows.len(), page.row_count(), "one tick per row");
    assert_eq!(rows[row], 4, "the row reads the tick it was placed at");

    assert!(
        page.column_ticks(ComponentKey::of::<RenderId>()).is_none(),
        "a page has no ticks for a column it does not hold"
    );
}

/// Rows placed at different ticks keep their own: each row of a watched
/// column reads the tick it was placed at, the summary the latest, and the
/// floor stays where it was — no row history was ever dropped.
#[test]
fn rows_placed_at_different_ticks_keep_their_own_ticks() {
    let mut world = world_watching_position();
    let mut placed = Vec::new();
    for tick in [2u64, 5, 9] {
        advance_to(&mut world, tick);
        placed.push((world.spawn(Position(tick as i32)), tick));
    }

    let (page, _) = page_of(&world, placed[0].0, SemanticDomain::Spatial);
    let position = ticks(page, ComponentKey::of::<Position>());
    assert_eq!(position.summary, Tick(9), "the summary is the latest write");
    assert_eq!(position.floor, Tick(0), "no row history was dropped");
    assert_eq!(
        position.rows.as_ref().map(Vec::len),
        Some(page.row_count()),
        "one tick per row"
    );
    for (entity, tick) in placed {
        let (_, row) = page_of(&world, entity, SemanticDomain::Spatial);
        assert_eq!(row_tick(position, row), tick, "{entity:?}");
    }
}

/// A column nobody watches keeps no tick per row: only its summary, and a
/// floor that follows it — every row counts as written at the summary, so a
/// reader asking about it sees all of it or none of it, never too little.
#[test]
fn an_unobserved_column_holds_only_its_summary() {
    let mut world = world_watching_position();
    let mut entities = Vec::new();
    for tick in [3u64, 6, 8] {
        advance_to(&mut world, tick);
        entities.push(world.spawn((Position(0), Velocity(tick as i32))));
    }

    let (page, _) = page_of(&world, entities[0], SemanticDomain::Spatial);
    let velocity = ticks(page, ComponentKey::of::<Velocity>());
    assert!(
        velocity.rows.is_none(),
        "an unwatched column keeps no tick per row: {:?}",
        velocity.rows
    );
    assert_eq!(velocity.summary, Tick(8), "its summary is its latest write");
    assert_eq!(
        velocity.floor, velocity.summary,
        "with no row ticks the floor follows the summary, so every row is covered"
    );

    let position = ticks(page, ComponentKey::of::<Position>());
    assert!(
        position.rows.is_some(),
        "the watched column of the same page keeps its rows' ticks"
    );
}

/// A component watched only from some moment on keeps its rows' ticks from
/// the next row placed: the column's tick array appears, one tick per row,
/// the new row at its tick and every earlier row still covered — by its own
/// tick or by the floor.
#[test]
fn a_column_observed_at_run_time_keeps_its_rows_ticks_from_the_next_write() {
    let mut world = World::new();
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    advance_to(&mut world, 2);
    let early = world.spawn(Velocity(1));
    advance_to(&mut world, 5);
    world.observe(ComponentKey::of::<Velocity>());
    advance_to(&mut world, 7);
    let late = world.spawn(Velocity(2));

    let (page, late_row) = page_of(&world, late, SemanticDomain::Spatial);
    let (_, early_row) = page_of(&world, early, SemanticDomain::Spatial);
    let velocity = ticks(page, ComponentKey::of::<Velocity>());
    let rows = velocity
        .rows
        .as_ref()
        .expect("a watched column keeps a tick per row once written");
    assert_eq!(rows.len(), page.row_count(), "one tick per row");
    assert_eq!(rows[late_row], 7, "the new row reads its tick");
    assert!(
        row_tick(velocity, early_row).max(velocity.floor.0) >= 2,
        "the earlier row is still covered: τ {} φ {:?}",
        row_tick(velocity, early_row),
        velocity.floor
    );
    assert_eq!(velocity.summary, Tick(7));
}

// --- moving within a page ---

/// A despawn swap-removes its row: the page's last row takes the hole and
/// its tick comes with it; the order version moves, because a row changed
/// place; the summary does not, because nothing was written.
#[test]
fn swap_remove_carries_the_moved_rows_tick_and_bumps_order() {
    let mut world = world_watching_position();
    let mut entities = Vec::new();
    for tick in [1u64, 4, 6] {
        advance_to(&mut world, tick);
        entities.push(world.spawn((Position(tick as i32), Velocity(0))));
    }
    let [first, second, last] = [entities[0], entities[1], entities[2]];
    let (page, first_row) = page_of(&world, first, SemanticDomain::Spatial);
    let order_before = page.order();
    let page_id = location(&world, first, SemanticDomain::Spatial).page_id;

    advance_to(&mut world, 10);
    assert!(world.despawn(first));

    let page = &world.storage.pages[page_id as usize];
    let (_, last_row) = page_of(&world, last, SemanticDomain::Spatial);
    assert_eq!(last_row, first_row, "the last row took the hole");
    let position = ticks(page, ComponentKey::of::<Position>());
    assert_eq!(
        position.rows.as_ref().map(Vec::len),
        Some(page.row_count()),
        "the tick array lost the removed row"
    );
    assert_eq!(
        row_tick(position, last_row),
        6,
        "the moved row carries its own tick, not the despawn's"
    );
    let (_, second_row) = page_of(&world, second, SemanticDomain::Spatial);
    assert_eq!(
        row_tick(position, second_row),
        4,
        "a row not moved keeps its tick"
    );
    assert_eq!(position.summary, Tick(6), "a removal writes nothing");
    assert!(
        page.order() > order_before,
        "a row changed place: the order version moves ({} then {})",
        order_before,
        page.order()
    );
    assert!(
        ticks(page, ComponentKey::of::<Velocity>()).rows.is_none(),
        "the unwatched column still keeps no row ticks"
    );
}

/// Despawning the page's last row moves no other row, yet the page changed:
/// its order version moves all the same.
#[test]
fn a_despawn_of_the_last_row_bumps_order() {
    let mut world = world_watching_position();
    let kept = world.spawn(Position(1));
    advance_to(&mut world, 3);
    let gone = world.spawn(Position(2));
    let page_id = location(&world, gone, SemanticDomain::Spatial).page_id;
    let order_before = world.storage.pages[page_id as usize].order();

    assert!(world.despawn(gone));

    let page = &world.storage.pages[page_id as usize];
    assert!(
        page.order() > order_before,
        "a despawn moves the order version"
    );
    let position = ticks(page, ComponentKey::of::<Position>());
    assert_eq!(position.rows.as_ref().map(Vec::len), Some(1));
    let (_, row) = page_of(&world, kept, SemanticDomain::Spatial);
    assert_eq!(
        row_tick(position, row),
        1,
        "the remaining row keeps its tick"
    );
}

/// Compaction reclaims a migration's orphan rows: the survivors change place,
/// so the order version moves — but no survivor is written, so each keeps
/// its own tick and the summary stays.
#[test]
fn compaction_bumps_order_but_no_row_tick() {
    let mut world = world_watching_position();
    let mut placed = Vec::new();
    for tick in [1u64, 3, 5, 7] {
        advance_to(&mut world, tick);
        placed.push((world.spawn(Position(tick as i32)), tick));
    }
    let page_id = location(&world, placed[0].0, SemanticDomain::Spatial).page_id;

    // The first leaves the page (adding a component moves its row), leaving
    // an orphan at row 0.
    advance_to(&mut world, 9);
    world
        .add_component(placed[0].0, Velocity(1))
        .expect("Velocity attaches");
    let page = &world.storage.pages[page_id as usize];
    let order_before = page.order();
    let summary_before = ticks(page, ComponentKey::of::<Position>()).summary;

    advance_to(&mut world, 12);
    world.run_compaction(usize::MAX);

    let page = &world.storage.pages[page_id as usize];
    assert_eq!(page.row_count(), 3, "the orphan was reclaimed");
    assert!(
        page.order() > order_before,
        "rows changed place: the order version moves"
    );
    let position = ticks(page, ComponentKey::of::<Position>());
    assert_eq!(
        position.summary, summary_before,
        "compaction writes nothing"
    );
    assert_eq!(
        position.rows.as_ref().map(Vec::len),
        Some(page.row_count()),
        "one tick per remaining row"
    );
    for &(entity, tick) in &placed[1..] {
        let (_, row) = page_of(&world, entity, SemanticDomain::Spatial);
        assert_eq!(
            row_tick(position, row),
            tick,
            "{entity:?} keeps the tick it was placed at"
        );
    }
}

// --- moving between pages ---

/// Adding a component moves the entity's row to another page: the components
/// it already had carry their tick there — a migration is not a change, so
/// the destination's summary is raised to that tick, not to the clock — and
/// the component added is a fresh row, written now.
#[test]
fn a_migrated_row_carries_its_tick_and_is_not_a_change() {
    let mut world = world_watching_position();
    world.observe(ComponentKey::of::<Velocity>());
    advance_to(&mut world, 2);
    let entity = world.spawn(Position(1));
    let source = location(&world, entity, SemanticDomain::Spatial);

    advance_to(&mut world, 6);
    world
        .add_component(entity, Velocity(3))
        .expect("Velocity attaches");

    let (dest, row) = page_of(&world, entity, SemanticDomain::Spatial);
    assert_ne!(
        location(&world, entity, SemanticDomain::Spatial).page_id,
        source.page_id,
        "the row moved to another page"
    );
    let position = ticks(dest, ComponentKey::of::<Position>());
    assert_eq!(
        row_tick(position, row).max(position.floor.0),
        2,
        "the moved component carries the tick it was written at"
    );
    assert_eq!(
        position.summary,
        Tick(2),
        "a migration is not a change: the summary is the carried tick, not the clock's"
    );
    let velocity = ticks(dest, ComponentKey::of::<Velocity>());
    assert_eq!(
        row_tick(velocity, row),
        6,
        "the added component is a row written now"
    );
    assert_eq!(velocity.summary, Tick(6));

    // The page it left keeps its orphan row and its summary.
    let left = &world.storage.pages[source.page_id as usize];
    let left_position = ticks(left, ComponentKey::of::<Position>());
    assert_eq!(
        left_position.summary,
        Tick(2),
        "leaving a page writes nothing"
    );
    assert_eq!(
        left_position.rows.as_ref().map(Vec::len),
        Some(left.row_count())
    );
}

/// Removing a component moves the row back: the components kept carry
/// their tick again, and the page they join is not changed by it.
#[test]
fn a_row_moved_back_by_a_removal_carries_its_tick() {
    let mut world = world_watching_position();
    advance_to(&mut world, 3);
    let entity = world.spawn(Position(1));
    advance_to(&mut world, 5);
    world
        .add_component(entity, Velocity(2))
        .expect("Velocity attaches");
    advance_to(&mut world, 8);
    world
        .remove_component::<Velocity>(entity)
        .expect("Velocity detaches");

    let (page, row) = page_of(&world, entity, SemanticDomain::Spatial);
    assert_eq!(page.keys, vec![ComponentKey::of::<Position>()]);
    let position = ticks(page, ComponentKey::of::<Position>());
    assert_eq!(
        row_tick(position, row).max(position.floor.0),
        3,
        "Position carries the tick it was written at, through both moves"
    );
    assert_eq!(
        position.summary,
        Tick(3),
        "two migrations, no change: the summary never reached the clock"
    );
}

/// A row leaving a column that kept no row ticks carries the column's floor
/// — `max(τ, φ)` with τ absent — into a column that keeps them: it is
/// covered there as it was, and it is still not a change.
#[test]
fn a_row_moved_out_of_an_unobserved_column_carries_its_floor() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    advance_to(&mut world, 4);
    let entity = world.spawn(Position(1));
    {
        let (page, _) = page_of(&world, entity, SemanticDomain::Spatial);
        let position = ticks(page, ComponentKey::of::<Position>());
        assert!(position.rows.is_none(), "not watched yet: no row ticks");
    }

    advance_to(&mut world, 7);
    world.observe(ComponentKey::of::<Position>());
    advance_to(&mut world, 9);
    world
        .add_component(entity, Velocity(1))
        .expect("Velocity attaches");

    let (dest, row) = page_of(&world, entity, SemanticDomain::Spatial);
    let position = ticks(dest, ComponentKey::of::<Position>());
    assert_eq!(
        row_tick(position, row).max(position.floor.0),
        4,
        "the row carries the floor it had, the tick it was written at"
    );
    assert_eq!(
        position.summary,
        Tick(4),
        "a migration is not a change, whatever the source column kept"
    );
}

/// A component in another domain moves with the row: a mixed-domain page
/// carries every column's tick, the unwatched ones through their floor.
#[test]
fn a_migration_carries_the_ticks_of_every_domain_of_the_row() {
    let mut world = world_watching_position();
    world.observe(ComponentKey::of::<RenderId>());
    advance_to(&mut world, 2);
    let entity = world.spawn((Position(1), RenderId(5)));
    advance_to(&mut world, 11);
    world
        .add_component(entity, Velocity(0))
        .expect("Velocity attaches");

    let (page, row) = page_of(&world, entity, SemanticDomain::Render);
    let render = ticks(page, ComponentKey::of::<RenderId>());
    assert_eq!(row_tick(render, row).max(render.floor.0), 2);
    assert_eq!(render.summary, Tick(2), "the Render column was only moved");
    let velocity = ticks(page, ComponentKey::of::<Velocity>());
    assert!(velocity.rows.is_none(), "Velocity is unwatched");
    assert_eq!(
        velocity.summary,
        Tick(11),
        "the added component is written now"
    );
    assert!(velocity.floor.0 >= 11, "and covered by its floor");
}

// --- page slots recycled ---

/// A page drained by compaction gives its slot back, and the next page of
/// any signature takes it. The slot's order version must not start over: a
/// view that remembered `(page id, ω, rows)` for the page that lived there
/// would otherwise find the same key on a page holding other entities, and
/// keep a stale projection.
#[test]
fn a_recycled_page_slot_never_goes_back_on_its_order_version() {
    let mut world = world_watching_position();
    let first = world.spawn(Position(1));
    let slot = location(&world, first, SemanticDomain::Spatial).page_id;
    let seen_while_first_lived = world.storage.pages[slot as usize].order();

    // `first` leaves the page; compaction drains it and frees the slot.
    world
        .add_component(first, Velocity(1))
        .expect("Velocity attaches");
    world.run_compaction(usize::MAX);
    assert!(
        world.storage.free_pages.contains(&slot),
        "the drained page's slot is free"
    );
    let highest_seen = world.storage.pages[slot as usize]
        .order()
        .max(seen_while_first_lived);

    // Another entity of the same signature takes the slot.
    let second = world.spawn(Position(2));
    let again = location(&world, second, SemanticDomain::Spatial);
    assert_eq!(again.page_id, slot, "the freed slot is recycled");
    let page = &world.storage.pages[slot as usize];
    assert_eq!(page.row_count(), 1);
    assert!(
        page.order() > highest_seen,
        "the slot's order version went back to {} after reaching {highest_seen}: \
         (page {slot}, ω {}, 1 row) named `first`'s page and now names `second`'s",
        page.order(),
        seen_while_first_lived
    );
}

// --- domain moves ---

/// A declared component moved to another domain (a hot reload editing its
/// `[Domain(..)]`) keeps every value: its rows change page, nothing is
/// written. Like any move it carries `max(τ, φ)` and is not a change — the
/// moved column's summary stays at the tick the row was written at.
#[test]
fn a_domain_move_carries_its_rows_ticks_and_is_not_a_change() {
    use super::runtime_components::{declare, field};
    use crate::ecs::FieldKind;
    use khora_core::script::ScriptValue;

    let mut world = World::new();
    let wake = declare(
        &mut world,
        "Wake",
        SemanticDomain::Script,
        vec![field("strength", FieldKind::Int, ScriptValue::Int(1))],
    );
    world.observe(wake);
    advance_to(&mut world, 3);
    let entity = world.spawn(());
    world
        .add_runtime_component(entity, wake, &ScriptValue::Unit)
        .expect("Wake attaches");

    advance_to(&mut world, 9);
    world
        .move_component_domain(wake, SemanticDomain::Physics)
        .expect("Wake moves");

    let (page, row) = page_of(&world, entity, SemanticDomain::Physics);
    let moved = ticks(page, wake);
    assert_eq!(
        row_tick(moved, row).max(moved.floor.0),
        3,
        "the moved row carries the tick it was written at"
    );
    assert_eq!(
        moved.summary,
        Tick(3),
        "a domain move is not a change: the summary is not the clock's"
    );
}

// --- rows leaving a page ---

/// A row a migration leaves behind stays in its page until compaction, but
/// it is no longer the entity's: the page's live rows changed. Removing an
/// entity's only component of a domain touches no other page, so the page
/// it left is the only place a view can learn that the row went — its order
/// version must move, as it does for a despawn.
#[test]
fn a_row_left_behind_by_a_removal_moves_its_pages_order_version() {
    let mut world = world_watching_position();
    let stays = world.spawn(Position(1));
    let leaves = world.spawn(Position(2));
    let page_id = location(&world, leaves, SemanticDomain::Spatial).page_id;
    let before = {
        let page = &world.storage.pages[page_id as usize];
        (
            page.order(),
            ticks(page, ComponentKey::of::<Position>()).summary,
            page.row_count(),
        )
    };

    advance_to(&mut world, 5);
    world
        .remove_component::<Position>(leaves)
        .expect("Position detaches");
    assert!(
        world.get::<Position>(leaves).is_none(),
        "the component is gone"
    );
    assert!(world.get::<Position>(stays).is_some());

    let page = &world.storage.pages[page_id as usize];
    let after = (
        page.order(),
        ticks(page, ComponentKey::of::<Position>()).summary,
        page.row_count(),
    );
    assert_ne!(
        after, before,
        "the page lost a live row yet reads the same ω, σ and row count: \
         a view keyed on them keeps projecting the removed Position"
    );
}
