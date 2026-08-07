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

//! Every placed entity has a world pose.
//!
//! [`GlobalTransform`] is `provenance = Derived`, so it is **not** written to a
//! scene file — a duplicate must not carry a stale copy, because the engine
//! regenerates it. Nothing regenerated it: a scene-loaded entity arrived with a
//! `Transform` and no `GlobalTransform`, and every system that needs one asks
//! for it by reference.
//!
//! What that cost, all of it silent:
//!
//! - `transform_propagation`'s root query is
//!   `(&Transform, &mut GlobalTransform, Without<Parent>)`, so the entity was
//!   never propagated — and never given one either, since a query cannot add a
//!   component.
//! - `sync_rigid_bodies` queries `&GlobalTransform`, so the entity's body was
//!   never registered with the physics provider.
//! - Absent from that frame's `active_bodies`, `cleanup_orphans` then **removed**
//!   the body it had from a previous run.
//!
//! So loading a scene — or pressing Stop, which restores through the same
//! path — left every rigid body unsimulated. It fell in the editor before Play
//! and then stopped forever, which reads as physics being broken rather than as
//! a missing component.
//!
//! Regenerating derived state is what `Derived` *means*, and an invariant
//! restored by a registered `DataSystem` is how `RULES.md` §3 says to do it.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, GlobalTransform, TickPhase, Transform, World};

fn ensure_global_transform(world: &mut World, _runtime: &Runtime, _deck: &mut OutputDeck) {
    // Collected before adding, because adding a component is a structural
    // change and the query borrows the world.
    let missing: Vec<(EntityId, Transform)> = world
        .query::<(EntityId, &Transform)>()
        .filter(|(entity, _)| world.get::<GlobalTransform>(*entity).is_none())
        .map(|(entity, transform)| (entity, *transform))
        .collect();

    for (entity, transform) in missing {
        // Seeded from the entity's own local pose rather than the identity.
        // `transform_propagation` overwrites it later in this same phase, but
        // `capture_previous_transform` reads it first — and recording the
        // origin as where a newly loaded entity "was" would make the renderer
        // interpolate it in from nowhere on its first frame.
        let _ = world.add_component(entity, GlobalTransform(transform.to_mat4().into()));
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "ensure_global_transform",
        phase: TickPhase::PostSimulation,
        run: ensure_global_transform,
        // Before `capture_previous_transform` (-10) and `transform_propagation`
        // (0), which both need the component to exist before they can do
        // anything with it.
        order_hint: -20,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::Vec3;

    fn run(world: &mut World) {
        ensure_global_transform(world, &Runtime::default(), &mut OutputDeck::new());
    }

    /// **What a scene load produces.** The Recipe carries `Transform` and skips
    /// `GlobalTransform`, so this is the exact shape an entity arrives in.
    #[test]
    fn a_placed_entity_without_one_is_given_one() {
        let mut world = World::new();
        let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));

        run(&mut world);

        let global = world
            .get::<GlobalTransform>(entity)
            .expect("given a world pose");
        assert_eq!(global.0.translation(), Vec3::new(1.0, 2.0, 3.0));
    }

    /// Seeded from the entity's own pose, not the origin: the renderer
    /// interpolates from the previous world pose, and starting at zero would
    /// slide a loaded entity in from nowhere on its first frame.
    #[test]
    fn the_one_it_is_given_is_where_the_entity_is() {
        let mut world = World::new();
        let entity = world.spawn(Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)));

        run(&mut world);

        assert_ne!(
            world
                .get::<GlobalTransform>(entity)
                .unwrap()
                .0
                .translation(),
            Vec3::ZERO
        );
    }

    /// An entity that has one is left alone — this runs before propagation,
    /// which is what actually computes the value.
    #[test]
    fn an_entity_that_has_one_is_untouched() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            GlobalTransform::at_position(Vec3::new(9.0, 9.0, 9.0)),
        ));

        run(&mut world);

        assert_eq!(
            world
                .get::<GlobalTransform>(entity)
                .unwrap()
                .0
                .translation(),
            Vec3::new(9.0, 9.0, 9.0),
            "not recomputed here; propagation owns that"
        );
    }

    /// An entity with no `Transform` is not placed and gets nothing. A world
    /// pose for something that has no pose would be a lie the queries believe.
    #[test]
    fn an_entity_with_no_transform_is_not_given_a_world_pose() {
        let mut world = World::new();
        let entity = world.spawn(crate::ecs::Name::new("just a name"));

        run(&mut world);

        assert!(world.get::<GlobalTransform>(entity).is_none());
    }
}
