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

/// Component for kinematic character movement.
/// It provides high-level movement resolution (slopes, steps, etc.)
/// that is more suitable for player characters than raw rigid-body physics.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct KinematicCharacterController {
    /// The translation to apply in the current frame.
    pub desired_translation: Vec3,
    /// The offset from obstacles to maintain.
    pub offset: f32,
    /// Maximum angle for climbing slopes (in radians).
    pub max_slope_climb_angle: f32,
    /// Minimum angle for sliding down a slope (in radians).
    pub min_slope_slide_angle: f32,
    /// Maximum height for autostepping.
    pub autostep_height: f32,
    /// Minimum width for autostepping obstacles.
    pub autostep_min_width: f32,
    /// Whether autostepping is enabled.
    pub autostep_enabled: bool,
    /// Whether the character is currently grounded.
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
