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

use crate::ecs::component::Component;
use khora_core::math::Mat4;

/// A component that stores the final, calculated, world-space transformation of an entity.
///
/// This component's value is the result of combining the entity's local `Transform`
/// with the `GlobalTransform` of its `Parent`, recursively up to the root of the scene.
///
/// It is intended to be **read-only** for most systems (like rendering and physics).
/// It should only be written to by the dedicated transform propagation system.
/// This acts as a cache to avoid re-calculating the full transform hierarchy every time
/// it's needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalTransform(pub Mat4);

impl Component for GlobalTransform {}

impl GlobalTransform {
    /// Creates a new `GlobalTransform` from a `Mat4`.
    pub fn new(matrix: Mat4) -> Self {
        Self(matrix)
    }

    /// Creates a new identity `GlobalTransform`.
    pub fn identity() -> Self {
        Self(Mat4::IDENTITY)
    }

    /// Returns the inner `Mat4` representation.
    pub fn to_matrix(&self) -> Mat4 {
        self.0
    }
}

impl Default for GlobalTransform {
    /// Returns the identity `GlobalTransform`.
    fn default() -> Self {
        Self::identity()
    }
}
