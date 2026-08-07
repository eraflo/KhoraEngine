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
    CollisionEvent, PhysicsProvider, Ray, RaycastHit, RigidBodyDesc, RigidBodyHandle, Slot,
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
        from_rapier_rb_handle(handle)
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
            .user_data(stamp_owner(desc.owner))
            .build();

        let handle = if let Some(parent_handle) = desc.parent_body {
            let rb_handle = to_rapier_rb_handle(parent_handle);
            self.collider_set
                .insert_with_parent(collider, rb_handle, &mut self.rigid_body_set)
        } else {
            self.collider_set.insert(collider)
        };

        from_rapier_cl_handle(handle)
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
            .map(|(handle, _)| from_rapier_rb_handle(handle))
            .collect()
    }

    fn get_all_colliders(&self) -> Vec<ColliderHandle> {
        self.collider_set
            .iter()
            .map(|(handle, _)| from_rapier_cl_handle(handle))
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
            collider: from_rapier_cl_handle(handle),
            distance: intersection.time_of_impact,
            normal: from_rapier_vec(intersection.normal),
            position: from_rapier_vec(hit_pos),
        })
    }

    fn entity_of(&self, collider: ColliderHandle) -> Option<khora_core::ecs::entity::EntityId> {
        // Asked of the collider itself, not of an index kept beside the world.
        // A handle whose slot has been recycled does not resolve at all, which
        // is the answer — the collider it named is gone, and so is its entity.
        self.collider_set
            .get(to_rapier_cl_handle(collider))
            .and_then(|collider| stamped_owner(collider.user_data))
    }

    fn take_collision_events(&self) -> Vec<CollisionEvent> {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
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

/// Both directions carry Rapier's generation, which they used to drop.
///
/// `from_raw_parts(index, 0)` resolved a handle to whatever now sits in the
/// slot: a stale handle moved somebody else's body instead of failing, and only
/// once Rapier had recycled that slot — so never in a short session and
/// reliably in a long one.
fn to_rapier_rb_handle(handle: RigidBodyHandle) -> rapier3d::dynamics::RigidBodyHandle {
    let slot = handle.slot();
    rapier3d::dynamics::RigidBodyHandle::from_raw_parts(slot.index, slot.generation)
}

pub(super) fn from_rapier_rb_handle(
    handle: rapier3d::dynamics::RigidBodyHandle,
) -> RigidBodyHandle {
    let (index, generation) = handle.into_raw_parts();
    RigidBodyHandle(Slot { index, generation }.pack())
}

fn to_rapier_cl_handle(handle: ColliderHandle) -> rapier3d::geometry::ColliderHandle {
    let slot = handle.slot();
    rapier3d::geometry::ColliderHandle::from_raw_parts(slot.index, slot.generation)
}

pub(super) fn from_rapier_cl_handle(handle: rapier3d::geometry::ColliderHandle) -> ColliderHandle {
    let (index, generation) = handle.into_raw_parts();
    ColliderHandle(Slot { index, generation }.pack())
}

/// Set when the low 64 bits hold an entity.
///
/// A flag rather than treating zero as absent: `EntityId { index: 0,
/// generation: 0 }` is the first entity a fresh `World` hands out, and it packs
/// to zero. Reading that back as "no owner" made the first entity in a scene
/// the one a collision could never name — silently, and only that one.
const OWNED: u128 = 1 << 64;

/// Packs an entity into the `u128` Rapier carries on every collider.
///
/// The identity travels **with the collider**, so there is no index to keep in
/// step with the world and nothing to invalidate when an entity dies: the
/// collider dies with it.
fn stamp_owner(entity: Option<khora_core::ecs::entity::EntityId>) -> u128 {
    match entity {
        Some(entity) => OWNED | ((entity.generation as u128) << 32) | entity.index as u128,
        None => 0,
    }
}

/// Reads back what [`stamp_owner`] wrote.
pub(super) fn stamped_owner(user_data: u128) -> Option<khora_core::ecs::entity::EntityId> {
    if user_data & OWNED == 0 {
        return None;
    }
    Some(khora_core::ecs::entity::EntityId {
        index: user_data as u32,
        generation: (user_data >> 32) as u32,
    })
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;
    use khora_core::physics::ColliderShape;

    fn entity(index: u32, generation: u32) -> EntityId {
        EntityId { index, generation }
    }

    fn a_box(owner: Option<EntityId>) -> ColliderDesc {
        ColliderDesc {
            owner,
            parent_body: None,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            shape: ColliderShape::Sphere(1.0),
            active_events: true,
            friction: 0.5,
            restitution: 0.0,
        }
    }

    /// **What unblocks a collision anybody can act on.** A contact names two
    /// colliders; this is what turns one back into the entity gameplay knows.
    #[test]
    fn a_collider_remembers_the_entity_that_made_it() {
        let mut world = RapierPhysicsWorld::default();
        let owner = entity(12, 3);

        let handle = world.add_collider(a_box(Some(owner)));

        assert_eq!(world.entity_of(handle), Some(owner));
    }

    /// The generation is carried, so two entities that reused one index are
    /// told apart — the same recycling problem as the slot, one level up.
    #[test]
    fn a_recycled_entity_index_is_not_mistaken_for_the_old_one() {
        let mut world = RapierPhysicsWorld::default();
        let old = world.add_collider(a_box(Some(entity(4, 0))));
        let new = world.add_collider(a_box(Some(entity(4, 1))));

        assert_eq!(world.entity_of(old), Some(entity(4, 0)));
        assert_eq!(world.entity_of(new), Some(entity(4, 1)));
    }

    /// **The first entity a `World` hands out is `{ index: 0, generation: 0 }`,
    /// which packs to zero.** Treating zero as "no owner" made that one entity
    /// the one a collision could never name — and only that one, so a scene
    /// whose floor happened to be spawned first lost every contact with it.
    #[test]
    fn the_zeroth_entity_is_still_an_entity() {
        let mut world = RapierPhysicsWorld::default();
        let first = entity(0, 0);

        let handle = world.add_collider(a_box(Some(first)));

        assert_eq!(world.entity_of(handle), Some(first));
    }

    /// A collider the ECS did not make — a query volume, a tool — answers
    /// nothing rather than answering entity zero.
    #[test]
    fn a_collider_with_no_owner_names_nobody() {
        let mut world = RapierPhysicsWorld::default();

        let handle = world.add_collider(a_box(None));

        assert_eq!(world.entity_of(handle), None);
    }

    /// A handle whose collider is gone resolves to nothing, which is the
    /// answer: what it named does not exist, so neither does its entity.
    #[test]
    fn a_removed_colliders_handle_names_nobody() {
        let mut world = RapierPhysicsWorld::default();
        let handle = world.add_collider(a_box(Some(entity(1, 0))));

        world.remove_collider(handle);

        assert_eq!(world.entity_of(handle), None);
    }

    /// Isolates the provider: an overlapping pair that asked for events should
    /// produce one.
    ///
    /// The ball needs a dynamic parent body. Rapier generates no contact
    /// between two colliders that both count as fixed, and a collider with no
    /// parent counts as fixed — so a test written without one measures that
    /// rule rather than the event wiring.
    #[test]
    fn two_overlapping_colliders_report_a_contact() {
        use khora_core::physics::{BodyType, RigidBodyDesc};

        let mut world = RapierPhysicsWorld::default();

        let mut floor = a_box(Some(entity(0, 0)));
        floor.shape = ColliderShape::Box(Vec3::new(10.0, 0.5, 10.0));
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
        let mut ball = a_box(Some(entity(1, 0)));
        ball.parent_body = Some(body);
        world.add_collider(ball);

        world.step(1.0 / 60.0);

        assert!(
            !world.take_collision_events().is_empty(),
            "the overlap should have been reported"
        );
    }
}
