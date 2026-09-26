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

//! The physics lane against a real Rapier world.
//!
//! Here rather than beside the lane because the CLAD descent puts `khora-infra`
//! *above* `khora-lanes`: a lane knows `PhysicsProvider` and never Rapier. This
//! crate sees both, so it is where the two can meet.
//!
//! The lane is driven directly rather than through an agent. What is under test
//! is the translation from a backend contact to two entities and out onto the
//! deck; the agent's fixed-timestep sub-stepping is a separate question with
//! its own tests.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck, PhysicsDeltaTime, Slot};
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, ColliderDesc, ColliderShape, CollisionKind, ContactBatch, PhysicsProvider,
    RigidBodyDesc,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use khora_lanes::physics_lane::StandardPhysicsLane;

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

fn a_collider(owner: EntityId, shape: ColliderShape) -> ColliderDesc {
    ColliderDesc {
        owner: Some(owner),
        parent_body: None,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        shape,
        active_events: true,
        friction: 0.5,
        restitution: 0.0,
    }
}

/// A floor, and a ball resting in it.
///
/// Overlapping on purpose, so the contact does not depend on the integrator —
/// a test that waited for a fall would fail for reasons that have nothing to do
/// with what it checks. The ball needs a **dynamic parent body**: Rapier reports
/// no contact between two colliders that both count as fixed, and a collider
/// with no parent counts as fixed.
fn a_world_with_a_contact() -> Box<dyn PhysicsProvider> {
    let mut world = RapierPhysicsWorld::default();
    world.add_collider(a_collider(
        entity(0),
        ColliderShape::Box(Vec3::new(10.0, 0.5, 10.0)),
    ));

    let body = world.add_body(RigidBodyDesc {
        body_type: BodyType::Dynamic,
        position: Vec3::new(0.0, 0.8, 0.0),
        rotation: Quat::IDENTITY,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 1.0,
        ccd_enabled: false,
    });
    let mut ball = a_collider(entity(1), ColliderShape::Sphere(0.5));
    ball.parent_body = Some(body);
    world.add_collider(ball);

    Box::new(world)
}

fn run(provider: &mut Box<dyn PhysicsProvider>, deck: &mut OutputDeck) {
    let mut ctx = LaneContext::new();
    ctx.insert(PhysicsDeltaTime(1.0 / 60.0));
    ctx.insert(Slot::new(provider.as_mut()));
    ctx.insert(Slot::new(deck));
    StandardPhysicsLane::new()
        .execute(&mut ctx)
        .expect("the lane ran");
}

/// **What the whole collision path is for.** A contact leaves Rapier naming two
/// colliders; the lane turns those into the two entities gameplay knows, while
/// it still holds the provider, and leaves them on the deck.
///
/// Before this, every contact Rapier reported was drained into a query that
/// matched no entity and discarded — every frame, silently.
#[test]
fn a_contact_lands_on_the_deck_naming_both_entities() {
    let mut provider = a_world_with_a_contact();
    let mut deck = OutputDeck::new();

    run(&mut provider, &mut deck);

    let batch = deck.slot::<ContactBatch>();
    let started = batch
        .contacts
        .iter()
        .find(|contact| contact.kind == CollisionKind::Started)
        .expect("the overlap was reported");
    assert!(
        [started.a, started.b].contains(&entity(0)) && [started.a, started.b].contains(&entity(1)),
        "both entities named; got {started:?}"
    );
}

/// The agent runs this lane once per fixed sub-step, up to five in a frame.
/// Replacing the slot rather than extending it would keep only the last
/// sub-step's contacts and drop the rest without saying so.
#[test]
fn several_sub_steps_accumulate_on_one_deck() {
    let mut provider = a_world_with_a_contact();
    let mut deck = OutputDeck::new();

    run(&mut provider, &mut deck);
    let after_one = deck.slot::<ContactBatch>().contacts.len();
    run(&mut provider, &mut deck);

    assert!(after_one > 0, "the first sub-step reported the contact");
    assert!(
        deck.slot::<ContactBatch>().contacts.len() >= after_one,
        "the second sub-step discarded the first's"
    );
}

/// A collider the ECS does not own — a query volume, a tool's probe — has no
/// entity to name, so the contact is not a gameplay event and is dropped rather
/// than reported half-resolved.
#[test]
fn a_contact_with_an_unowned_collider_is_not_reported() {
    let mut world = RapierPhysicsWorld::default();
    let mut floor = a_collider(entity(0), ColliderShape::Box(Vec3::new(10.0, 0.5, 10.0)));
    floor.owner = None;
    world.add_collider(floor);

    let body = world.add_body(RigidBodyDesc {
        body_type: BodyType::Dynamic,
        position: Vec3::new(0.0, 0.8, 0.0),
        rotation: Quat::IDENTITY,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 1.0,
        ccd_enabled: false,
    });
    let mut ball = a_collider(entity(1), ColliderShape::Sphere(0.5));
    ball.parent_body = Some(body);
    world.add_collider(ball);

    let mut provider: Box<dyn PhysicsProvider> = Box::new(world);
    let mut deck = OutputDeck::new();

    run(&mut provider, &mut deck);

    assert!(deck.slot::<ContactBatch>().contacts.is_empty());
}
