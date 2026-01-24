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

//! Rapier implementation of the physics provider.

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, ColliderDesc, ColliderHandle, ColliderShape, PhysicsProvider, RigidBodyDesc,
    RigidBodyHandle,
};
use rapier3d::prelude::*;

/// Implementation of the `PhysicsProvider` trait using the Rapier3D physics engine.
pub struct RapierPhysicsWorld {
    rigid_body_set: RigidBodySet,
    collider_set: ColliderSet,
    gravity: Vector<Real>,
    integration_parameters: IntegrationParameters,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: BroadPhaseMultiSap,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
}

impl Default for RapierPhysicsWorld {
    fn default() -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: vector![0.0, -9.81, 0.0],
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: BroadPhaseMultiSap::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
        }
    }
}

impl PhysicsProvider for RapierPhysicsWorld {
    fn step(&mut self, dt: f32) {
        self.integration_parameters.dt = dt;
        self.physics_pipeline.step(
            &self.gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            None,
            &(),
            &(),
        );
    }

    fn set_gravity(&mut self, gravity: Vec3) {
        self.gravity = vector![gravity.x, gravity.y, gravity.z];
    }

    fn add_body(&mut self, desc: RigidBodyDesc) -> RigidBodyHandle {
        let rb_type = match desc.body_type {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicVelocityBased,
        };

        let rigid_body = RigidBodyBuilder::new(rb_type)
            .translation(vector![desc.position.x, desc.position.y, desc.position.z])
            .rotation(
                rapier3d::na::UnitQuaternion::from_quaternion(rapier3d::na::Quaternion::new(
                    desc.rotation.w,
                    desc.rotation.x,
                    desc.rotation.y,
                    desc.rotation.z,
                ))
                .scaled_axis(),
            )
            .linvel(vector![
                desc.linear_velocity.x,
                desc.linear_velocity.y,
                desc.linear_velocity.z
            ])
            .angvel(vector![
                desc.angular_velocity.x,
                desc.angular_velocity.y,
                desc.angular_velocity.z
            ])
            .additional_mass(desc.mass)
            .build();

        let handle = self.rigid_body_set.insert(rigid_body);
        RigidBodyHandle(handle.into_raw_parts().0 as u64)
    }

    fn remove_body(&mut self, handle: RigidBodyHandle) {
        let rb_handle = rapier3d::dynamics::RigidBodyHandle::from_raw_parts(handle.0 as u32, 0); // Generation 0 placeholder
        self.rigid_body_set.remove(
            rb_handle,
            &mut self.island_manager,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            true,
        );
    }

    fn add_collider(&mut self, desc: ColliderDesc) -> ColliderHandle {
        let shape = match desc.shape {
            ColliderShape::Box(half) => SharedShape::cuboid(half.x, half.y, half.z),
            ColliderShape::Sphere(r) => SharedShape::ball(r),
            ColliderShape::Capsule(h, r) => SharedShape::capsule_y(h, r),
        };

        let collider = ColliderBuilder::new(shape)
            .translation(vector![desc.position.x, desc.position.y, desc.position.z])
            .rotation(
                rapier3d::na::UnitQuaternion::from_quaternion(rapier3d::na::Quaternion::new(
                    desc.rotation.w,
                    desc.rotation.x,
                    desc.rotation.y,
                    desc.rotation.z,
                ))
                .scaled_axis(),
            )
            .build();

        let handle = if let Some(parent_handle) = desc.parent_body {
            let rb_handle =
                rapier3d::dynamics::RigidBodyHandle::from_raw_parts(parent_handle.0 as u32, 0);
            self.collider_set
                .insert_with_parent(collider, rb_handle, &mut self.rigid_body_set)
        } else {
            self.collider_set.insert(collider)
        };

        ColliderHandle(handle.into_raw_parts().0 as u64)
    }

    fn remove_collider(&mut self, handle: ColliderHandle) {
        let cl_handle = rapier3d::geometry::ColliderHandle::from_raw_parts(handle.0 as u32, 0);
        self.collider_set.remove(
            cl_handle,
            &mut self.island_manager,
            &mut self.rigid_body_set,
            true,
        );
    }

    fn get_body_transform(&self, handle: RigidBodyHandle) -> (Vec3, Quat) {
        let rb_handle = rapier3d::dynamics::RigidBodyHandle::from_raw_parts(handle.0 as u32, 0);
        if let Some(rb) = self.rigid_body_set.get(rb_handle) {
            let t = rb.translation();
            let r = rb.rotation();
            (Vec3::new(t.x, t.y, t.z), Quat::new(r.i, r.j, r.k, r.w))
        } else {
            (Vec3::ZERO, Quat::IDENTITY)
        }
    }

    fn set_body_transform(&mut self, handle: RigidBodyHandle, pos: Vec3, rot: Quat) {
        let rb_handle = rapier3d::dynamics::RigidBodyHandle::from_raw_parts(handle.0 as u32, 0);
        if let Some(rb) = self.rigid_body_set.get_mut(rb_handle) {
            rb.set_translation(vector![pos.x, pos.y, pos.z], true);
            rb.set_rotation(
                rapier3d::na::UnitQuaternion::from_quaternion(rapier3d::na::Quaternion::new(
                    rot.w, rot.x, rot.y, rot.z,
                )),
                true,
            );
        }
    }
}
