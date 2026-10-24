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

//! Where a collider is, as the Rapier backend reads a `ColliderDesc`.
//!
//! The contract: `ColliderDesc.position/rotation` is the collider's pose in its
//! parent body's frame when `parent_body` is `Some`, its world pose otherwise —
//! at `add_collider` **and** at every `update_collider_properties`, which the
//! ECS sync calls each frame with the same desc.
//!
//! The provider has no collider-pose getter, so these tests ask the world the
//! way gameplay would: a ray cast straight down onto the collider, whose hit
//! height says where its top face is. Rapier refreshes collider poses only
//! inside `step`, so every probe follows a step.

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, ColliderDesc, ColliderHandle, ColliderShape, PhysicsProvider, Ray, RigidBodyDesc,
    RigidBodyHandle,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn static_body(
    provider: &mut dyn PhysicsProvider,
    position: Vec3,
    rotation: Quat,
) -> RigidBodyHandle {
    provider.add_body(RigidBodyDesc {
        position,
        rotation,
        body_type: BodyType::Static,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 0.0,
        ccd_enabled: false,
    })
}

fn box_desc(parent_body: Option<RigidBodyHandle>, position: Vec3, half: f32) -> ColliderDesc {
    ColliderDesc {
        owner: None,
        parent_body,
        position,
        rotation: Quat::IDENTITY,
        shape: ColliderShape::Box(Vec3::new(half, half, half)),
        active_events: false,
        friction: 0.5,
        restitution: 0.0,
    }
}

/// Runs `frames` frames the way the ECS sync drives the backend: the same desc
/// pushed through `update_collider_properties`, then a step.
fn frames_with_the_same_desc(
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

/// The height at which a ray cast straight down through `(x, z)` first hits
/// something, if it hits anything.
fn top_under(provider: &dyn PhysicsProvider, x: f32, z: f32) -> Option<f32> {
    let ray = Ray {
        origin: Vec3::new(x, 20.0, z),
        direction: Vec3::new(0.0, -1.0, 0.0),
    };
    provider
        .cast_ray(&ray, 40.0, true)
        .map(|hit| hit.position.y)
}

/// **The sync rewrote an attached collider's world pose with its offset.**
/// `update_collider_properties` called Rapier's `set_translation`, which is for
/// a standalone collider: on an attached one it overwrote the world pose with
/// the offset and left the offset alone. A static body never re-derives its
/// colliders' poses, so the box stayed at `o` — far from its body.
///
/// Body at `p = (0, 2, 0)`, offset `o = (0, 1, 0)`, half-extent 0.5: the top
/// face belongs at `p + o + 0.5 = 3.5`; at `o` it would read 1.5, at `2p + o`
/// 5.5.
#[test]
fn an_attached_collider_updated_every_frame_stays_at_its_body_plus_its_offset() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;

    let body = static_body(provider, Vec3::new(0.0, 2.0, 0.0), Quat::IDENTITY);
    let desc = box_desc(Some(body), Vec3::new(0.0, 1.0, 0.0), 0.5);
    let collider = provider.add_collider(desc.clone());
    provider.step(DT);

    frames_with_the_same_desc(provider, collider, &desc, 5);

    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 3.5).abs() < 0.01),
        "the attached box's top belongs at p + o + 0.5 = 3.5 (1.5 means it sits at o, \
         5.5 at 2p + o); the ray hit {top:?}"
    );
}

/// The offset is in the body's **frame**, rotation included. A body turned a
/// quarter about Y carries an offset of +X to world −Z.
///
/// Body at `(0, 2, 0)` yawed 90°, offset `(1, 0, 0)`, half-extent 0.25: the box
/// belongs at `(0, 2, −1)`, top at 2.25 — and nothing at `(1, ·, 0)`.
#[test]
fn an_attached_collider_offset_is_read_in_its_bodys_rotated_frame() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;

    let yaw = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
    let body = static_body(provider, Vec3::new(0.0, 2.0, 0.0), yaw);
    let desc = box_desc(Some(body), Vec3::new(1.0, 0.0, 0.0), 0.25);
    let collider = provider.add_collider(desc.clone());
    provider.step(DT);

    frames_with_the_same_desc(provider, collider, &desc, 5);

    let top = top_under(provider, 0.0, -1.0);
    assert!(
        top.is_some_and(|y| (y - 2.25).abs() < 0.01),
        "offset +X under a 90° yaw is world −Z: the box's top belongs at (0, 2.25, −1); \
         the ray there hit {top:?}"
    );
    let unrotated = top_under(provider, 1.0, 0.0);
    assert!(
        unrotated.is_none(),
        "nothing belongs at the unrotated offset (1, ·, 0); the ray there hit {unrotated:?}"
    );
}

/// A desc whose offset changed moves the collider **relative to its body**:
/// the new offset is applied, in the body's frame, not taken as a world pose.
#[test]
fn an_attached_collider_given_a_new_offset_moves_relative_to_its_body() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;

    let body = static_body(provider, Vec3::new(0.0, 2.0, 0.0), Quat::IDENTITY);
    let first = box_desc(Some(body), Vec3::new(0.0, 1.0, 0.0), 0.5);
    let collider = provider.add_collider(first.clone());
    provider.step(DT);
    frames_with_the_same_desc(provider, collider, &first, 2);

    let moved = box_desc(Some(body), Vec3::new(0.0, 3.0, 0.0), 0.5);
    frames_with_the_same_desc(provider, collider, &moved, 3);

    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 5.5).abs() < 0.01),
        "the new offset (0, 3, 0) on a body at (0, 2, 0) puts the top at 5.5 \
         (3.5 means the offset was read as a world position); the ray hit {top:?}"
    );
}

/// A collider with no body is placed in the world, and an update moves it to
/// exactly where the desc says. Pins the standalone half of the contract while
/// the attached half changes.
#[test]
fn a_standalone_collider_moved_by_an_update_is_where_the_desc_says() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;

    let collider = provider.add_collider(box_desc(None, Vec3::new(0.0, 1.0, 0.0), 0.5));
    provider.step(DT);

    let moved = box_desc(None, Vec3::new(0.0, 4.0, 0.0), 0.5);
    frames_with_the_same_desc(provider, collider, &moved, 3);

    let top = top_under(provider, 0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 4.5).abs() < 0.01),
        "a standalone collider moved to (0, 4, 0) has its top at 4.5; the ray hit {top:?}"
    );
}

/// A small turn is a turn. A 20 m plank turned 0.15° about Z lifts its far
/// end by 2.6 cm; the update that turns it must reach the collider, not be
/// taken for float noise from an unchanged pose.
///
/// Plank of half-extents (10, 0.05, 0.5) at the origin, standalone. Turned
/// 0.15° about Z, its top at x = 9.5 sits at 0.05 / cos θ + 9.5 · tan θ ≈ 0.0749
/// (0.05 if the turn was dropped).
#[test]
fn a_standalone_collider_turned_a_fraction_of_a_degree_turns() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;

    let mut plank = box_desc(None, Vec3::ZERO, 0.5);
    plank.shape = ColliderShape::Box(Vec3::new(10.0, 0.05, 0.5));
    let collider = provider.add_collider(plank.clone());
    provider.step(DT);

    let angle = 0.15f32.to_radians();
    let mut turned = plank.clone();
    turned.rotation = Quat::from_axis_angle(Vec3::Z, angle);
    frames_with_the_same_desc(provider, collider, &turned, 3);

    let expected = 0.05 / angle.cos() + 9.5 * angle.tan();
    let top = top_under(provider, 9.5, 0.0);
    assert!(
        top.is_some_and(|y| (y - expected).abs() < 0.002),
        "the plank turned 0.15° has its top at x = 9.5 at {expected:.4} (0.05 means the turn \
         never reached the collider); the ray hit {top:?}"
    );
}
