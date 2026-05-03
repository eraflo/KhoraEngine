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

//! Integration tests for PhysicsAgent and PhysicsQueryService.
//!
//! Per CLAUDE.md:
//!   - `step()` is private — tests drive the agent via `execute()`.
//!   - `cast_ray()` was removed from the agent — tests use `PhysicsQueryService`.

use khora_agents::physics_agent::PhysicsAgent;
use khora_agents::PhysicsQueryService;
use khora_core::agent::Agent;
use khora_core::context::EngineContext;
use khora_core::math::Vec3;
use khora_core::physics::BodyType;
use khora_core::service_registry::ServiceRegistry;
use khora_data::ecs::{RigidBody, Transform, World};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use std::sync::{Arc, Mutex};

fn make_services(
    provider: &Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>>,
) -> Arc<ServiceRegistry> {
    let mut services = ServiceRegistry::new();
    services.insert(Arc::clone(provider));
    Arc::new(services)
}

/// Runs `agent.execute()` N times with the given world and services.
fn step_n(agent: &mut PhysicsAgent, world: &mut World, services: &Arc<ServiceRegistry>, n: usize) {
    let bus = khora_core::lane::LaneBus::new();
    let mut deck = khora_core::lane::OutputDeck::new();
    for _ in 0..n {
        let mut ctx = EngineContext {
            world: Some(world as &mut dyn std::any::Any),
            services: Arc::clone(services),
            bus: &bus,
            deck: &mut deck,
        };
        agent.execute(&mut ctx);
    }
}

/// Helper to build a fresh `EngineContext` for `on_initialize` calls in tests.
/// Bus and deck are owned by the caller's stack frame.
fn make_init_ctx<'a>(
    world: &'a mut World,
    services: &Arc<ServiceRegistry>,
    bus: &'a khora_core::lane::LaneBus,
    deck: &'a mut khora_core::lane::OutputDeck,
) -> EngineContext<'a> {
    EngineContext {
        world: Some(world as &mut dyn std::any::Any),
        services: Arc::clone(services),
        bus,
        deck,
    }
}

#[test]
fn test_physics_gravity_influence() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));

    let services = make_services(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &services, &bus, &mut deck);
        agent.on_initialize(&mut ctx);
    }

    // Spawn a dynamic body at (0, 10, 0).
    let entity = world.spawn((
        Transform::new(Vec3::new(0.0, 10.0, 0.0), Default::default(), Vec3::ONE),
        khora_data::ecs::GlobalTransform::at_position(Vec3::new(0.0, 10.0, 0.0)),
        RigidBody {
            body_type: BodyType::Dynamic,
            ..Default::default()
        },
    ));

    // Run 10 steps (≈ 160 ms at 60 fps fixed timestep).
    step_n(&mut agent, &mut world, &services, 10);

    let transform = world.get::<Transform>(entity).unwrap();
    assert!(
        transform.translation.y < 10.0,
        "Entity should have fallen under gravity. Current Y: {}",
        transform.translation.y
    );
}

#[test]
fn test_physics_raycast() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));

    let services = make_services(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &services, &bus, &mut deck);
        agent.on_initialize(&mut ctx);
    }

    // Add a static box at origin.
    world.spawn((
        Transform::default(),
        khora_data::ecs::GlobalTransform::default(),
        RigidBody::new_static(),
        khora_data::ecs::Collider::new_box(Vec3::ONE),
    ));

    // One physics step to register the collider with the backend.
    step_n(&mut agent, &mut world, &services, 1);

    // Raycast via PhysicsQueryService — NOT via agent.cast_ray().
    let query_svc = PhysicsQueryService::new(Arc::clone(&provider));
    let ray = khora_core::physics::Ray {
        origin: Vec3::new(0.0, 5.0, 0.0),
        direction: Vec3::new(0.0, -1.0, 0.0),
    };

    let hit = query_svc.cast_ray(&ray, 10.0, true);
    assert!(hit.is_some(), "Ray should hit the static box");

    let hit = hit.unwrap();
    // Box half-extents 1 → top surface at Y = 1.0.
    assert!(
        (hit.position.y - 1.0).abs() < 0.01,
        "Hit Y should be ~1.0, got {}",
        hit.position.y
    );
}

#[test]
fn test_physics_kcc_grounding() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));

    let services = make_services(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &services, &bus, &mut deck);
        agent.on_initialize(&mut ctx);
    }

    // Static ground.
    world.spawn((
        Transform::default(),
        khora_data::ecs::GlobalTransform::default(),
        RigidBody::new_static(),
        khora_data::ecs::Collider::new_box(Vec3::new(10.0, 0.1, 10.0)),
    ));

    // Kinematic character just above ground.
    let char_id = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        khora_data::ecs::GlobalTransform::at_position(Vec3::new(0.0, 0.5, 0.0)),
        khora_data::ecs::KinematicCharacterController {
            desired_translation: Vec3::new(0.0, -0.6, 0.0),
            ..Default::default()
        },
        khora_data::ecs::Collider::new_sphere(0.3),
    ));

    step_n(&mut agent, &mut world, &services, 1);

    let kcc = world
        .get::<khora_data::ecs::KinematicCharacterController>(char_id)
        .unwrap();
    assert!(
        kcc.is_grounded,
        "Character should be grounded after moving down"
    );

    let transform = world.get::<Transform>(char_id).unwrap();
    // Sphere radius 0.3 + ground top 0.1 → sphere centre at ~0.4.
    assert!(
        transform.translation.y > 0.39 && transform.translation.y < 0.45,
        "Unexpected KCC Y: {}",
        transform.translation.y
    );
}
