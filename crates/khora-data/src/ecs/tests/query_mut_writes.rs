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

//! A `QueryMut` writes through the `&mut World` it was given, whatever plan
//! drives it.
//!
//! A query's plan — Native page rows, Native joined through the world, or
//! Transversal — is a choice of *how* to reach an entity's components. It must
//! never change *what* a write does. These tests are also meant to run under
//! Miri (`cargo +nightly miri test -p khora-data --lib query_mut_writes`): a
//! write through a pointer that only carries read permission is undefined
//! behaviour even when the plain run happens to produce the right values.

use std::collections::BTreeMap;

use khora_core::ecs::entity::EntityId;

use super::{Position, RenderId, RenderTag, Velocity};
use crate::ecs::query::WorldQuery;
use crate::ecs::{QueryMode, SemanticDomain, World};

/// The query every test here writes through: two required `&mut` terms and an
/// optional `&mut` one.
type Writes<'a> = (
    EntityId,
    &'a mut Position,
    &'a mut RenderId,
    Option<&'a mut Velocity>,
);

/// What one entity holds after the writes: `(position, render id, velocity)`.
type Held = (Option<i32>, Option<i32>, Option<i32>);

/// Runs [`Writes`] over `world`, adding 1000 to each term the entity holds,
/// and returns the entities it yielded in order of index.
fn write_all(world: &mut World) -> Vec<EntityId> {
    let mut seen = Vec::new();
    for (id, position, render, velocity) in world.query_mut::<Writes>() {
        position.0 += 1000;
        render.0 += 1000;
        if let Some(velocity) = velocity {
            velocity.0 += 1000;
        }
        seen.push(id);
    }
    seen.sort_by_key(|id| id.index);
    seen
}

fn held(world: &World, entities: &[EntityId]) -> BTreeMap<u32, Held> {
    entities
        .iter()
        .map(|&e| {
            (
                e.index,
                (
                    world.get::<Position>(e).map(|p| p.0),
                    world.get::<RenderId>(e).map(|r| r.0),
                    world.get::<Velocity>(e).map(|v| v.0),
                ),
            )
        })
        .collect()
}

fn plan_mode<Q: WorldQuery>(world: &World) -> QueryMode {
    world.analyze_query(&Q::type_ids()).mode
}

/// Whether a Native plan for `Q` joins each row through the world rather than
/// reading the driver page row.
fn joins_through_the_world<Q: WorldQuery>(world: &World) -> bool {
    let plan = world.analyze_query(&Q::type_ids());
    world
        .native_row_plan(&plan, &Q::without_type_ids(), &Q::optional_type_ids())
        .fetch_from_world
}

#[test]
fn transversal_query_mut_writes_through_the_world_it_was_given() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // Co-located, without the optional term.
    let bare = world.spawn((Position(1), RenderId(10)));
    // Co-located, with it.
    let moving = world.spawn((Position(2), Velocity(20), RenderId(200)));
    // Its Spatial and Render rows in different pages.
    let split = world.spawn((Position(3), Velocity(30)));
    world.add_component(split, RenderId(300)).expect("add");
    // Its Render row migrated, leaving an orphan behind.
    let migrated = world.spawn((Position(4), RenderId(400)));
    world.add_component(migrated, RenderTag).expect("add");
    // Lacking a required term: never yielded, never written.
    let spatial_only = world.spawn((Position(5), Velocity(50)));
    let render_only = world.spawn(RenderId(600));

    assert_eq!(
        plan_mode::<Writes>(&world),
        QueryMode::Transversal,
        "the test needs the transversal plan"
    );

    let seen = write_all(&mut world);

    assert_eq!(seen, vec![bare, moving, split, migrated], "each once");
    let all = [bare, moving, split, migrated, spatial_only, render_only];
    let expected: BTreeMap<u32, Held> = [
        (bare.index, (Some(1001), Some(1010), None)),
        (moving.index, (Some(1002), Some(1200), Some(1020))),
        (split.index, (Some(1003), Some(1300), Some(1030))),
        (migrated.index, (Some(1004), Some(1400), None)),
        (spatial_only.index, (Some(5), None, Some(50))),
        (render_only.index, (None, Some(600), None)),
    ]
    .into_iter()
    .collect();
    assert_eq!(held(&world, &all), expected);

    // The writes stay written: a second pass sees them and adds again.
    write_all(&mut world);
    assert_eq!(world.get::<Position>(split), Some(&Position(2003)));
    assert_eq!(world.get::<RenderId>(split), Some(&RenderId(2300)));
    assert_eq!(world.get::<Velocity>(moving), Some(&Velocity(2020)));
}

/// Builds the same entities in a world whose registrations make [`Writes`]
/// run under one plan or another.
///
/// - `render_domain`: where `RenderId` lives — `Render` makes the query
///   Transversal, `Spatial` keeps it Native.
/// - `velocity_domain`: where `Velocity` lives — outside the driver domain, the
///   Native plan joins its optional term through the world.
fn world_with(
    render_domain: SemanticDomain,
    velocity_domain: SemanticDomain,
) -> (World, Vec<EntityId>) {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(render_domain);
    world.register_component::<Velocity>(velocity_domain);
    world.register_component::<RenderTag>(render_domain);

    let co_located = world.spawn((Position(1), RenderId(10)));
    let with_velocity = world.spawn((Position(2), RenderId(20), Velocity(200)));
    let added = world.spawn((Position(3), Velocity(300)));
    world.add_component(added, RenderId(30)).expect("add");
    let tagged = world.spawn((Position(4), RenderId(40)));
    world.add_component(tagged, RenderTag).expect("add");
    let gone = world.spawn((Position(5), RenderId(50)));
    assert!(world.despawn(gone));
    let position_only = world.spawn(Position(6));
    let render_only = world.spawn(RenderId(70));
    let full = world.spawn((Position(8), RenderId(80), Velocity(800)));

    let entities = vec![
        co_located,
        with_velocity,
        added,
        tagged,
        position_only,
        render_only,
        full,
    ];
    (world, entities)
}

#[test]
fn native_and_transversal_query_mut_agree() {
    let (mut native, entities) = world_with(SemanticDomain::Spatial, SemanticDomain::Spatial);
    let (mut joined, joined_entities) =
        world_with(SemanticDomain::Spatial, SemanticDomain::Physics);
    let (mut transversal, transversal_entities) =
        world_with(SemanticDomain::Render, SemanticDomain::Spatial);
    assert_eq!(joined_entities, entities);
    assert_eq!(transversal_entities, entities);

    // Each world drives the same query under a different plan.
    assert_eq!(plan_mode::<Writes>(&native), QueryMode::Native);
    assert!(!joins_through_the_world::<Writes>(&native));
    assert_eq!(plan_mode::<Writes>(&joined), QueryMode::Native);
    assert!(joins_through_the_world::<Writes>(&joined));
    assert_eq!(plan_mode::<Writes>(&transversal), QueryMode::Transversal);

    let seen_native = write_all(&mut native);
    let seen_joined = write_all(&mut joined);
    let seen_transversal = write_all(&mut transversal);

    let [co_located, with_velocity, added, tagged, position_only, render_only, full] = entities[..]
    else {
        unreachable!("seven entities")
    };
    let yielded = vec![co_located, with_velocity, added, tagged, full];
    assert_eq!(seen_native, yielded);
    assert_eq!(seen_joined, yielded);
    assert_eq!(seen_transversal, yielded);

    let expected: BTreeMap<u32, Held> = [
        (co_located.index, (Some(1001), Some(1010), None)),
        (with_velocity.index, (Some(1002), Some(1020), Some(1200))),
        (added.index, (Some(1003), Some(1030), Some(1300))),
        (tagged.index, (Some(1004), Some(1040), None)),
        (position_only.index, (Some(6), None, None)),
        (render_only.index, (None, Some(70), None)),
        (full.index, (Some(1008), Some(1080), Some(1800))),
    ]
    .into_iter()
    .collect();
    assert_eq!(held(&native, &entities), expected, "native page rows");
    assert_eq!(held(&joined, &entities), expected, "native joined rows");
    assert_eq!(held(&transversal, &entities), expected, "transversal");
}
