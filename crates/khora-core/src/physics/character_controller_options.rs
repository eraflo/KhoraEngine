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

//! Options for resolving kinematic character movement.

use serde::{Deserialize, Serialize};

/// Options for resolving kinematic character movement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CharacterControllerOptions {
    /// Max height of obstacles the character can step over.
    pub autostep_height: f32,
    /// Min width of obstacles for autostepping.
    pub autostep_min_width: f32,
    /// Whether autostepping is enabled.
    pub autostep_enabled: bool,
    /// Max angle for climbing slopes.
    pub max_slope_climb_angle: f32,
    /// Min angle for sliding down slopes.
    pub min_slope_slide_angle: f32,
    /// Distance to maintain from obstacles.
    pub offset: f32,
}
