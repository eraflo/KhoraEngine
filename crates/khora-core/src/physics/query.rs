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

//! Scene queries and the character controller: raycasts and movement options.

use serde::{Deserialize, Serialize};

use super::handle::ColliderHandle;
use crate::math::Vec3;

pub use super::character_controller_options::CharacterControllerOptions;

/// Information about a raycast hit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RaycastHit {
    /// The collider that was hit.
    pub collider: ColliderHandle,
    /// Distance from ray origin to hit point.
    pub distance: f32,
    /// Normal vector at the hit point.
    pub normal: Vec3,
    /// Exact position of the hit.
    pub position: Vec3,
}
