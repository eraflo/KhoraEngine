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

//! Writes the physics provider's results back to the World: body poses,
//! velocities and character moves.
//!
//! See the book: concepts/physics.md, "Who owns a pose".

use crate::ecs::SystemContext;
use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::physics::{CharacterControllerOptions, PhysicsProvider};
use khora_core::Runtime;

use crate::ecs::{
    BodyMotion, Collider, DataSystemRegistration, GlobalTransform, KinematicCharacterController,
    RigidBody, SimulatedTransform, TickPhase, Transform, World,
};
use crate::flow::PhysicsStepResult;

fn physics_world_writeback(
    world: &mut World,
    runtime: &Runtime,
    deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    // Only on a frame the lane stepped: paused, throttled or skipped, the
    // provider's state is stale.
    if !deck.contains::<PhysicsStepResult>() {
        return;
    }
    let _step = deck.take::<PhysicsStepResult>();

    let Some(provider_arc) = runtime
        .backends
        .get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()
        .cloned()
    else {
        return;
    };
    let guard = match provider_arc.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!("physics_world_writeback: provider mutex poisoned: {}", e);
            return;
        }
    };
    let provider = guard.as_ref();

    sync_from_provider(world, provider);
    resolve_characters(world, provider);
}

/// Writes each body's pose into [`SimulatedTransform`] and its velocity into
/// [`BodyMotion`], never into the authored `Transform`.
///
/// `Transform` is handed back once, when the simulation lets go of the
/// entity, in [`physics_release`](crate::ecs::systems::physics_release).
fn sync_from_provider(world: &mut World, provider: &dyn PhysicsProvider) {
    // Read first, write after: adding a component is a structural change, and
    // an entity simulated for the first time has no `SimulatedTransform` yet.
    let observed: Vec<(EntityId, SimulatedTransform, BodyMotion)> = world
        .query::<(EntityId, &RigidBody)>()
        .filter_map(|(entity, rb)| {
            let handle = rb.handle?;
            let (pos, rot) = provider.get_body_transform(handle);
            let (linear, angular) = provider.get_body_velocity(handle);
            Some((
                entity,
                SimulatedTransform::from_parts(pos, rot),
                BodyMotion { linear, angular },
            ))
        })
        .collect();

    for (entity, pose, motion) in observed {
        set_simulated(world, entity, pose);
        // Read back: the authored velocity is only a starting condition.
        match world.get_mut::<BodyMotion>(entity) {
            // Written only when it differs: a body at rest writes nothing.
            Some(mut existing) => {
                existing.set_if_neq(motion);
            }
            None => {
                let _ = world.add_component(entity, motion);
            }
        }
    }
}

/// Writes an entity's simulated pose, giving it one if this is its first frame.
fn set_simulated(world: &mut World, entity: EntityId, pose: SimulatedTransform) {
    match world.get_mut::<SimulatedTransform>(entity) {
        // Written only when it differs: a body at rest writes nothing.
        Some(mut existing) => {
            existing.set_if_neq(pose);
        }
        None => {
            let _ = world.add_component(entity, pose);
        }
    }
}

/// Moves each kinematic character by what the provider resolved, and records
/// whether it is grounded.
fn resolve_characters(world: &mut World, provider: &dyn PhysicsProvider) {
    let mut results = Vec::new();
    {
        let query = world.query_mut::<(EntityId, &mut KinematicCharacterController, &Collider)>();
        for (id, kcc, collider) in query {
            if let Some(h) = collider.handle {
                let options = CharacterControllerOptions {
                    autostep_height: kcc.autostep_height,
                    autostep_min_width: kcc.autostep_min_width,
                    autostep_enabled: kcc.autostep_enabled,
                    max_slope_climb_angle: kcc.max_slope_climb_angle,
                    min_slope_slide_angle: kcc.min_slope_slide_angle,
                    offset: kcc.offset,
                };
                let (m, g) = provider.move_character(h, kcc.desired_translation, &options);
                results.push((id, m, g));
            }
        }
    }

    for (id, m, g) in results {
        // From where the character is drawn, the pose the move was resolved
        // from: added to its last simulated pose, a character `Teleported`
        // this frame would snap back. The authored pose only before anything
        // was drawn.
        let from = world
            .get::<GlobalTransform>(id)
            .map(|drawn| (drawn.0.translation(), drawn.0.rotation()))
            .or_else(|| {
                world
                    .get::<Transform>(id)
                    .map(|placed| (placed.translation, placed.rotation))
            });
        if let Some((translation, rotation)) = from {
            set_simulated(
                world,
                id,
                SimulatedTransform::from_parts(translation + m, rotation),
            );
        }

        if let Some(mut kcc) = world.get_mut::<KinematicCharacterController>(id) {
            kcc.is_grounded = g;
            kcc.desired_translation = khora_core::math::Vec3::ZERO;
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "physics_world_writeback",
        phase: TickPhase::Maintenance,
        run: physics_world_writeback,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: crate::ecs::Role::Serves(khora_core::physics::PHYSICS),
    }
}
