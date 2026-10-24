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

//! `ColliderDesc.shape` reaches the Rapier collider at every
//! `update_collider_properties`, not only at `add_collider`: a resized
//! entity, or a shape edited in the inspector, changes what the solver sees.
//!
//! Probed the way gameplay would, with a ray cast straight down onto the
//! collider after a step.

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, ColliderDesc, ColliderHandle, ColliderShape, CollisionEvent, PhysicsProvider, Ray,
    RigidBodyDesc, RigidBodyHandle,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn desc(parent_body: Option<RigidBodyHandle>, shape: ColliderShape) -> ColliderDesc {
    ColliderDesc {
        owner: None,
        parent_body,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        shape,
        active_events: false,
        friction: 0.5,
        restitution: 0.0,
        is_sensor: false,
        is_character: false,
    }
}

fn frames_with(
    provider: &mut dyn PhysicsProvider,
    collider: ColliderHandle,
    desc: &ColliderDesc,
    frames: usize,
) {
    for _ in 0..frames {
        provider.update_collider_properties(collider, desc.clone());
        provider.step(DT);
    }
}

fn top_under(provider: &dyn PhysicsProvider, x: f32, z: f32) -> Option<f32> {
    let ray = Ray {
        origin: Vec3::new(x, 20.0, z),
        direction: Vec3::new(0.0, -1.0, 0.0),
    };
    provider
        .cast_ray(&ray, 40.0, true)
        .map(|hit| hit.position.y)
}

/// A standalone box updated from half-extent 0.5 to 1.0 grows: its top moves
/// from 0.5 to 1.0.
#[test]
fn a_standalone_collider_given_a_new_shape_by_an_update_takes_it() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let small = desc(None, ColliderShape::Box(Vec3::new(0.5, 0.5, 0.5)));
    let collider = provider.add_collider(small.clone());
    frames_with(provider, collider, &small, 2);

    let large = desc(None, ColliderShape::Box(Vec3::new(1.0, 1.0, 1.0)));
    frames_with(provider, collider, &large, 2);

    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 1.0).abs() < 0.01),
        "the box updated to half-extent 1.0 has its top at 1.0 (0.5 means the shape never \
         changed); the ray hit {top:?}"
    );
}

/// A collider attached to a static body at (0, 2, 0), updated from a sphere of
/// radius 0.5 to a capsule of half-height 1 and radius 0.5: its top moves from
/// 2.5 to 3.5, and it stays on its body.
#[test]
fn an_attached_collider_given_a_new_shape_by_an_update_takes_it_on_its_body() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let body = provider.add_body(RigidBodyDesc {
        position: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::IDENTITY,
        body_type: BodyType::Static,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 0.0,
        ccd_enabled: false,
    });
    let ball = desc(Some(body), ColliderShape::Sphere(0.5));
    let collider = provider.add_collider(ball.clone());
    frames_with(provider, collider, &ball, 2);

    let capsule = desc(Some(body), ColliderShape::Capsule(1.0, 0.5));
    frames_with(provider, collider, &capsule, 2);

    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 3.5).abs() < 0.01),
        "the capsule (half-height 1, radius 0.5) on a body at y = 2 has its top at 3.5 (2.5 \
         means the shape never changed); the ray hit {top:?}"
    );
}

/// A ground box and a 0.3 m crate resting on it, the crate's collider handle
/// and its body. Both raise collision events.
fn crate_resting_on_ground(
    provider: &mut dyn PhysicsProvider,
) -> (ColliderHandle, ColliderHandle, RigidBodyHandle) {
    let body = |position: Vec3, body_type: BodyType| RigidBodyDesc {
        position,
        rotation: Quat::IDENTITY,
        body_type,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 8.0,
        ccd_enabled: false,
    };
    let ground_body = provider.add_body(body(Vec3::new(0.0, -0.5, 0.0), BodyType::Static));
    let ground = provider.add_collider(ColliderDesc {
        active_events: true,
        ..desc(
            Some(ground_body),
            ColliderShape::Box(Vec3::new(40.0, 0.5, 40.0)),
        )
    });
    let crate_body = provider.add_body(body(Vec3::new(0.0, 0.31, 0.0), BodyType::Dynamic));
    let crate_desc = ColliderDesc {
        active_events: true,
        ..desc(
            Some(crate_body),
            ColliderShape::Box(Vec3::new(0.3, 0.3, 0.3)),
        )
    };
    let crate_collider = provider.add_collider(crate_desc.clone());
    frames_with(provider, crate_collider, &crate_desc, 30);
    (ground, crate_collider, crate_body)
}

/// **A body resting on the ground and grown in place rises out of it.** The
/// pair already holds contact points for the small box; the grown one is half
/// sunk in the floor and must be pushed up to rest with its centre at 0.6 —
/// not left where the old contacts held it.
#[test]
fn a_resting_body_grown_in_place_rises_to_its_new_height() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let (_, crate_collider, crate_body) = crate_resting_on_ground(provider);

    let grown = ColliderDesc {
        active_events: true,
        ..desc(
            Some(crate_body),
            ColliderShape::Box(Vec3::new(0.6, 0.6, 0.6)),
        )
    };
    frames_with(provider, crate_collider, &grown, 120);

    let (at, _) = provider.get_body_transform(crate_body);
    assert!(
        (at.y - 0.6).abs() < 0.03,
        "the grown crate rests with its centre at 0.6; it is at {at:?}"
    );
}

/// **Growing a resting body changes nothing about who it touches.** The crate
/// lies on the ground before and after: the ground never hears it leave or
/// land again, and its handle still names the crate's one collider.
#[test]
fn growing_a_resting_body_raises_no_contact_events_and_keeps_its_handle() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let (ground, crate_collider, crate_body) = crate_resting_on_ground(provider);
    let _ = provider.take_collision_events();
    let colliders_before = provider.get_all_colliders().len();

    let grown = ColliderDesc {
        active_events: true,
        ..desc(
            Some(crate_body),
            ColliderShape::Box(Vec3::new(0.6, 0.6, 0.6)),
        )
    };
    let mut events = Vec::new();
    for _ in 0..60 {
        provider.update_collider_properties(crate_collider, grown.clone());
        provider.step(DT);
        events.extend(provider.take_collision_events());
    }

    assert!(
        events.is_empty(),
        "the crate never left the ground, so nothing is reported; the backend reported {events:?}"
    );
    let all = provider.get_all_colliders();
    assert_eq!(all.len(), colliders_before, "still one collider per shape");
    assert!(
        all.contains(&crate_collider) && all.contains(&ground),
        "the crate's handle still names its collider: {all:?} holds {crate_collider:?}"
    );
    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 1.2).abs() < 0.05),
        "a ray down the crate's handle-named collider finds the grown top at 1.2; it hit {top:?}"
    );
}

/// **A body grown until it reaches a neighbour reports the new contact.** Two
/// boxes stand 0.2 m apart; the crate is grown to touch the other: that contact is
/// new and is reported, under the grown crate's own handle.
#[test]
fn a_body_grown_into_a_neighbour_reports_the_new_contact_under_its_handle() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let (_, crate_collider, crate_body) = crate_resting_on_ground(provider);
    let neighbour_body = provider.add_body(RigidBodyDesc {
        position: Vec3::new(0.8, 0.3, 0.0),
        rotation: Quat::IDENTITY,
        body_type: BodyType::Static,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 0.0,
        ccd_enabled: false,
    });
    let neighbour = provider.add_collider(ColliderDesc {
        active_events: true,
        ..desc(
            Some(neighbour_body),
            ColliderShape::Box(Vec3::new(0.3, 0.3, 0.3)),
        )
    });
    provider.step(DT);
    let _ = provider.take_collision_events();

    let grown = ColliderDesc {
        active_events: true,
        ..desc(
            Some(crate_body),
            ColliderShape::Box(Vec3::new(0.55, 0.3, 0.3)),
        )
    };
    let mut events = Vec::new();
    for _ in 0..5 {
        provider.update_collider_properties(crate_collider, grown.clone());
        provider.step(DT);
        events.extend(provider.take_collision_events());
    }

    assert!(
        events.iter().any(|e| matches!(
            e,
            CollisionEvent::Started(a, b)
                if (*a == crate_collider && *b == neighbour) || (*a == neighbour && *b == crate_collider)
        )),
        "the grown crate is reported touching its neighbour under its own handle; the backend \
         reported {events:?}"
    );
}
