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

//! A query's plan follows the registrations it was made from.
//!
//! The plan (Native or Transversal, and its driver domain) is cached by the
//! query's component types. It is derived from where those types are
//! registered, so a registration made after the first run of a query must not
//! leave that query running a plan for a world that no longer exists.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::{Position, RenderId};
use crate::ecs::{SemanticDomain, World};

#[test]
fn a_query_run_before_a_registration_sees_the_registered_component_after() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);

    // Run once while `RenderId` is unknown: no entity can hold it yet.
    assert_eq!(world.query::<(&Position, &RenderId)>().count(), 0);

    // Register it in another domain, then give an entity both components.
    world.register_component::<RenderId>(SemanticDomain::Render);
    let entity = world.spawn(Position(1));
    world
        .add_component(entity, RenderId(10))
        .expect("registered now");

    let rows: Vec<(i32, i32)> = world
        .query::<(&Position, &RenderId)>()
        .map(|(p, r)| (p.0, r.0))
        .collect();
    assert_eq!(rows, vec![(1, 10)], "the entity holding both is found");
}

/// The stored `Position` of `entity` is found by `get` and by a query.
fn still_reachable(world: &World, entity: khora_core::ecs::entity::EntityId) {
    assert_eq!(
        world.get::<Position>(entity),
        Some(&Position(1)),
        "the stored component is still found by `get`"
    );
    let rows: Vec<i32> = world.query::<&Position>().map(|p| p.0).collect();
    assert_eq!(rows, vec![1], "and by a query");
}

#[test]
fn registering_a_stored_component_again_keeps_it_reachable() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    let entity = world.spawn(Position(1));

    // Again, in the same domain: nothing changes.
    world.register_component::<Position>(SemanticDomain::Spatial);
    still_reachable(&world, entity);

    // Again, in another domain, while one is stored: refused, naming the type
    // and both domains.
    let refused = catch_unwind(AssertUnwindSafe(|| {
        world.register_component::<Position>(SemanticDomain::Render);
    }));
    let payload = refused.expect_err("moving a stored type to another domain is refused");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("already registered")
            && message.contains("Position")
            && message.contains("Spatial")
            && message.contains("Render"),
        "the panic names the type and both domains: {message:?}"
    );

    // The refusal changed nothing: the component is where it was.
    still_reachable(&world, entity);
}
