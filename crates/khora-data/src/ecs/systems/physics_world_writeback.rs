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

//! Pulls per-body transforms, kinematic results, and collision events
//! from the physics provider back into the ECS World.
//!
//! Replaces `StandardPhysicsLane::sync_from_world` /
//! `resolve_characters` / `dispatch_events` (which queried the World
//! directly inside the Lane and violated the CLAD rule). Runs in
//! `Maintenance` phase, after the scheduler has executed the physics
//! agent and the lane has called `provider.step(dt)`.

use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::physics::{CharacterControllerOptions, PhysicsProvider};
use khora_core::Runtime;

use crate::ecs::{
    Collider, DataSystemRegistration, KinematicCharacterController, RigidBody, SimulatedTransform,
    TickPhase, Transform, World,
};
use crate::flow::PhysicsStepResult;

fn physics_world_writeback(world: &mut World, runtime: &Runtime, deck: &mut OutputDeck) {
    // Only writeback if the physics lane actually advanced the simulation
    // this frame (it publishes a `PhysicsStepResult` slot when it ran).
    // When the agent is paused, throttled, or the lane was skipped for
    // any reason, we don't want to pull stale provider state.
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

/// Pull body poses from the provider into [`SimulatedTransform`].
///
/// **Not into `Transform`**, which it used to overwrite every frame.
/// `Transform` is `Authored` — `ComponentProvenance` says an author writes it —
/// so a designer's placement survived exactly until the first frame of physics.
/// Saving mid-play recorded the fallen pose over it, the inspector showed a
/// number nobody typed, and a gizmo anchored wherever gravity had left the body.
fn sync_from_provider(world: &mut World, provider: &dyn PhysicsProvider) {
    // Read first, write after: adding a component is a structural change, and
    // an entity simulated for the first time has no `SimulatedTransform` yet.
    let poses: Vec<(EntityId, SimulatedTransform)> = world
        .query::<(EntityId, &RigidBody)>()
        .filter_map(|(entity, rb)| {
            let handle = rb.handle?;
            let (pos, rot) = provider.get_body_transform(handle);
            Some((entity, SimulatedTransform::from_parts(pos, rot)))
        })
        .collect();

    for (entity, pose) in poses {
        set_simulated(world, entity, pose);
    }
}

/// Writes an entity's simulated pose, giving it one if this is its first frame.
fn set_simulated(world: &mut World, entity: EntityId, pose: SimulatedTransform) {
    match world.get_mut::<SimulatedTransform>(entity) {
        Some(existing) => *existing = pose,
        None => {
            let _ = world.add_component(entity, pose);
        }
    }
}

/// Apply kinematic-character-controller movement results.
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
        // The controller's resolved movement is simulation output like any
        // other, so it lands where the body pose lands. Seeded from the
        // authored placement on the first frame, because a character with no
        // rigid body has nothing else to start from.
        let from = world
            .get::<SimulatedTransform>(id)
            .map(|pose| (pose.translation(), pose.rotation()))
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

        if let Some(kcc) = world.get_mut::<KinematicCharacterController>(id) {
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
    }
}
