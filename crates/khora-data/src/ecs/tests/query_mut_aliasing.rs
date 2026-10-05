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

//! What a `QueryMut` hands out may be held together.
//!
//! An iterator's items outlive the `next` call that made them: a caller may
//! collect every `&mut T` a `QueryMut` yields and write through them in any
//! order. Making the next item must therefore never claim the memory of an
//! earlier one, and no two items may name the same component. Run these under
//! Miri too (`cargo +nightly miri test -p khora-data --lib query_mut_aliasing`):
//! a reference invalidated by a later one is undefined behaviour even when the
//! plain run reads back the right values.

use std::panic::{catch_unwind, AssertUnwindSafe};

use khora_core::ecs::entity::EntityId;

use super::{Position, RenderId, Velocity};
use crate::ecs::query::Without;
use crate::ecs::query::WorldQuery;
use crate::ecs::{QueryMode, SemanticDomain, World};

fn native_world() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    for value in 1..=3 {
        world.spawn(Position(value));
    }
    world
}

#[test]
fn native_query_mut_items_held_together_stay_writable() {
    let mut world = native_world();

    // Every item of one column, alive at once.
    let mut items: Vec<&mut Position> = world.query_mut::<&mut Position>().collect();
    assert_eq!(items.len(), 3);
    for item in items.iter_mut() {
        item.0 += 100;
    }

    let mut values: Vec<i32> = world.query::<&Position>().map(|p| p.0).collect();
    values.sort();
    assert_eq!(values, vec![101, 102, 103]);
}

#[test]
fn transversal_query_mut_items_held_together_stay_writable() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    for value in 1..=3 {
        let entity = world.spawn(Position(value));
        world
            .add_component(entity, RenderId(value * 10))
            .expect("add");
    }
    assert_eq!(
        world
            .analyze_query(&<(&mut Position, &mut RenderId)>::type_ids())
            .mode,
        QueryMode::Transversal,
        "the test needs the transversal plan"
    );

    let mut items: Vec<(&mut Position, &mut RenderId)> = world
        .query_mut::<(&mut Position, &mut RenderId)>()
        .collect();
    assert_eq!(items.len(), 3);
    for (position, render) in items.iter_mut() {
        position.0 += 100;
        render.0 += 100;
    }

    let mut values: Vec<(i32, i32)> = world
        .query::<(&Position, &RenderId)>()
        .map(|(p, r)| (p.0, r.0))
        .collect();
    values.sort();
    assert_eq!(values, vec![(101, 110), (102, 120), (103, 130)]);
}

/// Runs `Q` over a world of one `Position`, and reports whether any item held
/// two references to the same component — refusing the query outright (a
/// panic) is fine; handing out an alias is not.
fn yields_an_alias<Q, F>(addresses: F) -> bool
where
    Q: WorldQuery,
    F: for<'a> Fn(&Q::Item<'a>) -> (*const Position, *const Position),
{
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.spawn(Position(1));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        world.query_mut::<Q>().any(|item| {
            let (a, b) = addresses(&item);
            std::ptr::eq(a, b)
        })
    }));
    // A refused query yields nothing at all.
    outcome.unwrap_or(false)
}

#[test]
fn query_mut_never_yields_one_component_twice() {
    let two_writes = yields_an_alias::<(&mut Position, &mut Position), _>(|(a, b)| {
        (&**a as *const Position, &**b as *const Position)
    });
    let a_write_and_a_read = yields_an_alias::<(&mut Position, &Position), _>(|(a, b)| {
        (&**a as *const Position, *b as *const Position)
    });
    let a_write_and_an_optional_write =
        yields_an_alias::<(&mut Position, Option<&mut Position>), _>(|(a, b)| {
            (
                &**a as *const Position,
                b.as_deref()
                    .map_or(std::ptr::null(), |b| b as *const Position),
            )
        });

    assert!(
        !two_writes,
        "`(&mut Position, &mut Position)` handed out two `&mut` to one component"
    );
    assert!(
        !a_write_and_a_read,
        "`(&mut Position, &Position)` handed out a `&` beside a `&mut` to one component"
    );
    assert!(
        !a_write_and_an_optional_write,
        "`(&mut Position, Option<&mut Position>)` handed out two `&mut` to one component"
    );
}

/// A world where `Position` and `Velocity` share the Spatial domain (or not,
/// with `velocity_domain`), three entities holding both.
fn two_term_world(velocity_domain: SemanticDomain) -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(velocity_domain);
    for value in 1..=3 {
        world.spawn((Position(value), Velocity(value * 10)));
    }
    world
}

fn positions_and_velocities(world: &World) -> Vec<(i32, Option<i32>)> {
    let mut rows: Vec<(i32, Option<i32>)> = world
        .query::<(EntityId, &Position)>()
        .map(|(id, p)| (p.0, world.get::<Velocity>(id).map(|v| v.0)))
        .collect();
    rows.sort();
    rows
}

#[test]
fn native_optional_query_mut_items_held_together_stay_writable() {
    let mut world = two_term_world(SemanticDomain::Spatial);

    let mut items: Vec<(&mut Position, Option<&mut Velocity>)> = world
        .query_mut::<(&mut Position, Option<&mut Velocity>)>()
        .collect();
    assert_eq!(items.len(), 3);
    for (position, velocity) in items.iter_mut() {
        position.0 += 100;
        if let Some(velocity) = velocity {
            velocity.0 += 100;
        }
    }

    assert_eq!(
        positions_and_velocities(&world),
        vec![(101, Some(110)), (102, Some(120)), (103, Some(130))]
    );
}

#[test]
fn joined_native_query_mut_items_held_together_stay_writable() {
    // `Velocity` outside the driver domain: the Native plan joins each row
    // through the world.
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Physics);
    for value in 1..=3 {
        let entity = world.spawn(Position(value));
        world
            .add_component(entity, Velocity(value * 10))
            .expect("add");
    }

    let mut items: Vec<(&mut Position, Option<&mut Velocity>)> = world
        .query_mut::<(&mut Position, Option<&mut Velocity>)>()
        .collect();
    assert_eq!(items.len(), 3);
    for (position, velocity) in items.iter_mut() {
        position.0 += 100;
        if let Some(velocity) = velocity {
            velocity.0 += 100;
        }
    }

    assert_eq!(
        positions_and_velocities(&world),
        vec![(101, Some(110)), (102, Some(120)), (103, Some(130))]
    );
}

#[test]
fn entity_scan_query_mut_items_held_together_stay_writable() {
    let mut world = two_term_world(SemanticDomain::Spatial);
    assert_eq!(
        world
            .analyze_query(&<(EntityId, Option<&mut Position>)>::type_ids())
            .mode,
        QueryMode::EntityScan
    );

    let mut items: Vec<(EntityId, Option<&mut Position>)> = world
        .query_mut::<(EntityId, Option<&mut Position>)>()
        .collect();
    assert_eq!(items.len(), 3);
    for (_, position) in items.iter_mut() {
        if let Some(position) = position {
            position.0 += 100;
        }
    }

    assert_eq!(
        positions_and_velocities(&world),
        vec![(101, Some(10)), (102, Some(20)), (103, Some(30))]
    );
}

#[test]
fn query_mut_read_and_write_items_held_together_stay_usable() {
    let mut world = two_term_world(SemanticDomain::Spatial);

    let items: Vec<(&mut Position, &Velocity)> =
        world.query_mut::<(&mut Position, &Velocity)>().collect();
    let mut sum = 0;
    for (position, velocity) in items {
        position.0 += velocity.0;
        sum += velocity.0;
    }
    assert_eq!(sum, 60);

    assert_eq!(
        positions_and_velocities(&world),
        vec![(11, Some(10)), (22, Some(20)), (33, Some(30))]
    );
}

#[test]
fn query_mut_sees_a_component_named_twice_inside_a_nested_tuple() {
    let nested = yields_an_alias::<((&mut Position,), &Position), _>(|((a,), b)| {
        (&**a as *const Position, *b as *const Position)
    });
    assert!(
        !nested,
        "`((&mut Position,), &Position)` handed out a `&` beside a `&mut` to one component"
    );
}

#[test]
fn query_mut_with_a_filter_on_its_own_written_component_is_not_refused() {
    // `Without<Position>` reaches no value: nothing to alias, so no refusal —
    // the query is simply empty.
    let mut world = native_world();
    let count = world
        .query_mut::<(&mut Position, Without<Position>)>()
        .count();
    assert_eq!(count, 0);
}
