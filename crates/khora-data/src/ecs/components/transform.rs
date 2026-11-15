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

use khora_core::math::{Mat4, Quaternion, Vec3};
use khora_macros::Component;

/// An entity's authored position, rotation and scale, relative to its `Parent` or the world.
///
/// This is the pose to edit; the engine composes it into the `GlobalTransform`.
/// A simulated body's pose lives in `SimulatedTransform` instead.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[component(domain = Spatial)]
pub struct Transform {
    /// The position, in world units, in the parent's space.
    pub translation: Vec3,
    /// The rotation, as a unit quaternion.
    pub rotation: Quaternion,
    /// The scale along each local axis; `1` leaves the size unchanged.
    pub scale: Vec3,
}

impl Transform {
    /// Creates a `Transform` from a translation, a rotation and a scale.
    pub fn new(translation: Vec3, rotation: Quaternion, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Creates a `Transform` that only translates.
    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Creates the identity `Transform`: at the origin, unrotated, unscaled.
    pub fn identity() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Returns the matrix that scales, then rotates, then translates.
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_translation(self.translation)
            * Mat4::from_quat(self.rotation)
            * Mat4::from_scale(self.scale)
    }
}

impl Default for Transform {
    /// Returns the identity `Transform`.
    fn default() -> Self {
        Self::identity()
    }
}
