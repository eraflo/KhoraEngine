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
use khora_core::physics::{CharacterControllerOptions, PhysicsProvider};
use khora_core::Runtime;

use crate::ecs::{
    Collider, CollisionEvents, DataSystemRegistration, KinematicCharacterController, RigidBody,
    TickPhase, Transform, World,
};

fn physics_world_writeback(world: &mut World, runtime: &Runtime) {
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
    dispatch_collision_events(world, provider);
}

/// Pull body transforms from the provider into Transform / RigidBody.
fn sync_from_provider(world: &mut World, provider: &dyn PhysicsProvider) {
    for (transform, rb) in world.query_mut::<(&mut Transform, &mut RigidBody)>() {
        if let Some(handle) = rb.handle {
            let (pos, rot) = provider.get_body_transform(handle);
            transform.translation = pos;
            transform.rotation = rot;
        }
    }
}

/// Apply kinematic-character-controller movement results.
fn resolve_characters(world: &mut World, provider: &dyn PhysicsProvider) {
    let mut results = Vec::new();
    {
        let query =
            world.query_mut::<(EntityId, &mut KinematicCharacterController, &Collider)>();
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
        if let Some(kcc) = world.get_mut::<KinematicCharacterController>(id) {
            kcc.is_grounded = g;
            kcc.desired_translation = khora_core::math::Vec3::ZERO;
        }
        if let Some(transform) = world.get_mut::<Transform>(id) {
            transform.translation = transform.translation + m;
        }
    }
}

/// Mirror the provider's collision-event buffer into every entity that
/// declared a `CollisionEvents` component.
fn dispatch_collision_events(world: &mut World, provider: &dyn PhysicsProvider) {
    let events = provider.get_collision_events();
    for (_, buffer) in world.query_mut::<(EntityId, &mut CollisionEvents)>() {
        if events.is_empty() {
            buffer.events.clear();
        } else {
            buffer.events = events.clone();
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
