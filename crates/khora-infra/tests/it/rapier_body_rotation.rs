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

//! A body's rotation, given to the Rapier backend and read back through the
//! `PhysicsProvider` trait.

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{BodyType, PhysicsProvider, RigidBodyDesc};
use khora_infra::physics::rapier::RapierPhysicsWorld;

/// The angle, in degrees, of the rotation taking `a` to `b`.
fn angle_between_degrees(a: Quat, b: Quat) -> f32 {
    let dot = a.normalize().dot(b.normalize()).abs().min(1.0);
    (2.0 * dot.acos()).to_degrees()
}

fn body_with(provider: &mut dyn PhysicsProvider, rotation: Quat) -> Quat {
    let handle = provider.add_body(RigidBodyDesc {
        position: Vec3::new(1.0, 2.0, 3.0),
        rotation,
        body_type: BodyType::Static,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 0.0,
        ccd_enabled: false,
    });
    provider.step(1.0 / 60.0);
    provider.get_body_transform(handle).1
}

/// **An unrotated body read back as turned upside down.** The quaternion
/// coming out of Rapier was built with its components in the wrong order —
/// `w` in the `x` slot — so the identity came back as a half turn about X and
/// every simulated pose the write-back produced was rotated by it.
#[test]
fn an_unrotated_body_reads_back_unrotated() {
    let mut world = RapierPhysicsWorld::default();

    let read = body_with(&mut world, Quat::IDENTITY);

    let error = angle_between_degrees(read, Quat::IDENTITY);
    assert!(
        error < 0.01,
        "a body added with no rotation reads back with none; off by {error}° ({read:?})"
    );
}

/// The rotation a body is added with is the rotation the provider reports.
#[test]
fn a_rotated_body_reads_back_its_rotation() {
    let mut world = RapierPhysicsWorld::default();
    let rotation = Quat::from_axis_angle(Vec3::new(0.2, 1.0, -0.4).normalize(), 0.9);

    let read = body_with(&mut world, rotation);

    assert!(
        (read.magnitude() - 1.0).abs() < 1e-4,
        "a body's rotation is a unit quaternion; norm {}",
        read.magnitude()
    );
    let error = angle_between_degrees(read, rotation);
    assert!(
        error < 0.01,
        "a body reads back the rotation it was added with; off by {error}° ({read:?} vs {rotation:?})"
    );
}
