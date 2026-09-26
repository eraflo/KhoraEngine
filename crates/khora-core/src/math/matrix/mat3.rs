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

//! A 3x3 column-major matrix.

use super::mat4::Mat4;
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
