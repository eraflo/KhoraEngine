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

//! A `ColliderDesc` with `is_sensor`, as the Rapier backend reads it.
//!
//! The contract: a sensor pushes nothing, weighs nothing, reports what enters
//! and leaves it whether or not `active_events` is set — a standalone sensor
//! included, against a standalone collider moved by hand the way a character's
//! is — and a character controller moves through it. `update_collider_properties`
//! turns a collider into a sensor and back.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, CharacterControllerOptions, ColliderDesc, ColliderHandle, ColliderShape,
    CollisionEvent, PhysicsProvider, RigidBodyDesc, RigidBodyHandle,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

fn body(provider: &mut dyn PhysicsProvider, body_type: BodyType, at: Vec3) -> RigidBodyHandle {
    provider.add_body(RigidBodyDesc {
        position: at,
        rotation: Quat::IDENTITY,
        body_type,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: if body_type == BodyType::Dynamic {
            1.0
        } else {
            0.0
        },
        ccd_enabled: false,
    })
}

/// A box collider owned by entity `owner`, at `position` (in its body's frame
/// when it has one), events off, solid.
fn box_desc(
    owner: u32,
    parent_body: Option<RigidBodyHandle>,
    position: Vec3,
    half: Vec3,
) -> ColliderDesc {
    ColliderDesc {
        owner: Some(entity(owner)),
        parent_body,
        position,
        rotation: Quat::IDENTITY,
        shape: ColliderShape::Box(half),
        active_events: false,
        friction: 0.5,
        restitution: 0.0,
        is_sensor: false,
        is_character: false,
    }
}

fn as_sensor(mut desc: ColliderDesc) -> ColliderDesc {
    desc.is_sensor = true;
    desc
}

/// The static ground: a 80 × 1 × 80 m box, top face at y = 0.
fn ground(provider: &mut dyn PhysicsProvider) -> ColliderHandle {
    provider.add_collider(box_desc(
        0,
        None,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(40.0, 0.5, 40.0),
    ))
}

/// A dynamic 0.6 m crate centred at `at`, owned by entity `owner`.
fn crate_at(
    provider: &mut dyn PhysicsProvider,
    owner: u32,
    at: Vec3,
) -> (RigidBodyHandle, ColliderHandle) {
    let handle = body(provider, BodyType::Dynamic, at);
    let collider = provider.add_collider(box_desc(
        owner,
        Some(handle),
        Vec3::ZERO,
        Vec3::new(0.3, 0.3, 0.3),
    ));
    (handle, collider)
}

/// Steps `frames` times, keeping every collision event reported.
fn step_collecting(provider: &mut dyn PhysicsProvider, frames: usize) -> Vec<CollisionEvent> {
    let mut events = Vec::new();
    for _ in 0..frames {
        provider.step(DT);
        events.extend(provider.take_collision_events());
    }
    events
}

/// Whether `events` hold a `Started` between `a` and `b` followed by a
/// `Stopped` between them.
fn entered_then_left(events: &[CollisionEvent], a: ColliderHandle, b: ColliderHandle) -> bool {
    let pair = |x: ColliderHandle, y: ColliderHandle| (x == a && y == b) || (x == b && y == a);
    let started = events
        .iter()
        .position(|e| matches!(e, CollisionEvent::Started(x, y) if pair(*x, *y)));
    started.is_some_and(|start| {
        events[start..]
            .iter()
            .any(|e| matches!(e, CollisionEvent::Stopped(x, y) if pair(*x, *y)))
    })
}

/// How far, in degrees, a body's up axis leans from world up.
fn tilt_degrees(rotation: Quat) -> f32 {
    let up = rotation.normalize() * Vec3::Y;
    up.normalize()
        .dot(Vec3::Y)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// **A sensor pushes nothing.** A crate dropped onto a sensor platform falls
/// through it to the ground: centre at 0.3, not resting on the platform at 1.55.
#[test]
fn a_falling_body_passes_through_a_sensor() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    ground(provider);
    provider.add_collider(as_sensor(box_desc(
        1,
        None,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(2.0, 0.25, 2.0),
    )));
    let (crate_body, _) = crate_at(provider, 2, Vec3::new(0.0, 3.0, 0.0));

    step_collecting(provider, 120);

    let (at, _) = provider.get_body_transform(crate_body);
    assert!(
        (at.y - 0.3).abs() < 0.03,
        "the crate falls through the sensor onto the ground, centre at 0.3; it is at {at:?}"
    );
}

/// **A sensor reports without being asked.** A crate falling through a sensor
/// raises `Started` then `Stopped` between the two colliders, though neither
/// desc sets `active_events`.
#[test]
fn a_sensor_reports_a_body_entering_and_leaving_without_active_events() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    ground(provider);
    let trigger = provider.add_collider(as_sensor(box_desc(
        1,
        None,
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::new(1.0, 0.25, 1.0),
    )));
    let (_, crate_collider) = crate_at(provider, 2, Vec3::new(0.0, 5.0, 0.0));

    let events = step_collecting(provider, 120);

    assert!(
        entered_then_left(&events, trigger, crate_collider),
        "the crate is reported entering then leaving the sensor; the backend reported {events:?}"
    );
}

/// **A standalone sensor sees a character.** A character's collider has no
/// body and is moved by hand every frame; a trigger volume has no body either.
/// Both count as fixed, and the default pair filter skips two fixed colliders —
/// the character's collider, marked as one, must not be skipped.
///
/// A character's box moved from x = 0 to x = 4 through a sensor wall at x = 2.
#[test]
fn a_standalone_sensor_reports_a_characters_collider_moved_through_it() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let wall = provider.add_collider(as_sensor(box_desc(
        1,
        None,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.25, 1.0, 1.0),
    )));
    let half = Vec3::new(0.3, 0.3, 0.3);
    let character = |at: Vec3| ColliderDesc {
        is_character: true,
        ..box_desc(2, None, at, half)
    };
    let mover = provider.add_collider(character(Vec3::ZERO));
    provider.step(DT);

    let mut events = Vec::new();
    for frame in 1..=80 {
        let at = Vec3::new(frame as f32 * 0.05, 0.0, 0.0);
        provider.update_collider_properties(mover, character(at));
        provider.step(DT);
        events.extend(provider.take_collision_events());
    }

    assert!(
        entered_then_left(&events, wall, mover),
        "the moved box is reported entering then leaving the standalone sensor; the backend \
         reported {events:?}"
    );
}

/// **A character moves through a sensor.** Its controller's query does not
/// treat a sensor as an obstacle: asked to move 1 m along +X from x = 1.2
/// into a sensor wall whose face is at x = 1.75, it moves the whole metre (a
/// solid wall lets it move about 0.24 m).
#[test]
fn a_character_controller_moves_through_a_sensor() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    provider.add_collider(as_sensor(box_desc(
        1,
        None,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.25, 1.0, 1.0),
    )));
    let character = provider.add_collider(box_desc(
        2,
        None,
        Vec3::new(1.2, 0.0, 0.0),
        Vec3::new(0.3, 0.3, 0.3),
    ));
    provider.step(DT);

    let options = CharacterControllerOptions {
        autostep_height: 0.3,
        autostep_min_width: 0.2,
        autostep_enabled: false,
        max_slope_climb_angle: 0.785,
        min_slope_slide_angle: 0.523,
        offset: 0.01,
    };
    let (moved, _) = provider.move_character(character, Vec3::new(1.0, 0.0, 0.0), &options);

    assert!(
        (moved.x - 1.0).abs() < 0.02,
        "the character moves the whole metre through the sensor; it moved {moved:?}"
    );
}

/// **A sensor weighs nothing.** A slab resting flat with a sensor ball held
/// 3 m out to one side stays flat: with density 1 the ball outweighs the slab
/// and the centre of mass sits past the slab's edge, so it tips.
#[test]
fn a_sensor_attached_off_centre_adds_no_weight_to_its_body() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    ground(provider);
    let slab = provider.add_body(RigidBodyDesc {
        position: Vec3::new(0.0, 0.101, 0.0),
        rotation: Quat::IDENTITY,
        body_type: BodyType::Dynamic,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 0.1,
        ccd_enabled: false,
    });
    provider.add_collider(box_desc(
        1,
        Some(slab),
        Vec3::ZERO,
        Vec3::new(0.5, 0.1, 0.5),
    ));
    let mut ball = as_sensor(box_desc(
        2,
        Some(slab),
        Vec3::new(3.0, 1.5, 0.0),
        Vec3::ZERO,
    ));
    ball.shape = ColliderShape::Sphere(0.5);
    provider.add_collider(ball);

    step_collecting(provider, 120);

    let (at, rotation) = provider.get_body_transform(slab);
    let tilt = tilt_degrees(rotation);
    assert!(
        tilt < 3.0,
        "a weightless sensor leaves the slab flat; it leans {tilt:.2}° (at {at:?})"
    );
}

/// An update that turns a solid platform into a sensor lets the crate resting
/// on it fall to the ground.
#[test]
fn an_update_making_a_collider_a_sensor_lets_what_rests_on_it_fall_through() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    ground(provider);
    let solid = box_desc(1, None, Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 0.25, 2.0));
    let platform = provider.add_collider(solid.clone());
    let (crate_body, _) = crate_at(provider, 2, Vec3::new(0.0, 1.6, 0.0));
    step_collecting(provider, 60);
    let (rested, _) = provider.get_body_transform(crate_body);
    assert!(
        (rested.y - 1.55).abs() < 0.03,
        "precondition: the crate rests on the solid platform at 1.55; it is at {rested:?}"
    );

    for _ in 0..90 {
        provider.update_collider_properties(platform, as_sensor(solid.clone()));
        provider.step(DT);
    }

    let (at, _) = provider.get_body_transform(crate_body);
    assert!(
        (at.y - 0.3).abs() < 0.03,
        "the platform made a sensor no longer holds the crate, which rests on the ground at 0.3; \
         it is at {at:?}"
    );
}

/// And back: a sensor platform lets a first crate through; updated to solid,
/// it holds a second crate up.
#[test]
fn an_update_making_a_sensor_solid_holds_up_what_falls_on_it_afterwards() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    ground(provider);
    let solid = box_desc(1, None, Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 0.25, 2.0));
    let platform = provider.add_collider(as_sensor(solid.clone()));
    let (first, _) = crate_at(provider, 2, Vec3::new(0.0, 3.0, 0.0));
    for _ in 0..120 {
        provider.update_collider_properties(platform, as_sensor(solid.clone()));
        provider.step(DT);
    }
    let (through, _) = provider.get_body_transform(first);
    assert!(
        (through.y - 0.3).abs() < 0.03,
        "the first crate falls through the sensor onto the ground at 0.3; it is at {through:?}"
    );

    let (second, _) = crate_at(provider, 3, Vec3::new(1.0, 3.0, 1.0));
    for _ in 0..120 {
        provider.update_collider_properties(platform, solid.clone());
        provider.step(DT);
    }

    let (held, _) = provider.get_body_transform(second);
    assert!(
        (held.y - 1.55).abs() < 0.03,
        "the platform, solid now, holds the second crate up at 1.55; it is at {held:?}"
    );
}
