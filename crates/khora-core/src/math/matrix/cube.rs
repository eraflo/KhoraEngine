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
    /// The near plane, in world units, of every light-space perspective a
    /// shadow is rendered through (a point light's cube faces, a spot
    /// light's cone). The shader-side depth recompute used in
    /// omnidirectional shadow sampling assumes the same constant, so a light
    /// whose range does not exceed it can cast no shadow.
    pub const SHADOW_NEAR: f32 = 0.1;

    /// View-projection matrix for one face of an omnidirectional cubemap
    /// rendered from `position`.
    ///
    /// Builds a 90°-FOV perspective with `[SHADOW_NEAR, far]` depth range,
    /// then a look-at matrix using [`CubeFace::forward`] and [`CubeFace::up`]
    /// in the wgpu cubemap convention.
    ///
    /// `far` should be the light's effective range in world units, and must
    /// exceed [`Mat4::SHADOW_NEAR`] (a precondition: a range that comes from
    /// data goes through [`Mat4::try_cube_face_view_projs`]).
    pub fn cube_face_view_proj(position: Vec3, face: CubeFace, far: f32) -> Mat4 {
        Mat4::perspective_rh_zo(FRAC_PI_2, 1.0, Mat4::SHADOW_NEAR, far) * face_view(position, face)
    }

    /// Six view-projection matrices, one per cube face, in
    /// [`CubeFace::ALL`] order (`[+X, -X, +Y, -Y, +Z, -Z]`).
    ///
    /// Equivalent to:
    /// ```ignore
    /// CubeFace::ALL.map(|f| Mat4::cube_face_view_proj(position, f, far))
    /// ```
    pub fn cube_face_view_projs(position: Vec3, far: f32) -> [Mat4; 6] {
        CubeFace::ALL.map(|f| Mat4::cube_face_view_proj(position, f, far))
    }

    /// The six cube-face view-projections of [`Mat4::cube_face_view_projs`],
    /// or `None` when `far` cannot make them.
    ///
    /// The constructor for data — a point light's authored range. Built on
    /// [`Mat4::try_perspective_rh_zo`] with a 90° field of view, aspect 1 and
    /// depth range `[SHADOW_NEAR, far]`: `None` unless `far` is finite and
    /// greater than [`Mat4::SHADOW_NEAR`]. When it returns `Some`, the
    /// matrices equal [`Mat4::cube_face_view_projs`] for the same inputs.
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
