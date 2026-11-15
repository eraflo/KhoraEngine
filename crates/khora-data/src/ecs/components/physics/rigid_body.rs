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
use khora_core::physics::{BodyType, RigidBodyHandle};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// A body the physics simulation moves: dynamic, static or kinematic.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[component(domain = Physics)]
pub struct RigidBody {
    /// The physics provider's handle, set while the body exists there; not saved.
    #[component(skip)]
    pub handle: Option<RigidBodyHandle>,
    /// How the body moves: by forces (dynamic), never (static), or by code (kinematic).
    pub body_type: BodyType,
    /// The mass, in kilograms; only a dynamic body uses it.
    pub mass: f32,
    /// Whether continuous collision detection keeps a fast, small body from tunnelling.
    pub ccd_enabled: bool,
    /// The linear velocity the body starts with, in metres per second.
    ///
    /// Applied once, when the body is created. What the body is doing now is
    /// its [`BodyMotion`](crate::ecs::BodyMotion).
    pub initial_velocity: Vec3,
    /// The angular velocity the body starts with, in radians per second; applied once.
    pub initial_angular_velocity: Vec3,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            handle: None,
            body_type: BodyType::Dynamic,
            mass: 1.0,
            ccd_enabled: false,
            initial_velocity: Vec3::ZERO,
            initial_angular_velocity: Vec3::ZERO,
        }
    }
}

impl RigidBody {
    /// Creates a dynamic body of `mass` kilograms.
    pub fn new_dynamic(mass: f32) -> Self {
        Self {
            handle: None,
            body_type: BodyType::Dynamic,
            mass,
            ccd_enabled: false,
            initial_velocity: Vec3::ZERO,
            initial_angular_velocity: Vec3::ZERO,
        }
    }

    /// Creates a static body.
    pub fn new_static() -> Self {
        Self {
            handle: None,
            body_type: BodyType::Static,
            mass: 0.0,
            ccd_enabled: false,
            initial_velocity: Vec3::ZERO,
            initial_angular_velocity: Vec3::ZERO,
        }
    }
}
