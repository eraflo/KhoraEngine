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

use khora_core::math::{affine_transform::AffineTransform, Mat4};
use khora_macros::Component;

/// An entity's world-space pose: its `Transform` composed with its ancestors'.
///
/// Derived and never saved. Only the transform propagation system writes it;
/// rendering and physics read it.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[component(domain = Spatial, provenance = Derived)]
pub struct GlobalTransform(pub AffineTransform);

impl GlobalTransform {
    /// Creates a `GlobalTransform` from a world matrix.
    pub fn new(matrix: Mat4) -> Self {
        Self(AffineTransform(matrix))
    }

    /// Creates an identity `GlobalTransform`.
    pub fn identity() -> Self {
        Self(AffineTransform::IDENTITY)
    }

    /// Returns the world matrix.
    pub fn to_matrix(&self) -> Mat4 {
        self.0.into()
    }

    /// Creates a `GlobalTransform` that only translates to `position`.
    pub fn at_position(position: khora_core::math::Vec3) -> Self {
        Self::new(Mat4::from_translation(position))
    }
}

impl Default for GlobalTransform {
    /// Returns the identity `GlobalTransform`.
    fn default() -> Self {
        Self::identity()
    }
}
