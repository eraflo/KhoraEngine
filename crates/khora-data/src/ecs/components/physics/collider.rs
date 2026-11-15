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

/// The shape its entity collides with, and how its surface behaves on contact.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[component(domain = Physics)]
pub struct Collider {
    /// The physics provider's handle, set while the collider exists there; not saved.
    #[component(skip)]
    pub handle: Option<ColliderHandle>,
    /// The shape, in the entity's local units.
    ///
    /// Scaled by the entity's world scale, as its mesh is, so it collides at
    /// the size it is drawn at.
    pub shape: ColliderShape,
    /// The friction coefficient, `0` for ice; usually between `0` and `1`.
    pub friction: f32,
    /// The restitution (bounciness): `0` absorbs an impact, `1` bounces back fully.
    pub restitution: f32,
    /// Whether this collider is a sensor, reporting what enters and leaves it.
    ///
    /// A sensor reports with or without `ActiveEvents`, but never pushes
    /// anything, never stops a character, and adds no mass to its body.
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
    /// Creates a box collider of the given half extents.
    pub fn new_box(half_extents: Vec3) -> Self {
        Self {
            handle: None,
            shape: ColliderShape::Box(half_extents),
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
        }
    }

    /// Creates a sphere collider of the given radius.
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
