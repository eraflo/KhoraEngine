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

//! Description of a collider a backend creates, and its shape.

use serde::{Deserialize, Serialize};

use super::handle::RigidBodyHandle;
use crate::math::{Quat, Vec3};

/// Description for creating a collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderDesc {
    /// The entity this collider belongs to.
    ///
    /// Stamped into the backend so a contact names who touched whom, without
    /// scanning every collider for its handle. `None` for a collider the ECS
    /// does not own, such as a tool's or a query volume's.
    pub owner: Option<crate::ecs::entity::EntityId>,
    /// Parent rigid body to attach to (if any).
    pub parent_body: Option<RigidBodyHandle>,
    /// Position in the parent body's frame when `parent_body` is `Some`, in the world otherwise.
    ///
    /// Never the world position of an attached collider: a backend reads it as
    /// the offset from the body.
    pub position: Vec3,
    /// Rotation in the parent body's frame when `parent_body` is `Some`, in the world otherwise.
    pub rotation: Quat,
    /// The shape at world size: the authored shape scaled by the entity's world scale.
    ///
    /// A backend never sees the entity's scale.
    pub shape: ColliderShape,
    /// Whether to enable collision events for this collider.
    pub active_events: bool,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
    /// Whether the collider is a sensor: it reports what enters and leaves, nothing more.
    ///
    /// It reports whatever `active_events` says, but never pushes anything,
    /// never stops a character, and adds no mass to its body.
    pub is_sensor: bool,
    /// Whether the collider is a character's, moved by the character controller, not the solver.
    ///
    /// It has no body. A backend must let triggers see it as a moving body,
    /// though to the solver it is as still as the level.
    pub is_character: bool,
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
