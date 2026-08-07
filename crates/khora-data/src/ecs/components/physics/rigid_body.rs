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
#[component(domain = Physics)]
pub struct RigidBody {
    /// Opaque handle used by the physics provider.
    #[component(skip)]
    pub handle: Option<RigidBodyHandle>,
    /// Global type of the body (Static, Dynamic, Kinematic).
    pub body_type: BodyType,
    /// Mass of the body in kilograms.
    pub mass: f32,
    /// Whether to enable Continuous Collision Detection (CCD).
    pub ccd_enabled: bool,
    /// The linear velocity the body starts with.
    ///
    /// An **initial condition**, applied once when the body is created — not
    /// what it is doing now. The two were one field documented as "current"
    /// while being `Authored` and serialized, and the sync could not tell which
    /// it held: it pushed the authored value into the solver every frame, so
    /// gravity accumulated for exactly one step and was then erased. What the
    /// body is doing lives in [`BodyMotion`](crate::ecs::BodyMotion).
    pub initial_velocity: Vec3,
    /// The angular velocity the body starts with, for the reason
    /// [`initial_velocity`](Self::initial_velocity) gives.
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
    /// Creates a new dynamic rigid body.
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

    /// Creates a new static rigid body.
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
