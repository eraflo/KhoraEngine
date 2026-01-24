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

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::math::{Quat, Vec3};

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
    /// Mass of the body in kg (dynamic only).
    pub mass: f32,
}

/// Description for creating a collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderDesc {
    /// Associated rigid body if any.
    pub parent_body: Option<RigidBodyHandle>,
    /// Relative position to parent/world.
    pub position: Vec3,
    /// Relative rotation to parent/world.
    pub rotation: Quat,
    /// Shape description (placeholder for now, will expand with enum).
    pub shape: ColliderShape,
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
}
