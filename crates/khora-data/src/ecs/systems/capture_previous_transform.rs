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

//! Snapshots each simulated body's world-space transform into the engine's
//! [`TransformInterpolation`] store so the render path can interpolate between
//! the prior and current [`GlobalTransform`] for smooth motion at any frame
//! rate.
//!
//! Runs in [`TickPhase::PostSimulation`] **before** `transform_propagation`
//! (lower `order_hint`): it captures the still-current `GlobalTransform` — the
//! value produced by the previous frame's propagation — *before* propagation
//! overwrites it with this frame's simulated pose. Render then blends
//! `previous → current` by the `Time::interpolation_alpha`.
//!
//! The snapshots live in [`Runtime::resources`](khora_core::Runtime), NOT in
//! the ECS: interpolation is a render-only representation ("adapt the HOW,
//! never the WHAT"), so it carries no game meaning and must not appear in the
//! editor inspector or scene files. Only simulated entities (those carrying a
//! [`RigidBody`]) are snapshotted; everything else renders at its current
//! transform.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::interpolation::{SharedTransformInterpolation, TransformInterpolation};

use crate::ecs::{DataSystemRegistration, GlobalTransform, RigidBody, TickPhase, World};

fn capture_previous_transform(world: &World, store: &mut TransformInterpolation) {
    // Record the current world-space transform of every simulated body and
    // track which ids are still live so despawned/no-longer-simulated entries
    // can be pruned in the same pass (bounds memory, avoids stale generations).
    let mut live: HashSet<EntityId> = HashSet::new();
    for (id, gt) in world.query::<(EntityId, &GlobalTransform)>() {
        if world.get::<RigidBody>(id).is_some() {
            store.record(id, gt.0);
            live.insert(id);
        }
    }
    store.retain_live(&live);
}

/// Wrapper matching the `DataSystemRegistration::run` signature. Resolves the
/// shared interpolation store from the runtime; a no-op when it isn't
/// registered (e.g. headless data-layer tests that don't render).
fn capture_previous_transform_entry(
    world: &mut World,
    runtime: &khora_core::Runtime,
    _deck: &mut khora_core::lane::OutputDeck,
) {
    let Some(shared) = runtime.resources.get::<SharedTransformInterpolation>() else {
        return;
    };
    let mut store = match shared.write() {
        Ok(s) => s,
        Err(_) => {
            log::error!("capture_previous_transform: interpolation store lock poisoned");
            return;
        }
    };
    capture_previous_transform(world, &mut store);
}

inventory::submit! {
    DataSystemRegistration {
        name: "capture_previous_transform",
        phase: TickPhase::PostSimulation,
        run: capture_previous_transform_entry,
        // Must run before transform_propagation (order_hint 0) so it reads the
        // GlobalTransform from the *previous* frame's propagation.
        order_hint: -10,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{GlobalTransform, RigidBody, SemanticDomain, Transform, World};
    use khora_core::math::Vec3;

    fn register(world: &mut World) {
        world.register_component::<Transform>(SemanticDomain::Spatial);
        world.register_component::<GlobalTransform>(SemanticDomain::Spatial);
        world.register_component::<RigidBody>(SemanticDomain::Physics);
    }

    #[test]
    fn records_previous_for_rigid_body() {
        let mut world = World::default();
        register(&mut world);
        let mut store = TransformInterpolation::new();

        let body = world.spawn((
            GlobalTransform::at_position(Vec3::new(1.0, 2.0, 3.0)),
            RigidBody::default(),
        ));

        assert!(store.previous(body).is_none());
        capture_previous_transform(&world, &mut store);

        assert_eq!(
            store.previous(body).map(|t| t.translation()),
            Some(Vec3::new(1.0, 2.0, 3.0)),
            "a simulated body's transform should be snapshotted"
        );
    }

    #[test]
    fn updates_previous_to_current_each_pass() {
        let mut world = World::default();
        register(&mut world);
        let mut store = TransformInterpolation::new();

        let body = world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 0.0, 0.0)),
            RigidBody::default(),
        ));
        capture_previous_transform(&world, &mut store);

        // Move the body, then capture again: the snapshot mirrors the (now
        // current) GlobalTransform that propagation will overwrite next.
        if let Some(gt) = world.get_mut::<GlobalTransform>(body) {
            *gt = GlobalTransform::at_position(Vec3::new(5.0, 0.0, 0.0));
        }
        capture_previous_transform(&world, &mut store);

        assert_eq!(
            store.previous(body).map(|t| t.translation()),
            Some(Vec3::new(5.0, 0.0, 0.0))
        );
    }

    #[test]
    fn static_entity_without_rigid_body_is_not_snapshotted() {
        let mut world = World::default();
        register(&mut world);
        let mut store = TransformInterpolation::new();

        let stat = world.spawn(GlobalTransform::at_position(Vec3::new(1.0, 1.0, 1.0)));
        capture_previous_transform(&world, &mut store);

        assert!(
            store.previous(stat).is_none(),
            "a static entity must not be snapshotted"
        );
    }

    #[test]
    fn despawned_body_is_pruned_next_pass() {
        let mut world = World::default();
        register(&mut world);
        let mut store = TransformInterpolation::new();

        let body = world.spawn((
            GlobalTransform::at_position(Vec3::new(2.0, 0.0, 0.0)),
            RigidBody::default(),
        ));
        capture_previous_transform(&world, &mut store);
        assert!(store.previous(body).is_some());

        world.despawn(body);
        capture_previous_transform(&world, &mut store);
        assert!(
            store.previous(body).is_none(),
            "a despawned body's snapshot must be pruned"
        );
        assert!(store.is_empty());
    }
}
