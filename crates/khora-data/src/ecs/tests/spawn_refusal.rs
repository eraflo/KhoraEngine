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

//! A spawn refuses a component type nobody registered.
//!
//! A type with no registered domain has no location an entity's metadata could
//! record: stored in a page, it would be invisible to `get`, to every query and
//! to `despawn`, which would leave its row behind. `try_spawn` refuses such a
//! bundle before allocating anything; `spawn` panics on it, a programming error
//! caught the first time the code runs.

use std::panic::{catch_unwind, AssertUnwindSafe};

use khora_core::ecs::entity::EntityId;

use super::{Position, RenderId, Velocity};
use crate::ecs::{Component, SemanticDomain, SpawnError, World};

/// A component no world registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Unregistered;
impl Component for Unregistered {}

const DOMAINS: [SemanticDomain; SemanticDomain::COUNT] = [
    SemanticDomain::Spatial,
    SemanticDomain::Render,
    SemanticDomain::Audio,
    SemanticDomain::Physics,
    SemanticDomain::Ui,
    SemanticDomain::Script,
];

fn registered_world() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world
}

/// Everything a refused spawn must leave as it was.
#[derive(Debug, PartialEq)]
struct Footprint {
    entity_count: usize,
    slots: usize,
    pages: usize,
    rows: usize,
    epochs: Vec<u64>,
}

fn footprint(world: &World) -> Footprint {
    Footprint {
        entity_count: world.entity_count(),
        slots: world.entities.len(),
        pages: world.storage.pages.len(),
        rows: world.storage.pages.iter().map(|p| p.entities.len()).sum(),
        epochs: DOMAINS.iter().map(|d| world.domain_epoch(*d)).collect(),
    }
}

fn names_unregistered(error: &SpawnError) -> bool {
    match error {
        SpawnError::ComponentNotRegistered { component } => component.contains("Unregistered"),
        SpawnError::ComponentNamedTwice { .. } => false,
    }
}

/// The message of a caught panic, whichever way it was raised.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        String::new()
    }
}

#[test]
fn try_spawn_refuses_an_unregistered_component_and_allocates_nothing() {
    let mut world = registered_world();
    let keep = world.spawn(Position(7));
    let before = footprint(&world);

    let result = world.try_spawn(Unregistered);

    let error = result.expect_err("an unregistered component is refused");
    assert!(
        names_unregistered(&error),
        "the error names the type: {error:?}"
    );
    assert_eq!(footprint(&world), before, "a refused spawn stores nothing");
    assert_eq!(world.get::<Position>(keep), Some(&Position(7)));

    // No id was consumed: the next spawn takes the slot the refused one would
    // have taken, at its first generation.
    let next = world.spawn(Position(8));
    assert_eq!(
        next,
        EntityId {
            index: before.slots as u32,
            generation: 0
        }
    );
}

#[test]
fn try_spawn_refuses_a_bundle_mixing_registered_and_unregistered_components() {
    let mut world = registered_world();
    world.spawn((Position(1), Velocity(1)));
    let before = footprint(&world);

    let result = world.try_spawn((Position(2), Unregistered, RenderId(3)));

    let error = result.expect_err("a bundle holding an unregistered component is refused");
    assert!(
        names_unregistered(&error),
        "the error names the unregistered type, not a registered one: {error:?}"
    );
    assert_eq!(
        footprint(&world),
        before,
        "no page for the bundle, no row for its registered components"
    );
    assert_eq!(world.query::<&Position>().count(), 1);
    assert_eq!(world.query::<&RenderId>().count(), 0);
}

#[test]
#[should_panic(expected = "not registered")]
fn spawn_panics_on_an_unregistered_component() {
    let mut world = registered_world();
    world.spawn(Unregistered);
}

#[test]
fn spawn_panics_naming_the_unregistered_type_and_leaves_the_world_as_it_was() {
    let mut world = registered_world();
    let keep = world.spawn((Position(1), RenderId(1)));
    let before = footprint(&world);

    let unwound = catch_unwind(AssertUnwindSafe(|| {
        world.spawn((Position(2), Unregistered));
    }));

    let payload = unwound.expect_err("spawn panics on an unregistered component");
    let message = panic_message(payload.as_ref());
    assert!(
        message.contains("not registered") && message.contains("Unregistered"),
        "the panic names the problem and the type: {message:?}"
    );
    assert_eq!(footprint(&world), before);
    assert_eq!(world.get::<Position>(keep), Some(&Position(1)));
}

#[test]
fn try_spawn_of_registered_components_is_spawn() {
    let mut spawned = registered_world();
    let mut tried = registered_world();

    let a = spawned.spawn((Position(1), Velocity(2)));
    let b = spawned.spawn(RenderId(3));
    let c = spawned.spawn((Position(4), RenderId(5)));
    let d = spawned.spawn(());

    let ta = tried
        .try_spawn((Position(1), Velocity(2)))
        .expect("registered");
    let tb = tried.try_spawn(RenderId(3)).expect("registered");
    let tc = tried
        .try_spawn((Position(4), RenderId(5)))
        .expect("registered");
    let td = tried
        .try_spawn(())
        .expect("an empty bundle holds nothing to refuse");

    assert_eq!((ta, tb, tc, td), (a, b, c, d), "same ids");
    assert_eq!(footprint(&tried), footprint(&spawned), "same storage");
    for world in [&spawned, &tried] {
        assert_eq!(world.get::<Position>(a), Some(&Position(1)));
        assert_eq!(world.get::<Velocity>(a), Some(&Velocity(2)));
        assert_eq!(world.get::<RenderId>(b), Some(&RenderId(3)));
        assert_eq!(world.get::<Position>(c), Some(&Position(4)));
        assert_eq!(world.get::<RenderId>(c), Some(&RenderId(5)));
        assert!(world.contains(d));
    }
    let rows = |world: &World| {
        let mut rows: Vec<(EntityId, i32, i32)> = world
            .query::<(EntityId, &Position, &RenderId)>()
            .map(|(id, p, r)| (id, p.0, r.0))
            .collect();
        rows.sort_by_key(|(id, ..)| id.index);
        rows
    };
    assert_eq!(rows(&tried), rows(&spawned));
    assert_eq!(rows(&tried), vec![(c, 4, 5)]);
}

#[test]
fn a_try_spawned_entity_despawns_without_leaving_a_row() {
    let mut world = registered_world();
    let before = footprint(&world);
    let e = world
        .try_spawn((Position(1), RenderId(2)))
        .expect("registered");
    assert!(world.despawn(e));
    let after = footprint(&world);
    assert_eq!(after.entity_count, before.entity_count);
    assert_eq!(after.rows, before.rows, "despawn removed the row it stored");
}

#[test]
fn a_bundle_naming_a_type_twice_does_not_shift_the_next_entitys_row() {
    let mut world = registered_world();

    // One entity, one `Position` slot: the bundle names the type twice. Refused
    // (an error or a panic) or stored once — never two values for one row.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = world.try_spawn((Position(1), Position(2)));
    }));
    let next = world.spawn(Position(3));

    assert_eq!(
        world.get::<Position>(next),
        Some(&Position(3)),
        "the next entity in the page reads its own value"
    );
    for page in &world.storage.pages {
        let positions = page
            .columns
            .get(&crate::ecs::ComponentKey::of::<Position>())
            .and_then(|column| column.as_any().downcast_ref::<Vec<Position>>());
        if let Some(positions) = positions {
            assert_eq!(
                positions.len(),
                page.entities.len(),
                "the `Position` column holds one value per row"
            );
        }
    }
}

#[test]
fn try_spawn_refuses_a_type_named_twice_and_allocates_nothing() {
    let mut world = registered_world();
    world.spawn((Position(1), RenderId(1)));
    let before = footprint(&world);

    // Not adjacent, among other types.
    let result = world.try_spawn((Position(2), RenderId(3), Velocity(4), Position(5)));

    assert!(
        matches!(
            result,
            Err(SpawnError::ComponentNamedTwice { component }) if component.contains("Position")
        ),
        "the repeated type is named: {result:?}"
    );
    assert_eq!(footprint(&world), before, "a refused spawn stores nothing");
}
