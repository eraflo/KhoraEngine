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

//! A 4x4 column-major matrix.

use super::{is_invertible, Quaternion, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::ops::Mul;

// --- Mat4 ---

/// A 4x4 column-major matrix, used for 3D affine transformations.
///
/// This is the primary type for representing transformations (translation, rotation,
/// scale) in 3D space. It is also used for camera view and projection matrices.
/// The memory layout is column-major, which is compatible with modern graphics APIs
/// like Vulkan, Metal, and DirectX.
///
/// # Examples
///
/// ```rust
/// use khora_core::math::{Mat4, Quaternion, Vec3};
/// use std::f32::consts::FRAC_PI_2;
///
/// // The identity matrix leaves a point unchanged.
/// assert_eq!(Mat4::IDENTITY.transform_point(Vec3::ONE), Vec3::ONE);
///
/// // Compose translation ∘ rotation ∘ scale by multiplying matrices
/// // (applied right-to-left).
/// let model = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0))
///     * Mat4::from_quat(Quaternion::from_axis_angle(Vec3::Y, FRAC_PI_2))
///     * Mat4::from_scale(Vec3::ONE * 2.0);
/// let moved = model.transform_point(Vec3::ZERO);
/// assert!((moved - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
///
/// // A perspective projection for a 16:9 viewport.
/// let proj = Mat4::perspective_rh_zo(FRAC_PI_2, 16.0 / 9.0, 0.1, 1000.0);
/// assert!(proj != Mat4::IDENTITY);
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize,
)]
#[repr(C)]
pub struct Mat4 {
    /// The columns of the matrix. `cols[0]` is the first column, and so on.
    pub cols: [Vec4; 4],
}

impl Mat4 {
    /// The 4x4 identity matrix.
    pub const IDENTITY: Self = Self {
        cols: [Vec4::X, Vec4::Y, Vec4::Z, Vec4::W],
    };

    /// A 4x4 matrix with all elements set to 0.
    pub const ZERO: Self = Self {
        cols: [Vec4::ZERO; 4],
    };

    /// Creates a new matrix from four column vectors.
    #[inline]
    pub fn from_cols(c0: Vec4, c1: Vec4, c2: Vec4, c3: Vec4) -> Self {
        Self {
            cols: [c0, c1, c2, c3],
        }
    }

    /// Returns a row of the matrix as a `Vec4`.
    #[inline]
    pub fn get_row(&self, index: usize) -> Vec4 {
        Vec4 {
            x: self.cols[0].get(index),
            y: self.cols[1].get(index),
            z: self.cols[2].get(index),
            w: self.cols[3].get(index),
        }
    }

    /// Returns the matrix as a 2D array of columns: `[[x, y, z, w]; 4]`.
    #[inline]
    pub const fn to_cols_array_2d(&self) -> [[f32; 4]; 4] {
        [
            [
                self.cols[0].x,
                self.cols[0].y,
                self.cols[0].z,
                self.cols[0].w,
            ],
            [
                self.cols[1].x,
                self.cols[1].y,
                self.cols[1].z,
                self.cols[1].w,
            ],
            [
                self.cols[2].x,
                self.cols[2].y,
                self.cols[2].z,
                self.cols[2].w,
            ],
            [
                self.cols[3].x,
                self.cols[3].y,
                self.cols[3].z,
                self.cols[3].w,
            ],
        ]
    }

    /// Creates a translation matrix.
    ///
    /// # Arguments
    ///
    /// * `v`: The translation vector to apply.
    #[inline]
    pub fn from_translation(v: Vec3) -> Self {
        Self {
            cols: [
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(v.x, v.y, v.z, 1.0),
            ],
        }
    }

    /// Creates a non-uniform scaling matrix.
    #[inline]
    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            cols: [
                Vec4::new(scale.x, 0.0, 0.0, 0.0),
                Vec4::new(0.0, scale.y, 0.0, 0.0),
                Vec4::new(0.0, 0.0, scale.z, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a matrix for a rotation around the X-axis.
    ///
    /// # Arguments
    ///
    /// * `angle`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_x(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, c, s, 0.0),
                Vec4::new(0.0, -s, c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a matrix for a right-handed rotation around the Y-axis.
    ///
    /// # Arguments
    ///
    /// * `angle`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_y(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(c, 0.0, -s, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(s, 0.0, c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a matrix for a rotation around the Z-axis.
    ///
    /// # Arguments
    ///
    /// * `angle`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_z(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(c, s, 0.0, 0.0),
                Vec4::new(-s, c, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a rotation matrix from a normalized axis and an angle.
    ///
    /// # Arguments
    ///
    /// * `axis`: The axis of rotation. Must be a unit vector.
    /// * `angle`: The angle of rotation in radians.
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        let t = 1.0 - c;
        let x = axis.x;
        let y = axis.y;
        let z = axis.z;

        Self {
            cols: [
                Vec4::new(t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0),
                Vec4::new(t * y * x + s * z, t * y * y + c, t * y * z - s * x, 0.0),
                Vec4::new(t * z * x - s * y, t * z * y + s * x, t * z * z + c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a rotation matrix from a quaternion.
    #[inline]
    pub fn from_quat(q: Quaternion) -> Self {
        let x = q.x;
        let y = q.y;
        let z = q.z;
        let w = q.w;
        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        Self::from_cols(
            Vec4::new(1.0 - (yy + zz), xy + wz, xz - wy, 0.0),
            Vec4::new(xy - wz, 1.0 - (xx + zz), yz + wx, 0.0),
            Vec4::new(xz + wy, yz - wx, 1.0 - (xx + yy), 0.0),
            Vec4::W,
        )
    }

    /// Creates a right-handed perspective projection matrix with a [0, 1] depth range (ZO).
    ///
    /// # Arguments
    ///
    /// * `fov_y_radians`: Vertical field of view in radians.
    /// * `aspect_ratio`: Width divided by height of the viewport.
    /// * `z_near`: Distance to the near clipping plane (must be positive).
    /// * `z_far`: Distance to the far clipping plane (must be positive and > `z_near`).
    ///
    /// The planes are a precondition: passing others is a programming error
    /// and panics. Values that come from data — a camera or a light authored
    /// in a scene, an inspector, a script — go through
    /// [`Mat4::try_perspective_rh_zo`] instead.
    #[inline]
    pub fn perspective_rh_zo(
        fov_y_radians: f32,
        aspect_ratio: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        assert!(
            z_near > 0.0 && z_far > z_near,
            "a perspective needs 0 < near < far (near {z_near}, far {z_far})"
        );
        Self::perspective_unchecked(fov_y_radians, aspect_ratio, z_near, z_far)
    }

    /// The perspective of [`Mat4::perspective_rh_zo`], with no check at all.
    #[inline]
    fn perspective_unchecked(
        fov_y_radians: f32,
        aspect_ratio: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let tan_half_fovy = (fov_y_radians / 2.0).tan();
        let f = 1.0 / tan_half_fovy;
        let aa = f / aspect_ratio;
        let bb = f;
        let cc = z_far / (z_near - z_far);
        let dd = (z_near * z_far) / (z_near - z_far);

        Self::from_cols(
            Vec4::new(aa, 0.0, 0.0, 0.0),
            Vec4::new(0.0, bb, 0.0, 0.0),
            Vec4::new(0.0, 0.0, cc, -1.0),
            Vec4::new(0.0, 0.0, dd, 0.0),
        )
    }

    /// Creates a right-handed perspective projection matrix with a [0, 1] depth
    /// range (ZO), or `None` when the inputs cannot make one.
    ///
    /// The constructor for data — a camera authored in a scene, an inspector,
    /// a script. It returns `None` unless every input is finite,
    /// `0 < fov_y_radians < π`, `aspect_ratio > 0`, `0 < z_near < z_far`, and
    /// every entry of the resulting matrix is finite (a near-zero field of view
    /// can overflow `f / aspect`). When it returns `Some`, the matrix equals
    /// [`Mat4::perspective_rh_zo`] for the same inputs.
    #[inline]
    pub fn try_perspective_rh_zo(
        fov_y_radians: f32,
        aspect_ratio: f32,
        z_near: f32,
        z_far: f32,
    ) -> Option<Self> {
        let all_finite = [fov_y_radians, aspect_ratio, z_near, z_far]
            .iter()
            .all(|v| v.is_finite());
        let usable = all_finite
            && fov_y_radians > 0.0
            && fov_y_radians < std::f32::consts::PI
            && aspect_ratio > 0.0
            && z_near > 0.0
            && z_far > z_near;
        if !usable {
            return None;
        }
        let m = Self::perspective_unchecked(fov_y_radians, aspect_ratio, z_near, z_far);
        is_finite(&m).then_some(m)
    }

    /// Creates a right-handed orthographic projection matrix with a [0, 1] depth range (ZO).
    ///
    /// Bounds that make no volume (`right == left`, …) give infinite entries;
    /// values that come from data go through [`Mat4::try_orthographic_rh_zo`].
    #[inline]
    pub fn orthographic_rh_zo(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        let rml = right - left;
        let rpl = right + left;
        let tmb = top - bottom;
        let tpb = top + bottom;
        let fmn = z_far - z_near;
        let aa = 2.0 / rml;
        let bb = 2.0 / tmb;
        let cc = -1.0 / fmn;
        let dd = -rpl / rml;
        let ee = -tpb / tmb;
        let ff = -z_near / fmn;

        Self::from_cols(
            Vec4::new(aa, 0.0, 0.0, 0.0),
            Vec4::new(0.0, bb, 0.0, 0.0),
            Vec4::new(0.0, 0.0, cc, 0.0),
            Vec4::new(dd, ee, ff, 1.0),
        )
    }

    /// Creates a right-handed orthographic projection matrix with a [0, 1]
    /// depth range (ZO), or `None` when the bounds cannot make one.
    ///
    /// The constructor for data. It returns `None` unless every input is
    /// finite, `right > left`, `top > bottom`, `z_far > z_near` (`z_near` may be
    /// negative), and every entry of the resulting matrix is finite. When it
    /// returns `Some`, the matrix equals [`Mat4::orthographic_rh_zo`] for the
    /// same inputs.
    #[inline]
    pub fn try_orthographic_rh_zo(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        z_near: f32,
        z_far: f32,
    ) -> Option<Self> {
        let all_finite = [left, right, bottom, top, z_near, z_far]
            .iter()
            .all(|v| v.is_finite());
        if !(all_finite && right > left && top > bottom && z_far > z_near) {
            return None;
        }
        let m = Self::orthographic_rh_zo(left, right, bottom, top, z_near, z_far);
        is_finite(&m).then_some(m)
    }

    /// Creates a right-handed view matrix for a camera looking from `eye` towards `target`.
    ///
    /// # Arguments
    ///
    /// * `eye`: The position of the camera in world space.
    /// * `target`: The point in world space that the camera is looking at.
    /// * `up`: A vector indicating the "up" direction of the world (commonly `Vec3::Y`).
    ///
    /// # Returns
    ///
    /// Returns `Some(Mat4)` if a valid view matrix can be constructed, or `None` if
    /// `eye` and `target` are too close, or if `up` is parallel to the view direction.
    #[inline]
    pub fn look_at_rh(eye: Vec3, target: Vec3, up: Vec3) -> Option<Self> {
        let forward = target - eye;
        if forward.length_squared() < crate::math::EPSILON * crate::math::EPSILON {
            return None;
        }
        let f = forward.normalize();
        let s = f.cross(up);
        if s.length_squared() < crate::math::EPSILON * crate::math::EPSILON {
            return None;
        }
        let s = s.normalize();
        let u = s.cross(f);

        Some(Self::from_cols(
            Vec4::new(s.x, u.x, -f.x, 0.0),
            Vec4::new(s.y, u.y, -f.y, 0.0),
            Vec4::new(s.z, u.z, -f.z, 0.0),
            Vec4::new(-eye.dot(s), -eye.dot(u), eye.dot(f), 1.0),
        ))
    }

    /// Returns the transpose of the matrix, where rows and columns are swapped.
    #[inline]
    pub fn transpose(&self) -> Self {
        Self::from_cols(
            Vec4::new(
                self.cols[0].x,
                self.cols[1].x,
                self.cols[2].x,
                self.cols[3].x,
            ),
            Vec4::new(
                self.cols[0].y,
                self.cols[1].y,
                self.cols[2].y,
                self.cols[3].y,
            ),
            Vec4::new(
                self.cols[0].z,
                self.cols[1].z,
                self.cols[2].z,
                self.cols[3].z,
            ),
            Vec4::new(
                self.cols[0].w,
                self.cols[1].w,
                self.cols[2].w,
                self.cols[3].w,
            ),
        )
    }

    /// Computes the determinant of the matrix.
    pub fn determinant(&self) -> f32 {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        let c3 = self.cols[3];

        let m00 = c1.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c1.z * c3.w - c3.z * c1.w)
            + c3.y * (c1.z * c2.w - c2.z * c1.w);
        let m01 = c0.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c0.z * c3.w - c3.z * c0.w)
            + c3.y * (c0.z * c2.w - c2.z * c0.w);
        let m02 = c0.y * (c1.z * c3.w - c3.z * c1.w) - c1.y * (c0.z * c3.w - c3.z * c0.w)
            + c3.y * (c0.z * c1.w - c1.z * c0.w);
        let m03 = c0.y * (c1.z * c2.w - c2.z * c1.w) - c1.y * (c0.z * c2.w - c2.z * c0.w)
            + c2.y * (c0.z * c1.w - c1.z * c0.w);

        c0.x * m00 - c1.x * m01 + c2.x * m02 - c3.x * m03
    }

    /// Computes the inverse of the matrix.
    /// Returns `None` if the matrix is not invertible.
    ///
    /// Invertibility is judged relative to the matrix's own scale: the
    /// determinant is compared with the product of the column lengths (its
    /// largest possible magnitude), not with a fixed threshold. A well-shaped
    /// matrix with small entries — an orthographic projection of a large view,
    /// a uniformly shrunk transform — inverts; a degenerate one, or one holding
    /// a NaN or an infinity, does not.
    pub fn inverse(&self) -> Option<Self> {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        let c3 = self.cols[3];

        let a00 = c1.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c1.z * c3.w - c3.z * c1.w)
            + c3.y * (c1.z * c2.w - c2.z * c1.w);
        let a01 = -(c1.x * (c2.z * c3.w - c3.z * c2.w) - c2.x * (c1.z * c3.w - c3.z * c1.w)
            + c3.x * (c1.z * c2.w - c2.z * c1.w));
        let a02 = c1.x * (c2.y * c3.w - c3.y * c2.w) - c2.x * (c1.y * c3.w - c3.y * c1.w)
            + c3.x * (c1.y * c2.w - c2.y * c1.w);
        let a03 = -(c1.x * (c2.y * c3.z - c3.y * c2.z) - c2.x * (c1.y * c3.z - c3.y * c1.z)
            + c3.x * (c1.y * c2.z - c2.y * c1.z));

        let a10 = -(c0.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c0.z * c3.w - c3.z * c0.w)
            + c3.y * (c0.z * c2.w - c2.z * c0.w));
        let a11 = c0.x * (c2.z * c3.w - c3.z * c2.w) - c2.x * (c0.z * c3.w - c3.z * c0.w)
            + c3.x * (c0.z * c2.w - c2.z * c0.w);
        let a12 = -(c0.x * (c2.y * c3.w - c3.y * c2.w) - c2.x * (c0.y * c3.w - c3.y * c0.w)
            + c3.x * (c0.y * c2.w - c2.y * c0.w));
        let a13 = c0.x * (c2.y * c3.z - c3.y * c2.z) - c2.x * (c0.y * c3.z - c3.y * c0.z)
            + c3.x * (c0.y * c2.z - c2.y * c0.z);

        let a20 = c0.y * (c1.z * c3.w - c3.z * c1.w) - c1.y * (c0.z * c3.w - c3.z * c0.w)
            + c3.y * (c0.z * c1.w - c1.z * c0.w);
        let a21 = -(c0.x * (c1.z * c3.w - c3.z * c1.w) - c1.x * (c0.z * c3.w - c3.z * c0.w)
            + c3.x * (c0.z * c1.w - c1.z * c0.w));
        let a22 = c0.x * (c1.y * c3.w - c3.y * c1.w) - c1.x * (c0.y * c3.w - c3.y * c0.w)
            + c3.x * (c0.y * c1.w - c1.y * c0.w);
        let a23 = -(c0.x * (c1.y * c3.z - c3.y * c1.z) - c1.x * (c0.y * c3.z - c3.y * c0.z)
            + c3.x * (c0.y * c1.z - c1.y * c0.z));

        let a30 = -(c0.y * (c1.z * c2.w - c2.z * c1.w) - c1.y * (c0.z * c2.w - c2.z * c0.w)
            + c2.y * (c0.z * c1.w - c1.z * c0.w));
        let a31 = c0.x * (c1.z * c2.w - c2.z * c1.w) - c1.x * (c0.z * c2.w - c2.z * c0.w)
            + c2.x * (c0.z * c1.w - c1.z * c0.w);
        let a32 = -(c0.x * (c1.y * c2.w - c2.y * c1.w) - c1.x * (c0.y * c2.w - c2.y * c0.w)
            + c2.x * (c0.y * c1.w - c1.y * c0.w));
        let a33 = c0.x * (c1.y * c2.z - c2.y * c1.z) - c1.x * (c0.y * c2.z - c2.y * c0.z)
            + c2.x * (c0.y * c1.z - c1.y * c0.z);

        let det = c0.x * a00 + c1.x * a10 + c2.x * a20 + c3.x * a30;
        let scale = c0.dot(c0).sqrt() * c1.dot(c1).sqrt() * c2.dot(c2).sqrt() * c3.dot(c3).sqrt();
        if !is_invertible(det, scale) {
            return None;
        }
        let inv_det = 1.0 / det;

        Some(Self::from_cols(
            Vec4::new(a00 * inv_det, a10 * inv_det, a20 * inv_det, a30 * inv_det),
            Vec4::new(a01 * inv_det, a11 * inv_det, a21 * inv_det, a31 * inv_det),
            Vec4::new(a02 * inv_det, a12 * inv_det, a22 * inv_det, a32 * inv_det),
            Vec4::new(a03 * inv_det, a13 * inv_det, a23 * inv_det, a33 * inv_det),
        ))
    }

    /// Computes the inverse of an affine transformation matrix more efficiently
    /// and with better numerical stability than the general `inverse` method.
    ///
    /// An affine matrix is one composed of only translation, rotation, and scale.
    ///
    /// # Returns
    ///
    /// `None` if the matrix is not affine or is not invertible.
    #[inline]
    pub fn affine_inverse(&self) -> Option<Self> {
        let c0 = self.cols[0].truncate();
        let c1 = self.cols[1].truncate();
        let c2 = self.cols[2].truncate();
        let translation = self.cols[3].truncate();
        let det3x3 = c0.x * (c1.y * c2.z - c2.y * c1.z) - c1.x * (c0.y * c2.z - c2.y * c0.z)
            + c2.x * (c0.y * c1.z - c1.y * c0.z);

        let scale = c0.length() * c1.length() * c2.length();
        if !is_invertible(det3x3, scale) {
            return None;
        }

        // The rows of the inverse of the 3×3 whose columns are c0, c1, c2 are
        // the cross products of the two other columns over the determinant:
        // row i dotted with column j is the triple product, det when i = j
        // and zero otherwise.
        let inv_det3x3 = 1.0 / det3x3;
        let row0 = c1.cross(c2) * inv_det3x3;
        let row1 = c2.cross(c0) * inv_det3x3;
        let row2 = c0.cross(c1) * inv_det3x3;
        let inv_translation = -Vec3::new(
            row0.dot(translation),
            row1.dot(translation),
            row2.dot(translation),
        );

        Some(Self::from_cols(
            Vec4::new(row0.x, row1.x, row2.x, 0.0),
            Vec4::new(row0.y, row1.y, row2.y, 0.0),
            Vec4::new(row0.z, row1.z, row2.z, 0.0),
            Vec4::new(inv_translation.x, inv_translation.y, inv_translation.z, 1.0),
        ))
    }

    /// Transforms a 3D point by this matrix (treating it as a homogeneous point with w=1).
    ///
    /// This applies the full affine transformation including translation, rotation, and scale.
    #[inline]
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        (*self * Vec4::from_vec3(p, 1.0)).truncate()
    }

    /// Transforms a 3D direction vector by this matrix (treating it as w=0).
    ///
    /// This applies rotation and scale but **not** translation. Use this for
    /// normals, axes, and other direction vectors.
    #[inline]
    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        (*self * Vec4::from_vec3(v, 0.0)).truncate()
    }
}

// --- Operators Overloading ---

impl Default for Mat4 {
    /// Returns the 4x4 identity matrix.
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul<Mat4> for Mat4 {
    type Output = Self;
    /// Multiplies this matrix by another `Mat4`. Note that matrix multiplication is not commutative.
    #[inline]
    fn mul(self, rhs: Mat4) -> Self::Output {
        let mut result_cols = [Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        }; 4];
        for (c_idx, target_col_ref_mut) in result_cols.iter_mut().enumerate().take(4) {
            let col_from_rhs = rhs.cols[c_idx];
            *target_col_ref_mut = Vec4 {
                x: self.get_row(0).dot(col_from_rhs),
                y: self.get_row(1).dot(col_from_rhs),
                z: self.get_row(2).dot(col_from_rhs),
                w: self.get_row(3).dot(col_from_rhs),
            };
        }
        Mat4 { cols: result_cols }
    }
}

impl Mul<Vec4> for Mat4 {
    type Output = Vec4;
    /// Transforms a `Vec4` by this matrix.
    #[inline]
    fn mul(self, rhs: Vec4) -> Self::Output {
        self.cols[0] * rhs.x + self.cols[1] * rhs.y + self.cols[2] * rhs.z + self.cols[3] * rhs.w
    }
}

/// Whether every entry of `m` is finite.
#[inline]
fn is_finite(m: &Mat4) -> bool {
    m.cols
        .iter()
        .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite() && c.w.is_finite())
}
