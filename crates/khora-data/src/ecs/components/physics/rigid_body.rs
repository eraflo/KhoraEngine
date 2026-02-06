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

/// Component representing a rigid body in the physics simulation.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct RigidBody {
    /// Opaque handle used by the physics provider.
    pub handle: Option<RigidBodyHandle>,
    /// Global type of the body (Static, Dynamic, Kinematic).
    pub body_type: BodyType,
    /// Mass of the body in kilograms.
    pub mass: f32,
    /// Whether to enable Continuous Collision Detection (CCD).
    pub ccd_enabled: bool,
    /// Current linear velocity.
    pub linear_velocity: Vec3,
    /// Current angular velocity.
    pub angular_velocity: Vec3,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            handle: None,
            body_type: BodyType::Dynamic,
            mass: 1.0,
            ccd_enabled: false,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }
}

impl RigidBody {
    /// Creates a new dynamic rigid body.
    pub fn new_dynamic(mass: f32) -> Self {
        Self {
            handle: None,
            body_type: BodyType::Dynamic,
            mass,
            ccd_enabled: false,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }

    /// Creates a new static rigid body.
    pub fn new_static() -> Self {
        Self {
            handle: None,
            body_type: BodyType::Static,
            mass: 0.0,
            ccd_enabled: false,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }
}
