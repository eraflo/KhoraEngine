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

//! Integration tests for PhysicsAgent.
//!
//! Per CLAUDE.md:
//!   - `step()` is private — tests drive the agent via `execute()`.
//!   - Raycast queries call `PhysicsProvider::cast_ray` directly on the
//!     locked backend.

use khora_agents::physics_agent::PhysicsAgent;
use khora_core::agent::Agent;
use khora_core::context::EngineContext;
use khora_core::math::Vec3;
use khora_core::physics::BodyType;
use khora_core::Runtime;
use khora_data::ecs::{RigidBody, Transform, World};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use std::sync::{Arc, Mutex};

fn make_runtime(
    provider: &Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>>,
) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(provider));
    runtime
        .resources
        .insert(khora_core::physics::collision_channel());
    Arc::new(runtime)
}

/// Runs the full physics tick `n` times: PreExtract DataSystems
/// (physics_provider_sync — ECS → provider) → Substrate-Pass flows
/// (read-only projection) → CLAD descent (the agent drives the lane →
/// provider.step) → Maintenance DataSystems (physics_world_writeback —
/// sync_from_provider).
///
/// Mirrors what `khora-sdk::EngineCore::tick_with_runtime` does in
/// production but without the renderer / scheduler scaffolding.
fn step_n(agent: &mut PhysicsAgent, world: &mut World, runtime: &Arc<Runtime>, n: usize) {
    use khora_control::substrate;
    use khora_core::lane::LaneBus;

    for _ in 0..n {
        let mut bus = LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();

        // The three pre-agent phases, in the order `EngineCore::run_app_update`
        // runs them. `PostSimulation` is the one this harness used to skip, and
        // skipping it broke the loop: `transform_propagation` lives there, and
        // it is what carries the `Transform` the writeback produced into the
        // `GlobalTransform` the next frame's provider sync reads. Without it
        // the sync pushed the *spawn* pose back into the provider every frame,
        // so a body fell one step and was teleported home, forever.
        for phase in [
            khora_data::ecs::TickPhase::PreSimulation,
            khora_data::ecs::TickPhase::PostSimulation,
            khora_data::ecs::TickPhase::PreExtract,
        ] {
            substrate::run_data_systems(world, runtime, &mut deck, phase);
        }

        // Substrate Pass — Flows project read-only Views into the bus.
        substrate::run_flows(world, &mut bus, runtime);

        // CLAD descent — agent invokes the lane (provider.step(dt)).
        // The agent's own declaration, exactly as the scheduler would stamp
        // it — a test that granted more would be testing a context the engine
        // never builds.
        let permit = agent.contention();
        let mut ctx = EngineContext::for_agent(
            khora_core::WorldAccess::Exclusive(world as &mut dyn std::any::Any),
            Arc::clone(runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);

        // Maintenance — physics_world_writeback pulls provider state back into
        // Transform / KCC, then collision_dispatch moves the lane's contacts
        // from the deck to the collision channel.
        substrate::run_data_systems(
            world,
            runtime,
            &mut deck,
            khora_data::ecs::TickPhase::Maintenance,
        );
    }
}

/// Helper to build a fresh `EngineContext` for `on_initialize` calls in tests.
/// Bus and deck are owned by the caller's stack frame.
fn make_init_ctx<'a>(
    world: &'a mut World,
    runtime: &Arc<Runtime>,
    bus: &'a khora_core::lane::LaneBus,
    deck: &'a mut khora_core::lane::OutputDeck,
) -> EngineContext<'a> {
    EngineContext::for_agent(
        khora_core::WorldAccess::Exclusive(world as &mut dyn std::any::Any),
        Arc::clone(runtime),
        bus,
        deck,
        NOTHING,
        None,
    )
}

/// `on_initialize` reaches backends, which are outside the contract.
static NOTHING: &khora_core::agent::Contention = &khora_core::agent::Contention {
    deck: Vec::new(),
    reads: Vec::new(),
    writes: Vec::new(),
};

#[test]
fn test_physics_gravity_influence() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));

    let runtime = make_runtime(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &runtime, &bus, &mut deck);
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
    step_n(&mut agent, &mut world, &runtime, 10);

    // A real distance, not merely "lower than it started". Ten steps of 1/60 s
    // under semi-implicit Euler falls `g·dt²·n(n+1)/2` ≈ 0.15 m; the window is
    // wide enough for the integrator's details and narrow enough to fail on
    // what was actually happening — one step's worth (~0.003 m) repeated
    // forever, which the old `y < 10.0` accepted without complaint.
    let fallen = 10.0 - world.get::<Transform>(entity).unwrap().translation.y;
    assert!(
        (0.10..0.20).contains(&fallen),
        "ten steps of gravity should fall about 0.15 m; fell {fallen}"
    );
}

#[test]
fn test_physics_raycast() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));

    let runtime = make_runtime(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &runtime, &bus, &mut deck);
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
    step_n(&mut agent, &mut world, &runtime, 1);

    // Raycast directly via the PhysicsProvider trait — the legacy
    // `PhysicsQueryService` façade was removed in Phase F.
    let ray = khora_core::physics::Ray {
        origin: Vec3::new(0.0, 5.0, 0.0),
        direction: Vec3::new(0.0, -1.0, 0.0),
    };

    let hit = provider.lock().unwrap().cast_ray(&ray, 10.0, true);
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

    let runtime = make_runtime(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &runtime, &bus, &mut deck);
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

    step_n(&mut agent, &mut world, &runtime, 1);

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

/// **What a scene load leaves behind, and why physics stopped after one.**
///
/// `GlobalTransform` is `Derived`, so a Recipe does not carry it and a loaded
/// entity arrives with a `Transform` alone. Every system that needs a world
/// pose asks for it by reference: propagation skipped the entity, the provider
/// sync never registered its body, and `cleanup_orphans` then removed the body
/// it had. A rigid body stopped being simulated at all — after a scene load, or
/// after pressing Stop, which restores through the same path.
#[test]
fn an_entity_loaded_without_a_global_transform_is_still_simulated() {
    let mut world = World::new();
    let provider: Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>> =
        Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));
    let runtime = make_runtime(&provider);
    let mut agent = PhysicsAgent::default();

    {
        let bus = khora_core::lane::LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = make_init_ctx(&mut world, &runtime, &bus, &mut deck);
        agent.on_initialize(&mut ctx);
    }

    // Exactly what `recipe_strategy` produces: the authored components, and no
    // derived one.
    let entity = world.spawn((
        Transform::new(Vec3::new(0.0, 10.0, 0.0), Default::default(), Vec3::ONE),
        RigidBody {
            body_type: BodyType::Dynamic,
            ..Default::default()
        },
    ));
    assert!(
        world
            .get::<khora_data::ecs::GlobalTransform>(entity)
            .is_none(),
        "the fixture is the shape a load produces"
    );

    step_n(&mut agent, &mut world, &runtime, 10);

    let fallen = 10.0 - world.get::<Transform>(entity).unwrap().translation.y;
    assert!(
        fallen > 0.10,
        "a loaded body must fall like any other; fell {fallen}"
    );
}
