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

use khora_core::math::Vec3;
use khora_core::physics::{ColliderHandle, ColliderShape};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Component representing a collider attached to an entity.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct Collider {
    /// Opaque handle used by the physics provider.
    pub handle: Option<ColliderHandle>,
    /// Shape of the collider.
    pub shape: ColliderShape,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
    /// Whether this collider is a sensor (does not respond to forces).
    pub is_sensor: bool,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            handle: None,
            shape: ColliderShape::Sphere(0.5),
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
        }
    }
}

impl Collider {
    /// Creates a new box collider.
    pub fn new_box(half_extents: Vec3) -> Self {
        Self {
            handle: None,
            shape: ColliderShape::Box(half_extents),
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
        }
    }

    /// Creates a new sphere collider.
    pub fn new_sphere(radius: f32) -> Self {
        Self {
            handle: None,
            shape: ColliderShape::Sphere(radius),
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
        }
    }
}
