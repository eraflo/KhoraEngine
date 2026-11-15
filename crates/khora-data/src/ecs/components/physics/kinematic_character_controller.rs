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
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Moves a character by intent, resolving slopes and steps, rather than by forces.
///
/// Suited to player characters, where a dynamic rigid body would be hard to steer.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[component(domain = Physics)]
pub struct KinematicCharacterController {
    /// The move wanted this frame, in world units; reset to zero once applied.
    pub desired_translation: Vec3,
    /// The gap kept from obstacles, in world units.
    pub offset: f32,
    /// The steepest slope the character climbs, in radians.
    pub max_slope_climb_angle: f32,
    /// The gentlest slope the character slides down, in radians.
    pub min_slope_slide_angle: f32,
    /// The highest step climbed automatically, in world units.
    pub autostep_height: f32,
    /// The narrowest step climbed automatically, in world units.
    pub autostep_min_width: f32,
    /// Whether steps are climbed automatically.
    pub autostep_enabled: bool,
    /// Whether the character stood on the ground after its last move; written by the engine.
    pub is_grounded: bool,
}

impl Default for KinematicCharacterController {
    fn default() -> Self {
        Self {
            desired_translation: Vec3::ZERO,
            offset: 0.01,
            max_slope_climb_angle: 0.785, // 45 degrees
            min_slope_slide_angle: 0.523, // 30 degrees
            autostep_height: 0.3,
            autostep_min_width: 0.2,
            autostep_enabled: true,
            is_grounded: false,
        }
    }
}
