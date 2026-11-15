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

//! [`SimulatedTransform`]: where the simulation put an entity, as opposed to
//! where its author did.
//!
//! See the book: concepts/physics.md, "Who owns a pose".

use khora_core::math::{AffineTransform, Mat4, Quat, Vec3};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The world pose the simulation currently gives an entity, never composed with a parent.
///
/// Runtime state, never saved. Present only while something simulates the
/// entity. [`transform_propagation`] prefers
/// it over the authored `Transform` when it is there; [`physics_release`]
/// removes it once the entity has neither a body nor a character controller,
/// handing the pose back to the `Transform`.
///
/// [`transform_propagation`]: crate::ecs::systems::transform_propagation
/// [`physics_release`]: crate::ecs::systems::physics_release
#[derive(Debug, Clone, Copy, PartialEq, Component, Serialize, Deserialize)]
#[component(domain = Spatial, provenance = Runtime)]
pub struct SimulatedTransform(pub AffineTransform);

impl SimulatedTransform {
    /// Creates the pose a provider just reported.
    ///
    /// It holds no scale: a solver never resizes a body, and a scale here would
    /// overwrite the author's on the way to `GlobalTransform`.
    pub fn from_parts(translation: Vec3, rotation: Quat) -> Self {
        Self(AffineTransform(
            Mat4::from_translation(translation) * Mat4::from_quat(rotation),
        ))
    }

    /// Returns the world position.
    pub fn translation(&self) -> Vec3 {
        self.0.translation()
    }

    /// Returns the world rotation.
    pub fn rotation(&self) -> Quat {
        self.0.rotation()
    }
}

impl Default for SimulatedTransform {
    fn default() -> Self {
        Self(AffineTransform::IDENTITY)
    }
}
