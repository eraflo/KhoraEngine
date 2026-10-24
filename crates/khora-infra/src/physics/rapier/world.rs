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

//! The Rapier-backed `PhysicsProvider`.

use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    BodyType, CharacterControllerOptions, ColliderDesc, ColliderHandle, ColliderShape,
    CollisionEvent, PhysicsProvider, Ray, RaycastHit, RigidBodyDesc, RigidBodyHandle,
};
use rapier3d::control::*;
use rapier3d::prelude::*;
use std::sync::{Arc, Mutex};

use super::collider_aliases::{ColliderAliases, RapierCollider};
use super::conversions::*;
use super::debug::*;
use super::events::*;
use super::{
    from_rapier_cl_handle, from_rapier_rb_handle, stamp_owner, stamped_owner, to_rapier_rb_handle,
};

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
    /// Engine handles whose Rapier collider was replaced — see
    /// [`RapierPhysicsWorld::replace_collider`].
    aliases: ColliderAliases,
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
            aliases: ColliderAliases::default(),
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
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        self.aliases.settle(&mut events);
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
            // Detached, not destroyed: the collider belongs to an entity, which
            // may outlive its body and still hold the handle. A collider no
            // entity holds is removed on its own.
            false,
        );
    }

    fn add_collider(&mut self, desc: ColliderDesc) -> ColliderHandle {
        from_rapier_cl_handle(self.insert_collider(&desc))
    }

    fn remove_collider(&mut self, handle: ColliderHandle) {
        let cl_handle = self.aliases.rapier_of(handle);
        self.collider_set.remove(
            cl_handle,
            &mut self.island_manager,
            &mut self.rigid_body_set,
            true,
        );
        self.aliases.removed(handle, cl_handle);
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

    fn get_body_velocity(&self, handle: RigidBodyHandle) -> (Vec3, Vec3) {
        match self.rigid_body_set.get(to_rapier_rb_handle(handle)) {
            Some(rb) => (from_rapier_vec(rb.linvel()), from_rapier_vec(rb.angvel())),
            None => (Vec3::ZERO, Vec3::ZERO),
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
            .map(|(handle, _)| self.aliases.khora_of(handle))
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
            rb.enable_ccd(desc.ccd_enabled);
            // **Not the velocity.** This runs every frame from
            // `physics_provider_sync`, and `desc.linear_velocity` is the
            // authored component — an *initial condition*, set once at
            // `add_body`. Writing it here meant gravity accumulated for exactly
            // one step and was then thrown away: a body fell a fraction of a
            // millimetre per frame forever instead of falling.
            //
            // Setting a velocity on purpose is a deliberate act, like
            // teleporting is, and wants a method of its own rather than a
            // side-effect of the frame's sync.
        }
    }

    fn update_collider_properties(&mut self, handle: ColliderHandle, desc: ColliderDesc) {
        let cl_handle = self.aliases.rapier_of(handle);
        // The body it hangs from first: an entity given a `RigidBody` after its
        // `Collider`, or losing it, changes the frame the pose below is read in.
        let parent = desc.parent_body.map(to_rapier_rb_handle);
        if self
            .collider_set
            .get(cl_handle)
            .is_some_and(|cl| cl.parent() != parent)
        {
            self.collider_set
                .set_parent(cl_handle, parent, &mut self.rigid_body_set);
        }
        // Everything compared first, through a shared borrow: in Rapier taking
        // a collider mutably marks it modified whether or not a setter runs,
        // and a collider modified every frame keeps its body awake — not one
        // resting body in the scene could ever fall asleep.
        let Some(cl) = self.collider_set.get(cl_handle) else {
            return;
        };
        // An attached collider's pose is its offset from its body, and only
        // that is written. `set_position` on an attached collider overwrites
        // the world pose the solver derives from the body, so the contacts of
        // a step were found at one pose and solved at another — a stack of
        // boxes gained energy from nowhere and blew apart.
        let pose = Pose::from_parts(to_rapier_vec(desc.position), to_rapier_quat(desc.rotation));
        let attached = cl.position_wrt_parent().is_some();
        let pose_differs = match cl.position_wrt_parent() {
            Some(current) => !same_pose(current, &pose),
            None => !same_pose(cl.position(), &pose),
        };
        // The shape arrives at world size, so a rescaled entity, like an
        // edited shape, changes it. A new shape recomputes the body's mass and
        // re-runs its contacts.
        let shape_differs = !same_shape(cl.shape(), &desc.shape);
        if shape_differs && self.has_cached_contacts(cl_handle) {
            self.replace_collider(handle, cl_handle, &desc);
            return;
        }
        let sensor_differs = cl.is_sensor() != desc.is_sensor;
        let types = collision_types_of(&desc);
        let types_differ = cl.active_collision_types() != types;
        let events = events_of(&desc);
        let events_differ = cl.active_events() != events;
        let friction_differs = cl.friction() != desc.friction;
        let restitution_differs = cl.restitution() != desc.restitution;
        if !(pose_differs
            || shape_differs
            || sensor_differs
            || types_differ
            || events_differ
            || friction_differs
            || restitution_differs)
        {
            return;
        }

        let Some(cl) = self.collider_set.get_mut(cl_handle) else {
            return;
        };
        if pose_differs {
            if attached {
                cl.set_position_wrt_parent(pose);
            } else {
                cl.set_position(pose);
            }
        }
        if shape_differs {
            cl.set_shape(to_rapier_shape(&desc.shape));
        }
        if sensor_differs {
            cl.set_sensor(desc.is_sensor);
            cl.set_density(density_of(desc.is_sensor));
        }
        if types_differ {
            cl.set_active_collision_types(types);
        }
        if events_differ {
            cl.set_active_events(events);
        }
        if friction_differs {
            cl.set_friction(desc.friction);
        }
        if restitution_differs {
            cl.set_restitution(desc.restitution);
        }
        // Rapier flags no change for a new surface: a body asleep on this
        // collider would never be told, and a crate resting on a slope made
        // frictionless would stay put. Its body and everything touching it are
        // woken, so the next step's contacts read the new coefficients.
        if friction_differs || restitution_differs {
            self.wake_around(cl_handle);
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
            collider: self.aliases.khora_of(handle),
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
            .get(self.aliases.rapier_of(collider))
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
        let cl_handle = self.aliases.rapier_of(collider);
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
                // A sensor is a volume that notices, not a wall: a character
                // walks into a trigger rather than being stopped at its edge.
                QueryFilter::default()
                    .exclude_collider(cl_handle)
                    .exclude_sensors(),
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

impl RapierPhysicsWorld {
    /// Builds and inserts the Rapier collider a desc describes.
    fn insert_collider(&mut self, desc: &ColliderDesc) -> RapierCollider {
        let collider = ColliderBuilder::new(to_rapier_shape(&desc.shape))
            .translation(to_rapier_vec(desc.position))
            .rotation(to_rapier_quat(desc.rotation).to_scaled_axis())
            .active_events(events_of(desc))
            .friction(desc.friction)
            .restitution(desc.restitution)
            .sensor(desc.is_sensor)
            .density(density_of(desc.is_sensor))
            .active_collision_types(collision_types_of(desc))
            .user_data(stamp_owner(desc.owner))
            .build();

        match desc.parent_body {
            Some(parent) => self.collider_set.insert_with_parent(
                collider,
                to_rapier_rb_handle(parent),
                &mut self.rigid_body_set,
            ),
            None => self.collider_set.insert(collider),
        }
    }

    /// Whether the narrow phase holds contact points for this collider.
    fn has_cached_contacts(&self, collider: RapierCollider) -> bool {
        self.narrow_phase
            .contact_pairs_with(collider)
            .any(|pair| pair.manifolds.iter().any(|m| !m.points.is_empty()))
    }

    /// Gives an engine handle a new Rapier collider built from `desc`, so a
    /// resized collider's contacts are found from scratch — Rapier would keep
    /// the old shape's points (see [`super::collider_aliases`]). Only a
    /// collider holding contact points is replaced; any other takes the new
    /// shape in place.
    fn replace_collider(
        &mut self,
        handle: ColliderHandle,
        current: RapierCollider,
        desc: &ColliderDesc,
    ) {
        let mut touching: Vec<RapierCollider> = self
            .narrow_phase
            .contact_pairs_with(current)
            .filter(|pair| pair.has_any_active_contact())
            .map(|pair| other_of(pair.collider1, pair.collider2, current))
            .collect();
        touching.extend(
            self.narrow_phase
                .intersection_pairs_with(current)
                .filter(|(_, _, intersecting)| *intersecting)
                .map(|(a, b, _)| other_of(a, b, current)),
        );

        self.collider_set.remove(
            current,
            &mut self.island_manager,
            &mut self.rigid_body_set,
            true,
        );
        let new = self.insert_collider(desc);
        // What it touched may be asleep on it.
        for partner in &touching {
            if let Some(body) = self.collider_set.get(*partner).and_then(|cl| cl.parent()) {
                if let Some(rb) = self.rigid_body_set.get_mut(body) {
                    rb.wake_up(true);
                }
            }
        }

        self.aliases.replaced(handle, current, new, touching);
    }

    /// Wakes the body a collider hangs from and every body in contact with it.
    fn wake_around(&mut self, collider: rapier3d::geometry::ColliderHandle) {
        let parent_of = |handle| self.collider_set.get(handle).and_then(|cl| cl.parent());
        let mut bodies: Vec<_> = parent_of(collider).into_iter().collect();
        for pair in self.narrow_phase.contact_pairs_with(collider) {
            let other = if pair.collider1 == collider {
                pair.collider2
            } else {
                pair.collider1
            };
            bodies.extend(parent_of(other));
        }
        for body in bodies {
            if let Some(rb) = self.rigid_body_set.get_mut(body) {
                rb.wake_up(true);
            }
        }
    }
}

/// The collider of a pair that is not `this` one.
fn other_of(a: RapierCollider, b: RapierCollider, this: RapierCollider) -> RapierCollider {
    if a == this {
        b
    } else {
        a
    }
}

/// The Rapier shape of a world-size [`ColliderShape`].
fn to_rapier_shape(shape: &ColliderShape) -> SharedShape {
    match *shape {
        ColliderShape::Box(half) => SharedShape::cuboid(half.x, half.y, half.z),
        ColliderShape::Sphere(r) => SharedShape::ball(r),
        ColliderShape::Capsule(h, r) => SharedShape::capsule_y(h, r),
    }
}

/// Whether `current` is already the shape `wanted` describes, to within what a
/// frame's sync can produce from an unchanged authored shape and scale.
///
/// Relative, because the size is the authored one times the column lengths of
/// a world matrix that carries the solver's rotation: rounding there grows
/// with the shape, and an absolute bound would replace a large shape — mass,
/// contacts and all — on every frame it turns.
fn same_shape(current: &dyn Shape, wanted: &ColliderShape) -> bool {
    const REL: f32 = 1e-5;
    let close = |a: f32, b: f32| (a - b).abs() <= REL * a.abs().max(b.abs()).max(1.0);
    match *wanted {
        ColliderShape::Box(half) => current.as_cuboid().is_some_and(|c| {
            close(c.half_extents.x, half.x)
                && close(c.half_extents.y, half.y)
                && close(c.half_extents.z, half.z)
        }),
        ColliderShape::Sphere(r) => current.as_ball().is_some_and(|b| close(b.radius, r)),
        ColliderShape::Capsule(h, r) => current.as_capsule().is_some_and(|c| {
            close(c.radius, r)
                && close(c.segment.a.x, 0.0)
                && close(c.segment.a.y, -h)
                && close(c.segment.a.z, 0.0)
                && close(c.segment.b.x, 0.0)
                && close(c.segment.b.y, h)
                && close(c.segment.b.z, 0.0)
        }),
    }
}

/// The events a collider raises. A sensor exists to say what enters it, so it
/// always reports: one that stayed silent until somebody also asked for
/// events would be a trigger that never triggers.
fn events_of(desc: &ColliderDesc) -> ActiveEvents {
    if desc.active_events || desc.is_sensor {
        ActiveEvents::COLLISION_EVENTS
    } else {
        ActiveEvents::empty()
    }
}

/// A sensor weighs nothing: it is a volume that notices, not matter, so it
/// must not shift its body's mass or centre of mass. A solid collider keeps
/// Rapier's default density.
fn density_of(is_sensor: bool) -> f32 {
    if is_sensor {
        0.0
    } else {
        1.0
    }
}

/// Which pairs a collider is tested against — a pair is kept when either side
/// asks for it.
///
/// Rapier's default skips every pair with no dynamic body. A sensor also
/// wants kinematic bodies (a moving platform entering a trigger). A
/// character's collider has no body and is moved by hand, so Rapier counts it
/// as fixed, like the level: it asks for fixed pairs itself, which lets a
/// trigger — fixed too — see it. A sensor does **not** ask for fixed pairs: a
/// trigger placed against the floor, or overlapping another trigger, would
/// report the level at spawn though nothing entered it.
fn collision_types_of(desc: &ColliderDesc) -> ActiveCollisionTypes {
    let mut types = ActiveCollisionTypes::default();
    if desc.is_sensor {
        types |= ActiveCollisionTypes::KINEMATIC_FIXED | ActiveCollisionTypes::KINEMATIC_KINEMATIC;
    }
    if desc.is_character {
        types |= ActiveCollisionTypes::FIXED_FIXED | ActiveCollisionTypes::KINEMATIC_FIXED;
    }
    types
}

/// Whether two poses are the same to within what a frame's sync can produce
/// from an unchanged authored pose.
fn same_pose(a: &Pose, b: &Pose) -> bool {
    const EPS: f32 = 1e-6;
    // Rotations compared component-wise (either sign: `q` and `-q` are one
    // rotation). A dot-product test near 1 would hide every turn under ~0.16°,
    // since `1 - cos(θ/2)` is second-order in `θ`.
    (a.translation - b.translation).abs().max_element() <= EPS
        && (a.rotation.abs_diff_eq(b.rotation, EPS) || a.rotation.abs_diff_eq(-b.rotation, EPS))
}
