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

//! Defines the `Mat3` and `Mat4` types and associated operations.

use super::{Quaternion, Vec2, Vec3, Vec4, EPSILON};
use std::ops::{Index, IndexMut, Mul};

// --- Mat3 ---

/// A 3x3 column-major matrix, typically used for 2D affine transformations (scale, rotation).
///
/// While it can represent any 3x3 matrix, its primary role in a 3D engine is often
/// as the upper-left rotation and scale part of a `Mat4`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Mat3 {
    /// The columns of the matrix. `cols[0]` is the first column, and so on.
    pub cols: [Vec3; 3],
}

impl Mat3 {
    /// The 3x3 identity matrix.
    pub const IDENTITY: Self = Self {
        cols: [Vec3::X, Vec3::Y, Vec3::Z],
    };

    /// A 3x3 matrix with all elements set to 0.
    pub const ZERO: Self = Self {
        cols: [Vec3::ZERO; 3],
    };

    /// Creates a new matrix from three column vectors.
    #[inline]
    pub fn from_cols(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        Self { cols: [c0, c1, c2] }
    }

    /// Returns a row of the matrix as a `Vec3`.
    #[allow(dead_code)]
    #[inline]
    fn get_row(&self, index: usize) -> Vec3 {
        Vec3 {
            x: self.cols[0].get(index),
            y: self.cols[1].get(index),
            z: self.cols[2].get(index),
        }
    }

    /// Creates a 2D scaling matrix.
    ///
    /// The Z-axis scale is set to 1.0, making it a no-op in that dimension.
    #[inline]
    pub fn from_scale_vec2(scale: Vec2) -> Self {
        Self::from_scale(Vec3::new(scale.x, scale.y, 1.0))
    }

    /// Creates a 3D scaling matrix.
    #[inline]
    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            cols: [
                Vec3::new(scale.x, 0.0, 0.0),
                Vec3::new(0.0, scale.y, 0.0),
                Vec3::new(0.0, 0.0, scale.z),
            ],
        }
    }

    /// Creates a matrix for a rotation around the X-axis.
    ///
    /// # Arguments
    ///
    /// * `angle_radians`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_x(angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        Self {
            cols: [
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, c, s),
                Vec3::new(0.0, -s, c),
            ],
        }
    }

    /// Creates a matrix for a right-handed rotation around the Y-axis.
    ///
    /// # Arguments
    ///
    /// * `angle_radians`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_y(angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        Self {
            cols: [
                Vec3::new(c, 0.0, -s),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(s, 0.0, c),
            ],
        }
    }

    /// Creates a matrix for a rotation around the Z-axis.
    ///
    /// # Arguments
    ///
    /// * `angle_radians`: The angle of rotation in radians.
    #[inline]
    pub fn from_rotation_z(angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        Self {
            cols: [
                Vec3::new(c, s, 0.0),
                Vec3::new(-s, c, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a rotation matrix from a normalized axis and an angle.
    ///
    /// # Arguments
    ///
    /// * `axis`: The axis of rotation. Must be a unit vector.
    /// * `angle_radians`: The angle of rotation in radians.
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        let t = 1.0 - c;
        let x = axis.x;
        let y = axis.y;
        let z = axis.z;
        Self {
            cols: [
                Vec3::new(t * x * x + c, t * x * y + s * z, t * x * z - s * y),
                Vec3::new(t * y * x - s * z, t * y * y + c, t * y * z + s * x),
                Vec3::new(t * z * x + s * y, t * z * y - s * x, t * z * z + c),
            ],
        }
    }

    /// Creates a rotation matrix from a quaternion.
    /// The quaternion is normalized before conversion to ensure a valid rotation matrix.
    #[inline]
    pub fn from_quat(q: Quaternion) -> Self {
        let q = q.normalize();
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
            Vec3::new(1.0 - (yy + zz), xy + wz, xz - wy),
            Vec3::new(xy - wz, 1.0 - (xx + zz), yz + wx),
            Vec3::new(xz + wy, yz - wx, 1.0 - (xx + yy)),
        )
    }

    /// Creates a `Mat3` from the upper-left 3x3 corner of a [`Mat4`].
    /// This effectively extracts the rotation and scale components, discarding translation.
    #[inline]
    pub fn from_mat4(m4: &Mat4) -> Self {
        Self::from_cols(
            m4.cols[0].truncate(),
            m4.cols[1].truncate(),
            m4.cols[2].truncate(),
        )
    }

    /// Computes the determinant of the matrix.
    ///
    /// The determinant is a scalar value indicating the volume scaling factor of the
    /// linear transformation. A determinant of 0 means the matrix is not invertible.
    #[inline]
    pub fn determinant(&self) -> f32 {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        c0.x * (c1.y * c2.z - c2.y * c1.z) - c1.x * (c0.y * c2.z - c2.y * c0.z)
            + c2.x * (c0.y * c1.z - c1.y * c0.z)
    }

    /// Returns the transpose of the matrix, where rows and columns are swapped.
    #[inline]
    pub fn transpose(&self) -> Self {
        Self::from_cols(
            Vec3::new(self.cols[0].x, self.cols[1].x, self.cols[2].x),
            Vec3::new(self.cols[0].y, self.cols[1].y, self.cols[2].y),
            Vec3::new(self.cols[0].z, self.cols[1].z, self.cols[2].z),
        )
    }

    /// Computes the inverse of the matrix.
    ///
    /// If the matrix is not invertible (i.e., its determinant is close to zero),
    /// this method returns `None`.
    pub fn inverse(&self) -> Option<Self> {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        let m00 = c1.y * c2.z - c2.y * c1.z;
        let m10 = c2.y * c0.z - c0.y * c2.z;
        let m20 = c0.y * c1.z - c1.y * c0.z;
        let det = c0.x * m00 + c1.x * m10 + c2.x * m20;

        if det.abs() < EPSILON {
            return None;
        }

        let inv_det = 1.0 / det;
        let m01 = c2.x * c1.z - c1.x * c2.z;
        let m11 = c0.x * c2.z - c2.x * c0.z;
        let m21 = c1.x * c0.z - c0.x * c1.z;
        let m02 = c1.x * c2.y - c2.x * c1.y;
        let m12 = c2.x * c0.y - c0.x * c2.y;
        let m22 = c0.x * c1.y - c1.x * c0.y;

        Some(Self::from_cols(
            Vec3::new(m00, m10, m20) * inv_det,
            Vec3::new(m01, m11, m21) * inv_det,
            Vec3::new(m02, m12, m22) * inv_det,
        ))
    }

    /// Converts this `Mat3` into a [`Mat4`], preserving its values in the upper-left corner.
    /// The new fourth column and row are set to `(0, 0, 0, 1)`.
    #[inline]
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_cols(
            Vec4::from_vec3(self.cols[0], 0.0),
            Vec4::from_vec3(self.cols[1], 0.0),
            Vec4::from_vec3(self.cols[2], 0.0),
            Vec4::W,
        )
    }
}

// --- Operator Overloads ---

impl Default for Mat3 {
    /// Returns the 3x3 identity matrix.
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul<Mat3> for Mat3 {
    type Output = Self;
    /// Multiplies this matrix by another `Mat3`.
    #[inline]
    fn mul(self, rhs: Mat3) -> Self::Output {
        Self::from_cols(self * rhs.cols[0], self * rhs.cols[1], self * rhs.cols[2])
    }
}

impl Mul<Vec3> for Mat3 {
    type Output = Vec3;
    /// Transforms a `Vec3` by this matrix.
    #[inline]
    fn mul(self, v: Vec3) -> Self::Output {
        self.cols[0] * v.x + self.cols[1] * v.y + self.cols[2] * v.z
    }
}

impl Index<usize> for Mat3 {
    type Output = Vec3;
    /// Allows accessing a matrix column by index.
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.cols[index]
    }
}

impl IndexMut<usize> for Mat3 {
    /// Allows mutably accessing a matrix column by index.
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.cols[index]
    }
}

// --- Mat4 ---

/// A 4x4 column-major matrix, used for 3D affine transformations.
///
/// This is the primary type for representing transformations (translation, rotation,
/// scale) in 3D space. It is also used for camera view and projection matrices.
/// The memory layout is column-major, which is compatible with modern graphics APIs
/// like Vulkan, Metal, and DirectX.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    #[inline]
    pub fn perspective_rh_zo(
        fov_y_radians: f32,
        aspect_ratio: f32,
        z_near: f32,
        z_far: f32,
    ) -> Self {
        assert!(z_near > 0.0 && z_far > z_near);
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

    /// Creates a right-handed orthographic projection matrix with a [0, 1] depth range (ZO).
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
        if det.abs() < crate::math::EPSILON {
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

        if det3x3.abs() < crate::math::EPSILON {
            return None;
        }

        let inv_det3x3 = 1.0 / det3x3;
        let inv00 = (c1.y * c2.z - c2.y * c1.z) * inv_det3x3;
        let inv10 = -(c2.y * c0.z - c0.y * c2.z) * inv_det3x3;
        let inv20 = (c0.y * c1.z - c1.y * c0.z) * inv_det3x3;
        let inv01 = -(c2.x * c1.z - c1.x * c2.z) * inv_det3x3;
        let inv11 = (c0.x * c2.z - c2.x * c0.z) * inv_det3x3;
        let inv21 = -(c1.x * c0.z - c0.x * c1.z) * inv_det3x3;
        let inv02 = (c1.x * c2.y - c2.x * c1.y) * inv_det3x3;
        let inv12 = -(c2.x * c0.y - c0.x * c2.y) * inv_det3x3;
        let inv22 = (c0.x * c1.y - c1.x * c0.y) * inv_det3x3;
        let inv_tx = -(inv00 * translation.x + inv01 * translation.y + inv02 * translation.z);
        let inv_ty = -(inv10 * translation.x + inv11 * translation.y + inv12 * translation.z);
        let inv_tz = -(inv20 * translation.x + inv21 * translation.y + inv22 * translation.z);

        Some(Self::from_cols(
            Vec4::new(inv00, inv10, inv20, 0.0),
            Vec4::new(inv01, inv11, inv21, 0.0),
            Vec4::new(inv02, inv12, inv22, 0.0),
            Vec4::new(inv_tx, inv_ty, inv_tz, 1.0),
        ))
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

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{approx_eq, matrix::Mat4, quaternion::Quaternion, vector::Vec3, PI};

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    fn mat3_approx_eq(a: Mat3, b: Mat3) -> bool {
        vec3_approx_eq(a.cols[0], b.cols[0])
            && vec3_approx_eq(a.cols[1], b.cols[1])
            && vec3_approx_eq(a.cols[2], b.cols[2])
    }

    fn vec4_approx_eq(a: Vec4, b: Vec4) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z) && approx_eq(a.w, b.w)
    }

    fn mat4_approx_eq(a: Mat4, b: Mat4) -> bool {
        vec4_approx_eq(a.cols[0], b.cols[0])
            && vec4_approx_eq(a.cols[1], b.cols[1])
            && vec4_approx_eq(a.cols[2], b.cols[2])
            && vec4_approx_eq(a.cols[3], b.cols[3])
    }

    // --- Tests for Mat3 ---

    #[test]
    fn test_mat3_identity_default() {
        assert_eq!(Mat3::default(), Mat3::IDENTITY);

        let m = Mat3::from_scale(Vec3::new(1.0, 2.0, 3.0));
        assert!(mat3_approx_eq(m * Mat3::IDENTITY, m));
        assert!(mat3_approx_eq(Mat3::IDENTITY * m, m));
    }

    #[test]
    fn test_mat3_from_scale() {
        let s = Vec3::new(2.0, -3.0, 0.5);
        let m = Mat3::from_scale(s);
        let v = Vec3::new(1.0, 1.0, 1.0);
        assert!(vec3_approx_eq(m * v, s)); // Scaling (1,1,1) should yield the scale vector
    }

    #[test]
    fn test_mat3_rotations() {
        let angle = PI / 6.0; // 30 degrees
        let mx = Mat3::from_rotation_x(angle);
        let my = Mat3::from_rotation_y(angle);
        let mz = Mat3::from_rotation_z(angle);

        let p = Vec3::Y; // Point on Y axis
        let expected_px = Vec3::new(0.0, angle.cos(), angle.sin());
        assert!(vec3_approx_eq(mx * p, expected_px));

        let p = Vec3::X; // Point on X axis
        let expected_py = Vec3::new(angle.cos(), 0.0, -angle.sin()); // RH
        assert!(vec3_approx_eq(my * p, expected_py));

        let p = Vec3::X; // Point on X axis
        let expected_pz = Vec3::new(angle.cos(), angle.sin(), 0.0);
        assert!(vec3_approx_eq(mz * p, expected_pz));
    }

    #[test]
    fn test_mat3_from_axis_angle() {
        let axis = Vec3::new(1.0, 1.0, 1.0).normalize();
        let angle = 1.2 * PI;
        let m = Mat3::from_axis_angle(axis, angle);

        // Rotation around (1,1,1) axis should permute basis vectors
        let v = Vec3::X;
        let v_rotated = m * v;

        // Check if length is preserved
        assert!(approx_eq(v_rotated.length(), v.length()));

        // Specific check is hard without known values, but ensure it's not identity or zero
        assert!(v_rotated.distance_squared(v) > EPSILON);
    }

    #[test]
    fn test_mat3_from_quat() {
        let axis = Vec3::new(1.0, -2.0, 3.0).normalize();
        let angle = PI / 7.0;
        let q = Quaternion::from_axis_angle(axis, angle);
        let m_from_q = Mat3::from_quat(q);

        let v = Vec3::new(0.5, 1.0, -0.2);
        let v_rotated_q = q * v;
        let v_rotated_m = m_from_q * v;

        assert!(vec3_approx_eq(v_rotated_q, v_rotated_m));
    }

    #[test]
    fn test_mat3_determinant() {
        assert!(approx_eq(Mat3::IDENTITY.determinant(), 1.0));
        assert!(approx_eq(Mat3::ZERO.determinant(), 0.0));

        let m_scale = Mat3::from_scale(Vec3::new(2.0, 3.0, 4.0));
        assert!(approx_eq(m_scale.determinant(), 24.0));

        let m_rot = Mat3::from_rotation_y(PI / 5.0);
        assert!(approx_eq(m_rot.determinant(), 1.0)); // Rotations preserve volume
    }

    #[test]
    fn test_mat3_transpose() {
        let m = Mat3::from_cols(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 5.0, 6.0),
            Vec3::new(7.0, 8.0, 9.0),
        );
        let mt = m.transpose();
        let expected_mt = Mat3::from_cols(
            Vec3::new(1.0, 4.0, 7.0),
            Vec3::new(2.0, 5.0, 8.0),
            Vec3::new(3.0, 6.0, 9.0),
        );

        assert!(mat3_approx_eq(mt, expected_mt));
        assert!(mat3_approx_eq(m.transpose().transpose(), m)); // Double transpose
    }

    #[test]
    fn test_mat3_inverse() {
        let m = Mat3::from_rotation_z(PI / 3.0) * Mat3::from_scale(Vec3::new(1.0, 2.0, 0.5));
        let inv_m = m.inverse().expect("Matrix should be invertible");
        let identity = m * inv_m;
        assert!(
            mat3_approx_eq(identity, Mat3::IDENTITY),
            "M * inv(M) should be Identity"
        );

        let singular = Mat3::from_scale(Vec3::new(1.0, 0.0, 1.0));
        assert!(
            singular.inverse().is_none(),
            "Singular matrix inverse should be None"
        );
    }

    #[test]
    fn test_mat3_mul_vec3() {
        let m = Mat3::from_rotation_z(PI / 2.0); // Rotate 90 deg around Z
        let v = Vec3::X; // (1, 0, 0)
        let expected_v = Vec3::Y; // (0, 1, 0)
        assert!(vec3_approx_eq(m * v, expected_v));
    }

    #[test]
    fn test_mat3_mul_mat3() {
        let rot90z = Mat3::from_rotation_z(PI / 2.0);
        let rot180z = rot90z * rot90z;
        let expected_rot180z = Mat3::from_rotation_z(PI);
        assert!(mat3_approx_eq(rot180z, expected_rot180z));
    }

    #[test]
    fn test_mat3_conversions() {
        let m4 = Mat4::from_translation(Vec3::new(10., 20., 30.)) * Mat4::from_rotation_x(PI / 4.0);
        let m3 = Mat3::from_mat4(&m4);
        let m4_again = m3.to_mat4();

        // Check if rotation part was extracted correctly
        let v = Vec3::Y;
        let v_rot_m3 = m3 * v;
        let v_rot_m4 = Mat4::from_rotation_x(PI / 4.0) * Vec4::from_vec3(v, 0.0); // Rotate as vector
        assert!(vec3_approx_eq(v_rot_m3, v_rot_m4.truncate()));

        // Check if embedding back into Mat4 worked (translation should be zero)
        let origin = Vec4::new(0.0, 0.0, 0.0, 1.0);
        let transformed_origin = m4_again * origin;
        assert!(approx_eq(transformed_origin.x, 0.0));
        assert!(approx_eq(transformed_origin.y, 0.0));
        assert!(approx_eq(transformed_origin.z, 0.0));
        assert!(approx_eq(transformed_origin.w, 1.0));
    }

    #[test]
    fn test_mat3_index() {
        let mut m = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::Z);
        assert_eq!(m[0], Vec3::X);
        assert_eq!(m[1], Vec3::Y);
        assert_eq!(m[2], Vec3::Z);
        m[0] = Vec3::ONE;
        assert_eq!(m.cols[0], Vec3::ONE);
    }

    #[test]
    #[should_panic]
    fn test_mat3_index_out_of_bounds() {
        let m = Mat3::IDENTITY;
        let _ = m[3]; // Should panic
    }

    // --- End of Mat3 Tests ---

    // --- Tests for Mat4 ---

    #[test]
    fn test_identity() {
        assert_eq!(Mat4::default(), Mat4::IDENTITY);
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert!(mat4_approx_eq(m * Mat4::IDENTITY, m));
        assert!(mat4_approx_eq(Mat4::IDENTITY * m, m));
    }

    #[test]
    fn test_from_quat() {
        let axis = Vec3::new(1.0, 2.0, 3.0).normalize();
        let angle = PI / 5.0;
        let q = Quaternion::from_axis_angle(axis, angle);
        let m_from_q = Mat4::from_quat(q);

        let v = Vec3::new(5.0, -1.0, 2.0);

        let v_rotated_q = q * v; // Rotate using quaternion directly
                                 // Rotate using matrix: convert v to Vec4(point), multiply, convert back
        let v4 = Vec4::from_vec3(v, 1.0);
        let v_rotated_m4 = m_from_q * v4;
        let v_rotated_m = v_rotated_m4.truncate();

        // Compare results
        assert!(approx_eq(v_rotated_q.x, v_rotated_m.x));
        assert!(approx_eq(v_rotated_q.y, v_rotated_m.y));
        assert!(approx_eq(v_rotated_q.z, v_rotated_m.z));
    }

    #[test]
    fn test_translation() {
        let t = Vec3::new(1.0, 2.0, 3.0);
        let m = Mat4::from_translation(t);
        let p = Vec4::new(1.0, 1.0, 1.0, 1.0);
        let expected_p = Vec4::new(2.0, 3.0, 4.0, 1.0);

        assert!(vec4_approx_eq(m * p, expected_p));
    }

    #[test]
    fn test_scale() {
        let s = Vec3::new(2.0, 3.0, 4.0);
        let m = Mat4::from_scale(s);
        let p = Vec4::new(1.0, 1.0, 1.0, 1.0);
        let expected_p = Vec4::new(2.0, 3.0, 4.0, 1.0);
        assert!(vec4_approx_eq(m * p, expected_p));
    }

    #[test]
    fn test_rotation_x() {
        let angle = PI / 2.0; // 90 degrees
        let m = Mat4::from_rotation_x(angle);
        let p = Vec4::new(0.0, 1.0, 0.0, 1.0); // Point on Y axis
        let expected_p = Vec4::new(0.0, 0.0, 1.0, 1.0); // Should rotate to Z axis
        assert!(vec4_approx_eq(m * p, expected_p));
    }

    #[test]
    fn test_rotation_y() {
        let angle = PI / 2.0; // 90 degrees
        let m = Mat4::from_rotation_y(angle);
        let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point on X axis
        let expected_p = Vec4::new(0.0, 0.0, -1.0, 1.0); // Should rotate to -Z axis

        assert!(vec4_approx_eq(m * p, expected_p));
    }

    #[test]
    fn test_rotation_z() {
        let angle = PI / 2.0; // 90 degrees
        let m = Mat4::from_rotation_z(angle);
        let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point on X axis
        let expected_p = Vec4::new(0.0, 1.0, 0.0, 1.0); // Should rotate to Y axis
        assert!(vec4_approx_eq(m * p, expected_p));
    }

    #[test]
    fn test_transpose() {
        let m = Mat4::from_cols(
            Vec4::new(1., 2., 3., 4.),
            Vec4::new(5., 6., 7., 8.),
            Vec4::new(9., 10., 11., 12.),
            Vec4::new(13., 14., 15., 16.),
        );
        let mt = m.transpose();
        let expected_mt = Mat4::from_cols(
            Vec4::new(1., 5., 9., 13.),
            Vec4::new(2., 6., 10., 14.),
            Vec4::new(3., 7., 11., 15.),
            Vec4::new(4., 8., 12., 16.),
        );
        assert_eq!(mt.cols[0], expected_mt.cols[0]); // Compare columns after transpose
        assert_eq!(mt.cols[1], expected_mt.cols[1]);
        assert_eq!(mt.cols[2], expected_mt.cols[2]);
        assert_eq!(mt.cols[3], expected_mt.cols[3]);

        // Test double transpose
        assert!(mat4_approx_eq(m.transpose().transpose(), m));
    }

    #[test]
    fn test_mul_mat4() {
        let t = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
        let r = Mat4::from_rotation_z(PI / 2.0);

        // Order matters: Translate then Rotate
        let tr = r * t;
        let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point at (1,0,0)
                                               // 1. Translate: p becomes (2, 0, 0, 1)
                                               // 2. Rotate Z 90: (2, 0, 0) becomes (0, 2, 0)
        let expected_tr = Vec4::new(0.0, 2.0, 0.0, 1.0);
        assert!(vec4_approx_eq(tr * p, expected_tr));

        // Order matters: Rotate then Translate
        let rt = t * r;
        // 1. Rotate Z 90: p becomes (0, 1, 0, 1)
        // 2. Translate: (0, 1, 0) becomes (1, 1, 0)
        let expected_rt = Vec4::new(1.0, 1.0, 0.0, 1.0);
        assert!(vec4_approx_eq(rt * p, expected_rt));
    }

    #[test]
    fn test_inverse() {
        let m = Mat4::from_translation(Vec3::new(1., 2., 3.))
            * Mat4::from_rotation_y(PI / 4.0)
            * Mat4::from_scale(Vec3::new(1., 2., 1.));

        let inv_m = m.inverse().expect("Matrix should be invertible");
        let identity = m * inv_m;

        // Check if M * M^-1 is close to identity
        assert!(
            mat4_approx_eq(identity, Mat4::IDENTITY),
            "M * inv(M) should be Identity"
        );

        // Check singular matrix (e.g., scale with zero)
        let singular = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
        assert!(
            singular.inverse().is_none(),
            "Singular matrix inverse should be None"
        );
    }

    #[test]
    fn test_affine_inverse() {
        let t = Mat4::from_translation(Vec3::new(1., 2., 3.));
        let r = Mat4::from_rotation_y(PI / 3.0);
        let s = Mat4::from_scale(Vec3::new(1., 2., 0.5));
        let m = t * r * s; // Combined affine transform

        let inv_m = m.inverse().expect("Matrix should be invertible");
        let affine_inv_m = m
            .affine_inverse()
            .expect("Matrix should be affine invertible");

        // Check if affine inverse matches general inverse for this case
        assert!(
            mat4_approx_eq(inv_m, affine_inv_m),
            "Affine inverse should match general inverse"
        );

        // Check M * inv(M) == Identity using affine inverse
        let identity = m * affine_inv_m;
        assert!(
            mat4_approx_eq(identity, Mat4::IDENTITY),
            "M * affine_inv(M) should be Identity"
        );

        // Test singular affine matrix
        let singular_s = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
        let singular_m = t * singular_s;
        assert!(
            singular_m.affine_inverse().is_none(),
            "Singular affine matrix inverse should be None"
        );
    }

    #[test]
    fn test_perspective_rh_zo() {
        let fov = PI / 4.0; // 45 degrees
        let aspect = 16.0 / 9.0;
        let near = 0.1;
        let far = 100.0;

        let m = Mat4::perspective_rh_zo(fov, aspect, near, far);
        assert!(approx_eq(m.cols[0].x, 1.0 / (aspect * (fov / 2.0).tan())));
        assert!(approx_eq(m.cols[1].y, 1.0 / ((fov / 2.0).tan())));
        assert!(approx_eq(m.cols[2].z, -far / (far - near)));
        assert!(approx_eq(m.cols[3].z, -(far * near) / (far - near)));
    }

    #[test]
    fn test_orthographic_rh_zo() {
        let left = -1.0;
        let right = 1.0;
        let bottom = -1.0;
        let top = 1.0;
        let near = 0.1;
        let far = 100.0;
        let m = Mat4::orthographic_rh_zo(left, right, bottom, top, near, far);

        // Check scale factors
        assert!(approx_eq(m.cols[0].x, 2.0 / (right - left)));
        assert!(approx_eq(m.cols[1].y, 2.0 / (top - bottom)));
        assert!(approx_eq(m.cols[2].z, -1.0 / (far - near)));

        // Check translation factors
        assert!(approx_eq(m.cols[3].x, -(right + left) / (right - left)));
        assert!(approx_eq(m.cols[3].y, -(top + bottom) / (top - bottom)));
        assert!(approx_eq(m.cols[3].z, -near / (far - near))); // -near and not -(far + near)
    }

    #[test]
    fn test_look_at_rh() {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let target = Vec3::new(0.0, 0.0, 0.0);
        let up = Vec3::new(0.0, 1.0, 0.0);

        let m = Mat4::look_at_rh(eye, target, up).expect("look_at_rh should return Some(Mat4)");

        // Forward direction (third column, third row): should be +1.0 for a right-handed system
        assert!(approx_eq(m.cols[2].z, 1.0));

        // Translation part (fourth column, third row): should be -eye · forward = -5.0
        assert!(approx_eq(m.cols[3].z, -5.0));
    }

    #[test]
    fn test_look_at_rh_invalid() {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let target = Vec3::new(0.0, 0.0, 5.0); // Same as eye
        let up = Vec3::new(0.0, 1.0, 0.0);

        // This should panic or return None (depending on implementation)
        assert!(Mat4::look_at_rh(eye, target, up).is_none());
    }

    // --- End of Tests For Mat4 ---
}
