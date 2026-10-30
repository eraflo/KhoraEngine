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

//! An observed column keeps one tick per row only where it moves: writable
//! access gives row ticks to the pages it can reach, never to a page of the
//! component it cannot write.

use khora_core::ecs::entity::EntityId;

use super::page_ticks::{page_of, ticks};
use super::{Position, Velocity};
use crate::ecs::{ComponentKey, SemanticDomain, World};

fn position() -> ComponentKey {
    ComponentKey::of::<Position>()
}

/// One entity holding `Position` alone (its own page), one holding
/// `Position` and `Velocity` (another page), both placed before anyone
/// watched `Position` — so neither page keeps row ticks for it — and then
/// `Position` watched, as a script's component is at run time.
fn watched_late() -> (World, EntityId, EntityId) {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    let alone = world.spawn(Position(1));
    let paired = world.spawn((Position(2), Velocity(2)));
    world.observe(position());
    for entity in [alone, paired] {
        let (page, _) = page_of(&world, entity, SemanticDomain::Spatial);
        assert!(
            ticks(page, position()).rows.is_none(),
            "placed unwatched: no row ticks yet"
        );
    }
    (world, alone, paired)
}

fn keeps_row_ticks(world: &World, entity: EntityId) -> bool {
    let (page, _) = page_of(world, entity, SemanticDomain::Spatial);
    ticks(page, position()).rows.is_some()
}

#[test]
fn a_writable_query_gives_row_ticks_only_to_the_pages_it_reaches() {
    let (mut world, alone, paired) = watched_late();

    for (mut position, velocity) in world.query_mut::<(&mut Position, &Velocity)>() {
        position.0 += velocity.0;
    }

    assert!(
        keeps_row_ticks(&world, paired),
        "the page written keeps them"
    );
    assert!(
        !keeps_row_ticks(&world, alone),
        "a page the query cannot reach (no `Velocity`) moved nothing: it keeps no row ticks"
    );
}

#[test]
fn get_many_mut_gives_row_ticks_only_to_the_pages_of_the_entities_named() {
    let (mut world, alone, paired) = watched_late();

    let [guard] = world.get_many_mut::<Position, 1>([paired]);
    guard.expect("a Position").0 = 5;

    assert!(
        keeps_row_ticks(&world, paired),
        "the page written keeps them"
    );
    assert!(
        !keeps_row_ticks(&world, alone),
        "a page holding no entity named moved nothing: it keeps no row ticks"
    );
}
