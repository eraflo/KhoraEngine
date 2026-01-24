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

//! Physics lane - hot path for physics operations.

use khora_core::physics::{ColliderDesc, PhysicsProvider, RigidBodyDesc};
use khora_data::ecs::{Collider, GlobalTransform, RigidBody, Transform, World};

/// A trait defining the behavior of a physics lane.
pub trait PhysicsLane: Send + Sync {
    /// Returns the name of the strategy.
    fn strategy_name(&self) -> &'static str;

    /// Executes the physics step.
    fn step(&self, world: &mut World, provider: &mut dyn PhysicsProvider, dt: f32);
}

/// The standard physics lane for industrial-grade simulation.
#[derive(Debug, Default)]
pub struct StandardPhysicsLane;

impl StandardPhysicsLane {
    /// Creates a new `StandardPhysicsLane`.
    pub fn new() -> Self {
        Self
    }

    /// Synchronizes components from ECS to the physics provider.
    fn sync_to_world(&self, world: &mut World, provider: &mut dyn PhysicsProvider) {
        // 1. Handle new RigidBodies
        let query = world.query_mut::<(&GlobalTransform, &mut RigidBody)>();
        for (transform, rb) in query {
            if rb.handle.is_none() {
                let desc = RigidBodyDesc {
                    position: transform.0.translation(),
                    rotation: transform.0.rotation(),
                    body_type: rb.body_type,
                    linear_velocity: rb.linear_velocity,
                    angular_velocity: rb.angular_velocity,
                    mass: rb.mass,
                };
                rb.handle = Some(provider.add_body(desc));
            }
        }

        // 2. Handle new Colliders
        // Note: For now, we query specifically for entities with a RigidBody and Collider,
        // then for entities with only a Collider. This is because our WorldQuery doesn't support Option<&T>.

        // --- Entities with BOTH RigidBody and Collider ---
        let query = world.query_mut::<(&RigidBody, &mut Collider, &GlobalTransform)>();
        for (rb, collider, transform) in query {
            if collider.handle.is_none() {
                let desc = ColliderDesc {
                    parent_body: rb.handle,
                    position: transform.0.translation(),
                    rotation: transform.0.rotation(),
                    shape: collider.shape.clone(),
                };
                collider.handle = Some(provider.add_collider(desc));
            }
        }
    }

    /// Synchronizes components from the physics provider back to ECS.
    fn sync_from_world(&self, world: &mut World, provider: &dyn PhysicsProvider) {
        let query = world.query_mut::<(&mut Transform, &mut RigidBody)>();
        for (transform, rb) in query {
            if let Some(handle) = rb.handle {
                let (pos, rot) = provider.get_body_transform(handle);
                // Update transform
                transform.translation = pos;
                transform.rotation = rot;
                // Update velocity (optional, if we want to expose it in the component)
            }
        }
    }
}

impl PhysicsLane for StandardPhysicsLane {
    fn strategy_name(&self) -> &'static str {
        "StandardPhysics"
    }

    fn step(&self, world: &mut World, provider: &mut dyn PhysicsProvider, dt: f32) {
        // 1. Sync ECS -> Pathological World
        self.sync_to_world(world, provider);

        // 2. Execute Step
        provider.step(dt);

        // 3. Sync Pathological World -> ECS
        self.sync_from_world(world, provider);
    }
}
