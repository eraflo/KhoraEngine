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

use khora_agents::physics_agent::PhysicsAgent;
use khora_core::math::Vec3;
use khora_core::physics::BodyType;
use khora_data::ecs::{RigidBody, Transform, World};
use khora_infra::physics::rapier::RapierPhysicsWorld;

#[test]
fn test_physics_gravity_influence() {
    let mut world = World::new();
    let provider = Box::new(RapierPhysicsWorld::default());
    let mut agent = PhysicsAgent::new(provider);

    // Spawn a dynamic body at (0, 10, 0)
    let entity = world.spawn((
        Transform::new(Vec3::new(0.0, 10.0, 0.0), Default::default(), Vec3::ONE),
        khora_data::ecs::GlobalTransform::at_position(Vec3::new(0.0, 10.0, 0.0)),
        RigidBody {
            body_type: BodyType::Dynamic,
            ..Default::default()
        },
    ));

    // Step physics (0.1s - enough to fall under gravity)
    for _ in 0..10 {
        agent.step(&mut world, 0.016); // 1.6ms steps * 10 = ~160ms
    }

    // Check if it fell
    let transform = world.get::<Transform>(entity).unwrap();
    assert!(
        transform.translation.y < 10.0,
        "Entity should have fallen due to gravity. Current Y: {}",
        transform.translation.y
    );
}
