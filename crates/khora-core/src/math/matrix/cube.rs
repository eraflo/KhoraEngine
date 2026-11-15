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

//! The light-space projections a shadow is rendered through: a point
//! light's six cube faces, and the near plane every shadow shares.

use super::{Mat4, Vec3};
use crate::math::{CubeFace, FRAC_PI_2};

impl Mat4 {
    /// The near plane, in world units, of every light-space perspective a shadow renders through.
    ///
    /// Point-light cube faces and spot-light cones share it, and the omnidirectional shadow
    /// sampling shader assumes the same value: a light whose range does not exceed it casts no
    /// shadow.
    pub const SHADOW_NEAR: f32 = 0.1;

    /// Returns the view-projection of one cubemap face rendered from `position`.
    ///
    /// A right-handed 90° perspective over `[SHADOW_NEAR, far]` (depth 0 to 1), looking along
    /// [`CubeFace::forward`] with [`CubeFace::up`]. `far` is the light's range in world units; a
    /// range that comes from data goes through [`Mat4::try_cube_face_view_projs`].
    ///
    /// # Panics
    ///
    /// Panics if `far` does not exceed [`Mat4::SHADOW_NEAR`].
    pub fn cube_face_view_proj(position: Vec3, face: CubeFace, far: f32) -> Mat4 {
        Mat4::perspective_rh_zo(FRAC_PI_2, 1.0, Mat4::SHADOW_NEAR, far) * face_view(position, face)
    }

    /// Returns [`Mat4::cube_face_view_proj`] for each face, in [`CubeFace::ALL`] order.
    ///
    /// # Panics
    ///
    /// Panics if `far` does not exceed [`Mat4::SHADOW_NEAR`].
    pub fn cube_face_view_projs(position: Vec3, far: f32) -> [Mat4; 6] {
        CubeFace::ALL.map(|f| Mat4::cube_face_view_proj(position, f, far))
    }

    /// Returns [`Mat4::cube_face_view_projs`], or `None` unless `far` is finite and exceeds
    /// [`Mat4::SHADOW_NEAR`].
    ///
    /// The constructor for data, such as a point light's authored range.
    pub fn try_cube_face_view_projs(position: Vec3, far: f32) -> Option<[Mat4; 6]> {
        let projection = Mat4::try_perspective_rh_zo(FRAC_PI_2, 1.0, Mat4::SHADOW_NEAR, far)?;
        Some(CubeFace::ALL.map(|f| projection * face_view(position, f)))
    }
}

/// The view from `position` through `face`; the identity when it has none
/// (a non-finite position).
fn face_view(position: Vec3, face: CubeFace) -> Mat4 {
    Mat4::look_at_rh(position, position + face.forward(), face.up()).unwrap_or(Mat4::IDENTITY)
}
