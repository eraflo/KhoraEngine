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

//! Descriptions of the bodies and colliders a backend creates.

use serde::{Deserialize, Serialize};

use super::handle::RigidBodyHandle;
use crate::math::{Quat, Vec3};

/// Defines the type of a rigid body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The entity this collider belongs to.
    ///
    /// Stamped into the backend so a contact can name **who** touched whom.
    /// Without it a collision arrives as two backend handles and the only way
    /// back to entities is to scan every collider component comparing handles —
    /// linear per contact, and ambiguous the moment a slot is recycled.
    ///
    /// `None` for a collider the ECS does not own, which nothing creates today
    /// and a tool or a query volume might.
    pub owner: Option<crate::ecs::entity::EntityId>,
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
