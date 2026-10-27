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

//! The physics lane's contacts, through the agent the engine boots, against a
//! real Rapier world and against a provider whose contacts are scripted step
//! by step.
//!
//! Here rather than beside the lane because the CLAD descent puts `khora-infra`
//! *above* `khora-lanes`: a lane knows `PhysicsProvider` and never Rapier. This
//! crate sees both, so it is where the two can meet.
//!
//! Each run of the agent is one fixed sub-step; several runs into one deck are
//! the sub-steps of one frame, as the scheduler runs them.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, ColliderDesc, ColliderHandle, ColliderShape, Collision, CollisionEvent,
    CollisionKind, ContactBatch, PhysicsProvider, RigidBodyDesc,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;

use crate::physics_family::fixtures::{
    entity, physics_agent, physics_runtime, run_once, ScriptedProvider, SharedProvider,
};

fn a_collider(owner: khora_core::ecs::entity::EntityId, shape: ColliderShape) -> ColliderDesc {
    ColliderDesc {
        owner: Some(owner),
        parent_body: None,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        shape,
        active_events: true,
        friction: 0.5,
        restitution: 0.0,
        is_sensor: false,
        is_character: false,
    }
}

/// A dynamic ball overlapping a floor, `floor_owned` saying whether the floor
/// belongs to an entity.
///
/// Overlapping on purpose, so the contact does not depend on the integrator —
/// a test that waited for a fall would fail for reasons that have nothing to do
/// with what it checks. The ball needs a **dynamic parent body**: Rapier reports
/// no contact between two colliders that both count as fixed, and a collider
/// with no parent counts as fixed.
fn a_ball_in_a_floor(floor_owned: bool) -> SharedProvider {
    let mut world = RapierPhysicsWorld::default();
    let mut floor = a_collider(entity(0), ColliderShape::Box(Vec3::new(10.0, 0.5, 10.0)));
    if !floor_owned {
        floor.owner = None;
    }
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

    let provider: Box<dyn PhysicsProvider> = Box::new(world);
    Arc::new(Mutex::new(provider))
}

/// The contacts on the deck, or none when the slot was never written.
fn contacts_on(deck: &OutputDeck) -> Vec<Collision> {
    deck.get::<ContactBatch>()
        .map(|batch| batch.contacts.clone())
        .unwrap_or_default()
}

fn between(kind: CollisionKind, a: u32, b: u32) -> Collision {
    Collision {
        kind,
        a: entity(a),
        b: entity(b),
    }
}

/// **What the whole collision path is for.** A contact leaves Rapier naming two
/// colliders; the lane turns those into the two entities gameplay knows, while
/// it still holds the provider, and the agent commits them to the deck.
///
/// Before this, every contact Rapier reported was drained into a query that
/// matched no entity and discarded — every frame, silently.
#[test]
fn a_contact_lands_on_the_deck_naming_both_entities() {
    let runtime = Arc::new(physics_runtime(&a_ball_in_a_floor(true)));
    let mut agent = physics_agent();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &mut deck);

    let contacts = contacts_on(&deck);
    let started = contacts
        .iter()
        .find(|contact| contact.kind == CollisionKind::Started)
        .unwrap_or_else(|| panic!("the overlap was reported: {contacts:?}"));
    assert!(
        [started.a, started.b].contains(&entity(0)) && [started.a, started.b].contains(&entity(1)),
        "both entities named; got {started:?}"
    );
}

/// **Every sub-step's contacts reach the deck.** A frame runs the agent once
/// per fixed sub-step, up to five, and each run commits what its lane wrote.
/// A commit replacing the slot would keep only the last sub-step's contacts
/// and drop the others without saying so — a hit that began and ended within
/// one frame would never reach gameplay.
#[test]
fn contacts_from_every_sub_step_of_a_frame_reach_the_deck() {
    let pair = (ColliderHandle(0), ColliderHandle(1));
    let (provider, _steps) = ScriptedProvider::new(
        vec![
            vec![CollisionEvent::Started(pair.0, pair.1)],
            vec![CollisionEvent::Stopped(pair.0, pair.1)],
            vec![CollisionEvent::Started(pair.1, pair.0)],
        ],
        2,
    );
    let runtime = Arc::new(physics_runtime(&provider.shared()));
    let mut agent = physics_agent();
    let mut deck = OutputDeck::new();

    for _ in 0..3 {
        run_once(&mut agent, &runtime, &mut deck);
    }

    assert_eq!(
        contacts_on(&deck),
        vec![
            between(CollisionKind::Started, 0, 1),
            between(CollisionKind::Stopped, 0, 1),
            between(CollisionKind::Started, 1, 0),
        ],
        "every sub-step's contact, in the order the steps reported them"
    );
}

/// A sub-step that reports no contact leaves the earlier sub-steps' contacts
/// on the deck: Rapier reports a contact the step it begins, and is silent on
/// the steps after.
#[test]
fn a_quiet_sub_step_keeps_the_contacts_of_the_earlier_ones() {
    let runtime = Arc::new(physics_runtime(&a_ball_in_a_floor(true)));
    let mut agent = physics_agent();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &mut deck);
    let after_one = contacts_on(&deck);
    run_once(&mut agent, &runtime, &mut deck);
    let after_two = contacts_on(&deck);

    assert!(
        !after_one.is_empty(),
        "the first sub-step reported the contact"
    );
    assert_eq!(
        after_two.get(..after_one.len()),
        Some(after_one.as_slice()),
        "the second sub-step kept the first's contacts: {after_two:?}"
    );
}

/// A collider the ECS does not own — a query volume, a tool's probe — has no
/// entity to name, so the contact is not a gameplay event and is dropped rather
/// than reported half-resolved. The step still happened, and says so.
#[test]
fn a_contact_with_an_unowned_collider_is_not_reported() {
    let runtime = Arc::new(physics_runtime(&a_ball_in_a_floor(false)));
    let mut agent = physics_agent();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &mut deck);

    assert!(
        deck.contains::<khora_data::flow::PhysicsStepResult>(),
        "the step ran"
    );
    assert!(contacts_on(&deck).is_empty(), "{:?}", contacts_on(&deck));
}
