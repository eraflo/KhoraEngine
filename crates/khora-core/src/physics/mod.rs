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

//! # Physics Abstractions
//!
//! Universal traits and types for physics simulation providers.

pub mod collision;
pub mod dynamic_tree;
pub mod solver;

pub use collision::*;
pub use dynamic_tree::*;
pub use solver::*;

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::math::{LinearRgba, Quat, Vec3};

/// Opaque handle to a rigid body in the physics engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub struct RigidBodyHandle(pub u64);

/// Opaque handle to a collider in the physics engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub struct ColliderHandle(pub u64);

/// Defines the type of a rigid body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum BodyType {
    /// Responds to forces and collisions.
    Dynamic,
    /// Fixed in place, does not move.
    Static,
    /// Controlled by the user, not by forces.
    Kinematic,
}

/// Description for creating a rigid body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigidBodyDesc {
    /// Initial position.
    pub position: Vec3,
    /// Initial rotation.
    pub rotation: Quat,
    /// Body type.
    pub body_type: BodyType,
    /// Linear velocity.
    pub linear_velocity: Vec3,
    /// Angular velocity.
    pub angular_velocity: Vec3,
    /// Mass of the body in kilograms.
    pub mass: f32,
    /// Whether to enable Continuous Collision Detection (CCD).
    pub ccd_enabled: bool,
}

/// Description for creating a collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderDesc {
    /// Parent rigid body to attach to (if any).
    pub parent_body: Option<RigidBodyHandle>,
    /// Relative or absolute position.
    pub position: Vec3,
    /// Relative or absolute rotation.
    pub rotation: Quat,
    /// Shape definition.
    pub shape: ColliderShape,
    /// Whether to enable collision events for this collider.
    pub active_events: bool,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
}

/// Supported collider shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColliderShape {
    /// Box with half-extents.
    Box(Vec3),
    /// Sphere with radius.
    Sphere(f32),
    /// Capsule with half-height and radius.
    Capsule(f32, f32),
}

impl ColliderShape {
    /// Computes the axis-aligned bounding box (AABB) for this shape in local space.
    pub fn compute_aabb(&self) -> crate::math::Aabb {
        match self {
            ColliderShape::Box(half_extents) => crate::math::Aabb::from_half_extents(*half_extents),
            ColliderShape::Sphere(radius) => {
                crate::math::Aabb::from_half_extents(Vec3::new(*radius, *radius, *radius))
            }
            ColliderShape::Capsule(half_height, radius) => {
                let r = Vec3::new(*radius, *radius, *radius);
                let h = Vec3::new(0.0, *half_height, 0.0);
                crate::math::Aabb::from_half_extents(r + h)
            }
        }
    }
}

/// Interface contract for any physics engine implementation (e.g., Rapier).
pub trait PhysicsProvider: Send + Sync {
    /// Advances the simulation by `dt` seconds.
    fn step(&mut self, dt: f32);

    /// Sets the global gravity vector.
    fn set_gravity(&mut self, gravity: Vec3);

    /// Adds a rigid body to the simulation.
    fn add_body(&mut self, desc: RigidBodyDesc) -> RigidBodyHandle;

    /// Removes a rigid body from the simulation.
    fn remove_body(&mut self, handle: RigidBodyHandle);

    /// Adds a collider to the simulation.
    fn add_collider(&mut self, desc: ColliderDesc) -> ColliderHandle;

    /// Removes a collider from the simulation.
    fn remove_collider(&mut self, handle: ColliderHandle);

    /// Synchronizes the position and rotation of a rigid body.
    fn get_body_transform(&self, handle: RigidBodyHandle) -> (Vec3, Quat);

    /// Manually sets the position and rotation of a rigid body.
    fn set_body_transform(&mut self, handle: RigidBodyHandle, pos: Vec3, rot: Quat);

    /// Returns a list of all active rigid body handles.
    fn get_all_bodies(&self) -> Vec<RigidBodyHandle>;

    /// Returns a list of all active collider handles.
    fn get_all_colliders(&self) -> Vec<ColliderHandle>;

    /// Updates the properties of an existing rigid body.
    fn update_body_properties(&mut self, handle: RigidBodyHandle, desc: RigidBodyDesc);

    /// Updates the properties of an existing collider.
    fn update_collider_properties(&mut self, handle: ColliderHandle, desc: ColliderDesc);

    /// Returns debug rendering lines from the physics engine.
    fn get_debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>);

    /// Casts a ray into the physics world and returns the closest hit.
    fn cast_ray(&self, ray: &Ray, max_toi: f32, solid: bool) -> Option<RaycastHit>;

    /// Returns the collision events that occurred during the last step.
    fn get_collision_events(&self) -> Vec<CollisionEvent>;

    /// Resolves movement for a kinematic character controller.
    /// Returns the actual translation applied and whether the character is grounded.
    fn move_character(
        &self,
        collider: ColliderHandle,
        desired_translation: Vec3,
        options: &CharacterControllerOptions,
    ) -> (Vec3, bool);
}

/// Options for resolving kinematic character movement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct CharacterControllerOptions {
    /// Max height of obstacles the character can step over.
    pub autostep_height: f32,
    /// Min width of obstacles for autostepping.
    pub autostep_min_width: f32,
    /// Whether autostepping is enabled.
    pub autostep_enabled: bool,
    /// Max angle for climbing slopes.
    pub max_slope_climb_angle: f32,
    /// Min angle for sliding down slopes.
    pub min_slope_slide_angle: f32,
    /// Distance to maintain from obstacles.
    pub offset: f32,
}

/// Events representing collision start/end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub enum CollisionEvent {
    /// Collision between two colliders started.
    Started(ColliderHandle, ColliderHandle),
    /// Collision between two colliders stopped.
    Stopped(ColliderHandle, ColliderHandle),
}

/// A ray in 3D space.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct Ray {
    /// Origin point.
    pub origin: Vec3,
    /// Direction vector (should be normalized).
    pub direction: Vec3,
}

/// Information about a raycast hit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct RaycastHit {
    /// The collider that was hit.
    pub collider: ColliderHandle,
    /// Distance from ray origin to hit point.
    pub distance: f32,
    /// Normal vector at the hit point.
    pub normal: Vec3,
    /// Exact position of the hit.
    pub position: Vec3,
}

/// Detailed information about a contact between two colliders.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct ContactManifold {
    /// Normal vector pointing from entity A to entity B.
    pub normal: Vec3,
    /// Intersection depth.
    pub depth: f32,
    /// Contact point in world space.
    pub point: Vec3,
}

impl ContactManifold {
    /// Returns the inverted manifold (flipped normal).
    pub fn inverted(&self) -> Self {
        Self {
            normal: -self.normal,
            depth: self.depth,
            point: self.point,
        }
    }
}

/// A simple line for debug rendering.
#[derive(Debug, Clone, Copy)]
pub struct DebugLine {
    /// Start point.
    pub start: Vec3,
    /// End point.
    pub end: Vec3,
    /// Color.
    pub color: LinearRgba,
}
