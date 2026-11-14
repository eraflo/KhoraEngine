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

use crate::math::{Quat, Vec3};

pub use super::collider_desc::{ColliderDesc, ColliderShape};

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
