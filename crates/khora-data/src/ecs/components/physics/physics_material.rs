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

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Defines the physical properties of a collider surface.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize)]
pub struct PhysicsMaterial {
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            friction: 0.5,
            restitution: 0.0,
        }
    }
}
