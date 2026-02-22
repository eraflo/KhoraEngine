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

mod conversions;
mod debug;
mod events;

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, CharacterControllerOptions, ColliderDesc, ColliderHandle, ColliderShape,
    CollisionEvent, PhysicsProvider, Ray, RaycastHit, RigidBodyDesc, RigidBodyHandle,
};
use rapier3d::control::*;
use rapier3d::prelude::*;
use std::sync::{Arc, Mutex};

use conversions::*;
use debug::*;
use events::*;

/// Implementation of the `PhysicsProvider` trait using the Rapier3D physics engine.
pub struct RapierPhysicsWorld {
    rigid_body_set: RigidBodySet,
    collider_set: ColliderSet,
    gravity: Vector,
    integration_parameters: IntegrationParameters,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    events: Arc<Mutex<Vec<CollisionEvent>>>,
}

impl Default for RapierPhysicsWorld {
    fn default() -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: Vector::new(0.0, -9.81, 0.0),
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl PhysicsProvider for RapierPhysicsWorld {
    fn step(&mut self, dt: f32) {
        self.integration_parameters.dt = dt;
        let event_handler = RapierEventHandler {
            events: self.events.clone(),
        };

        self.physics_pipeline.step(
            self.gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &(),
            &event_handler,
        );
    }

    fn set_gravity(&mut self, gravity: Vec3) {
        self.gravity = to_rapier_vec(gravity);
    }

    fn add_body(&mut self, desc: RigidBodyDesc) -> RigidBodyHandle {
        let rb_type = match desc.body_type {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicVelocityBased,
        };

        let rigid_body = RigidBodyBuilder::new(rb_type)
            .translation(to_rapier_vec(desc.position))
            .rotation(to_rapier_quat(desc.rotation).to_scaled_axis())
            .linvel(to_rapier_vec(desc.linear_velocity))
            .angvel(to_rapier_vec(desc.angular_velocity))
            .additional_mass(desc.mass)
            .ccd_enabled(desc.ccd_enabled)
            .build();

        let handle = self.rigid_body_set.insert(rigid_body);
        RigidBodyHandle(handle.into_raw_parts().0 as u64)
    }

    fn remove_body(&mut self, handle: RigidBodyHandle) {
        let rb_handle = to_rapier_rb_handle(handle);
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
            .translation(to_rapier_vec(desc.position))
            .rotation(to_rapier_quat(desc.rotation).to_scaled_axis())
            .active_events(if desc.active_events {
                ActiveEvents::COLLISION_EVENTS
            } else {
                ActiveEvents::empty()
            })
            .friction(desc.friction)
            .restitution(desc.restitution)
            .build();

        let handle = if let Some(parent_handle) = desc.parent_body {
            let rb_handle = to_rapier_rb_handle(parent_handle);
            self.collider_set
                .insert_with_parent(collider, rb_handle, &mut self.rigid_body_set)
        } else {
            self.collider_set.insert(collider)
        };

        ColliderHandle(handle.into_raw_parts().0 as u64)
    }

    fn remove_collider(&mut self, handle: ColliderHandle) {
        let cl_handle = to_rapier_cl_handle(handle);
        self.collider_set.remove(
            cl_handle,
            &mut self.island_manager,
            &mut self.rigid_body_set,
            true,
        );
    }

    fn get_body_transform(&self, handle: RigidBodyHandle) -> (Vec3, Quat) {
        let rb_handle = to_rapier_rb_handle(handle);
        if let Some(rb) = self.rigid_body_set.get(rb_handle) {
            let t = rb.translation();
            let r = rb.rotation();
            (from_rapier_vec(t), from_rapier_quat(*r))
        } else {
            (Vec3::ZERO, Quat::IDENTITY)
        }
    }

    fn set_body_transform(&mut self, handle: RigidBodyHandle, pos: Vec3, rot: Quat) {
        let rb_handle = to_rapier_rb_handle(handle);
        if let Some(rb) = self.rigid_body_set.get_mut(rb_handle) {
            rb.set_translation(to_rapier_vec(pos), true);
            rb.set_rotation(to_rapier_quat(rot), true);
        }
    }

    fn get_all_bodies(&self) -> Vec<RigidBodyHandle> {
        self.rigid_body_set
            .iter()
            .map(|(handle, _)| RigidBodyHandle(handle.into_raw_parts().0 as u64))
            .collect()
    }

    fn get_all_colliders(&self) -> Vec<ColliderHandle> {
        self.collider_set
            .iter()
            .map(|(handle, _)| ColliderHandle(handle.into_raw_parts().0 as u64))
            .collect()
    }

    fn update_body_properties(&mut self, handle: RigidBodyHandle, desc: RigidBodyDesc) {
        let rb_handle = to_rapier_rb_handle(handle);
        if let Some(rb) = self.rigid_body_set.get_mut(rb_handle) {
            let rb_type = match desc.body_type {
                BodyType::Dynamic => RigidBodyType::Dynamic,
                BodyType::Static => RigidBodyType::Fixed,
                BodyType::Kinematic => RigidBodyType::KinematicVelocityBased,
            };
            rb.set_body_type(rb_type, true);
            rb.set_additional_mass(desc.mass, true);
            rb.set_linvel(to_rapier_vec(desc.linear_velocity), true);
            rb.set_angvel(to_rapier_vec(desc.angular_velocity), true);
            rb.enable_ccd(desc.ccd_enabled);
        }
    }

    fn update_collider_properties(&mut self, handle: ColliderHandle, desc: ColliderDesc) {
        let cl_handle = to_rapier_cl_handle(handle);
        if let Some(cl) = self.collider_set.get_mut(cl_handle) {
            cl.set_translation(to_rapier_vec(desc.position));
            cl.set_rotation(to_rapier_quat(desc.rotation));
            cl.set_active_events(if desc.active_events {
                ActiveEvents::COLLISION_EVENTS
            } else {
                ActiveEvents::empty()
            });
            cl.set_friction(desc.friction);
            cl.set_restitution(desc.restitution);
        }
    }

    fn get_debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>) {
        let mut backend = RapierDebugBackend::default();
        let mut pipeline = DebugRenderPipeline::default();
        pipeline.render(
            &mut backend,
            &self.rigid_body_set,
            &self.collider_set,
            &self.impulse_joint_set,
            &self.multibody_joint_set,
            &self.narrow_phase,
        );
        (backend.vertices, backend.indices)
    }

    fn cast_ray(&self, ray: &Ray, max_toi: f32, solid: bool) -> Option<RaycastHit> {
        let rapier_ray =
            rapier3d::geometry::Ray::new(to_rapier_vec(ray.origin), to_rapier_vec(ray.direction));

        let query_pipeline = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            QueryFilter::default(),
        );

        let (handle, intersection) =
            query_pipeline.cast_ray_and_get_normal(&rapier_ray, max_toi, solid)?;

        let hit_pos = rapier_ray.point_at(intersection.time_of_impact);

        Some(RaycastHit {
            collider: ColliderHandle(handle.into_raw_parts().0 as u64),
            distance: intersection.time_of_impact,
            normal: from_rapier_vec(intersection.normal),
            position: from_rapier_vec(hit_pos),
        })
    }

    fn get_collision_events(&self) -> Vec<CollisionEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    fn move_character(
        &self,
        collider: ColliderHandle,
        desired_translation: Vec3,
        options: &CharacterControllerOptions,
    ) -> (Vec3, bool) {
        let cl_handle = to_rapier_cl_handle(collider);
        if let Some(cl) = self.collider_set.get(cl_handle) {
            let kcc = KinematicCharacterController {
                offset: CharacterLength::Absolute(options.offset),
                max_slope_climb_angle: options.max_slope_climb_angle,
                min_slope_slide_angle: options.min_slope_slide_angle,
                autostep: if options.autostep_enabled {
                    Some(CharacterAutostep {
                        max_height: CharacterLength::Absolute(options.autostep_height),
                        min_width: CharacterLength::Absolute(options.autostep_min_width),
                        include_dynamic_bodies: true,
                    })
                } else {
                    None
                },
                ..Default::default()
            };

            let query_pipeline = self.broad_phase.as_query_pipeline(
                self.narrow_phase.query_dispatcher(),
                &self.rigid_body_set,
                &self.collider_set,
                QueryFilter::default().exclude_collider(cl_handle),
            );

            let result = kcc.move_shape(
                self.integration_parameters.dt,
                &query_pipeline,
                cl.shape(),
                cl.position(),
                to_rapier_vec(desired_translation),
                |_| {},
            );

            (from_rapier_vec(result.translation), result.grounded)
        } else {
            (Vec3::ZERO, false)
        }
    }
}

// --- Internal Helpers ---

fn to_rapier_rb_handle(handle: RigidBodyHandle) -> rapier3d::dynamics::RigidBodyHandle {
    rapier3d::dynamics::RigidBodyHandle::from_raw_parts(handle.0 as u32, 0)
}

fn to_rapier_cl_handle(handle: ColliderHandle) -> rapier3d::geometry::ColliderHandle {
    rapier3d::geometry::ColliderHandle::from_raw_parts(handle.0 as u32, 0)
}
