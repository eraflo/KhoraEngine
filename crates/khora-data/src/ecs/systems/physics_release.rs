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

//! Hands an entity back to its author when the simulation lets go of it.
//!
//! An entity is simulated while it has a `RigidBody`, or is a character (a
//! `KinematicCharacterController` with a `Collider`). Its pose then lives in
//! [`SimulatedTransform`], which `transform_propagation` prefers over the
//! authored `Transform`. Nothing used to remove it: an entity that lost its
//! body kept the last simulated pose forever, drawn there whatever its
//! `Transform` said, and a later move of it did nothing.
//!
//! # Where it is, not where it started
//!
//! Released, the entity stays **where it is**: its `Transform` is rewritten to
//! the pose it is drawn at, in its parent's frame, and the runtime components
//! go. This is the one write of an `Authored` component the physics makes, and
//! it happens once, at a release an author caused by removing the body. The
//! alternatives are worse:
//!
//! - dropping the simulated pose would send the entity back to its authored
//!   one — a system that detaches far bodies and re-attaches near ones would
//!   teleport every far crate to where the level placed it;
//! - keeping the simulated pose would leave a runtime component nothing
//!   maintains, a released child that does not follow its parent, and a save
//!   that reloads the entity somewhere it is not drawn.
//!
//! Runs in `PostSimulation` **after** `transform_propagation`, so the frame of
//! the release is drawn from the simulated pose like the one before it, and the
//! parent's world pose read here is this frame's.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::math::{AffineTransform, Mat4, Quaternion, Vec3, Vec4};
use khora_core::Runtime;

use crate::ecs::{
    BodyMotion, Collider, DataSystemRegistration, GlobalTransform, KinematicCharacterController,
    Parent, RigidBody, SimulatedTransform, Teleported, TickPhase, Transform, World,
};

/// Releases every entity that carries a simulated pose and is no longer
/// simulated.
pub fn physics_release_system(world: &mut World) {
    let released: Vec<EntityId> = world
        .query::<(EntityId, &SimulatedTransform)>()
        .map(|(entity, _)| entity)
        .filter(|&entity| !is_simulated(world, entity))
        .collect();

    for entity in released {
        // A move declared this frame already won: propagation drew the
        // authored pose, and the author's `Transform` is the one to keep.
        if world.get::<Teleported>(entity).is_none() {
            if let Some(placed) = handed_back(world, entity) {
                if let Some(transform) = world.get_mut::<Transform>(entity) {
                    *transform = placed;
                }
            }
        }
        let _ = world.remove_component::<SimulatedTransform>(entity);
        let _ = world.remove_component::<BodyMotion>(entity);
    }
}

/// Whether something still drives the entity's pose.
fn is_simulated(world: &World, entity: EntityId) -> bool {
    world.get::<RigidBody>(entity).is_some()
        || (world.get::<KinematicCharacterController>(entity).is_some()
            && world.get::<Collider>(entity).is_some())
}

/// The local `Transform` that composes, under the entity's parent as it is
/// this frame, to the pose the entity is drawn at.
///
/// Exact unless the parent is scaled unevenly and the entity is turned against
/// its axes: the hierarchy then draws a sheared child, which no `Transform`
/// avoids, while a simulated pose is drawn unsheared. The entity keeps its
/// place and its turn, and takes the shape any child of that parent has.
fn handed_back(world: &World, entity: EntityId) -> Option<Transform> {
    let drawn = Mat4::from(world.get::<GlobalTransform>(entity)?.0);
    let local = match world.get::<Parent>(entity) {
        Some(parent) => inverse_of(Mat4::from(world.get::<GlobalTransform>(parent.0)?.0))? * drawn,
        None => drawn,
    };
    let authored = world.get::<Transform>(entity)?;
    decompose(local, authored)
}

/// The inverse of a parent's world matrix, whatever its size.
///
/// A general inverse refuses any matrix whose determinant is under a fixed
/// bound, and a parent at a centimetre scale — what a model exported in
/// centimetres gets — is far under it: the release would fail and the entity
/// jump back to its authored pose. Dividing the scale out first leaves a
/// matrix of unit axes to invert, and the scale is undone exactly.
fn inverse_of(parent: Mat4) -> Option<Mat4> {
    let affine = AffineTransform::from(parent);
    let scale = affine.scale();
    if !(scale.min_element() > 0.0 && is_finite(scale)) {
        return None;
    }
    let unit = parent * Mat4::from_scale(Vec3::new(1.0 / scale.x, 1.0 / scale.y, 1.0 / scale.z));
    let inverse = unit.affine_inverse()?;
    Some(Mat4::from_scale(Vec3::new(1.0 / scale.x, 1.0 / scale.y, 1.0 / scale.z)) * inverse)
}

/// `local` as a translation, a rotation and a scale, the mirror where the
/// author put it.
///
/// A mirrored matrix has several decompositions — (1, 1, −1) is also
/// (−1, 1, 1) after a half turn — and they draw the same. The one kept is the
/// one with the signs of the entity's own `Transform`, so a mirrored crate set
/// down keeps the numbers its author typed; the first axis takes the mirror
/// when those signs no longer match.
fn decompose(local: Mat4, authored: &Transform) -> Option<Transform> {
    let affine = AffineTransform::from(local);
    let lengths = affine.scale();
    if !(lengths.min_element() > f32::EPSILON && is_finite(lengths)) {
        return None;
    }
    let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
    let mut signs = Vec3::new(
        sign(authored.scale.x),
        sign(authored.scale.y),
        sign(authored.scale.z),
    );
    let mirrored = local.determinant() < 0.0;
    if (signs.x * signs.y * signs.z < 0.0) != mirrored {
        signs = Vec3::new(if mirrored { -1.0 } else { 1.0 }, 1.0, 1.0);
    }
    let scale = Vec3::new(
        lengths.x * signs.x,
        lengths.y * signs.y,
        lengths.z * signs.z,
    );
    let axis = |column: usize, by: f32| {
        let c = local.cols[column];
        Vec4::new(c.x / by, c.y / by, c.z / by, 0.0)
    };
    let basis = Mat4::from_cols(
        axis(0, scale.x),
        axis(1, scale.y),
        axis(2, scale.z),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
    );
    let rotation = Quaternion::from_rotation_matrix(&basis).normalize();
    Some(Transform::new(affine.translation(), rotation, scale))
}

fn is_finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn physics_release_entry(world: &mut World, _runtime: &Runtime, _deck: &mut OutputDeck) {
    physics_release_system(world);
}

inventory::submit! {
    DataSystemRegistration {
        name: "physics_release",
        phase: TickPhase::PostSimulation,
        run: physics_release_entry,
        order_hint: 0,
        runs_after: &["transform_propagation"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::{Quat, Vec3};

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_released_root_takes_the_pose_it_is_drawn_at() {
        let mut world = World::new();
        let turn = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.7);
        let drawn = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
            * Mat4::from_quat(turn)
            * Mat4::from_scale(Vec3::new(2.0, 2.0, 2.0));
        let entity = world.spawn((
            Transform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(2.0, 2.0, 2.0)),
            GlobalTransform::new(drawn),
            SimulatedTransform::from_parts(Vec3::new(1.0, 2.0, 3.0), turn),
        ));

        physics_release_system(&mut world);

        let placed = *world.get::<Transform>(entity).unwrap();
        assert!(close(placed.translation, Vec3::new(1.0, 2.0, 3.0)));
        assert!(close(placed.scale, Vec3::new(2.0, 2.0, 2.0)));
        assert!(placed.rotation.dot(turn).abs() > 1.0 - 1e-5);
        assert!(world.get::<SimulatedTransform>(entity).is_none());
    }

    #[test]
    fn a_body_still_simulated_is_left_alone() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            RigidBody::new_dynamic(1.0),
            SimulatedTransform::from_parts(Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY),
        ));

        physics_release_system(&mut world);

        assert!(world.get::<SimulatedTransform>(entity).is_some());
        assert_eq!(
            world.get::<Transform>(entity).unwrap().translation,
            Vec3::ZERO
        );
    }

    #[test]
    fn a_release_moved_the_same_frame_keeps_the_authored_move() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)),
            GlobalTransform::new(Mat4::from_translation(Vec3::new(9.0, 0.0, 0.0))),
            SimulatedTransform::from_parts(Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY),
            Teleported,
        ));

        physics_release_system(&mut world);

        assert!(close(
            world.get::<Transform>(entity).unwrap().translation,
            Vec3::new(9.0, 0.0, 0.0)
        ));
        assert!(world.get::<SimulatedTransform>(entity).is_none());
    }
}
