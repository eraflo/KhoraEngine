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

/// A component that describes an entity's position, rotation, and scale
/// relative to its `Parent`. If the entity has no `Parent`, this is relative
/// to the world origin.
///
/// This is the component that users and other systems should modify. A dedicated
/// transform propagation system will use this component's value to calculate
/// the final `GlobalTransform`.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Transform {
    /// The translation (position) of the entity.
    pub translation: Vec3,
    /// The rotation of the entity, represented as a quaternion.
    pub rotation: Quaternion,
    /// The scale of the entity.
    pub scale: Vec3,
}

impl Transform {
    /// Creates a new `Transform` with a given translation, rotation, and scale.
    pub fn new(translation: Vec3, rotation: Quaternion, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Creates a new `Transform` with a given translation, and identity rotation/scale.
    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Creates a new identity `Transform`, with no translation, rotation, or scaling.
    /// This represents the origin.
    pub fn identity() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Calculates the `Mat4` transformation matrix from this component's
    /// translation, rotation, and scale.
    ///
    /// The final matrix is calculated in the standard `Scale -> Rotate -> Translate` order.
    pub fn to_mat4(&self) -> Mat4 {
        // T * R * S
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
