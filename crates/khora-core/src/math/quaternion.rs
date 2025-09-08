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

//! Provides a Quaternion type for representing 3D rotations.

use serde::{Deserialize, Serialize};

use super::{Mat4, Vec3, EPSILON};
use std::ops::{Add, Mul, MulAssign, Neg, Sub};

/// Represents a quaternion for efficient 3D rotations.
///
/// Quaternions are a four-dimensional complex number system that can represent
/// rotations in 3D space. They are generally more efficient and numerically stable
/// than rotation matrices, avoiding issues like gimbal lock.
///
/// A quaternion is stored as `(x, y, z, w)`, where `[x, y, z]` is the "vector" part
/// and `w` is the "scalar" part. For representing rotations, it should be a "unit
/// quaternion" where `x² + y² + z² + w² = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct Quaternion {
    /// The x component of the vector part.
    pub x: f32,
    /// The y component of the vector part.
    pub y: f32,
    /// The z component of the vector part.
    pub z: f32,
    /// The scalar (real) part.
    pub w: f32,
}

impl Quaternion {
    /// The identity quaternion, representing no rotation.
    pub const IDENTITY: Quaternion = Quaternion {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Creates a new quaternion from its raw components.
    ///
    /// Note: This does not guarantee a unit quaternion. For creating rotations,
    /// prefer using `from_axis_angle` or other rotation-specific constructors.
    #[inline]
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Creates a quaternion representing a rotation around a given axis by a given angle.
    ///
    /// # Arguments
    ///
    /// * `axis`: The axis of rotation. It is recommended to pass a normalized vector.
    /// * `angle_radians`: The angle of rotation in radians.
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle_radians: f32) -> Self {
        let normalized_axis = axis.normalize();
        let half_angle = angle_radians * 0.5;
        let s = half_angle.sin();
        let c = half_angle.cos();
        Self {
            x: normalized_axis.x * s,
            y: normalized_axis.y * s,
            z: normalized_axis.z * s,
            w: c,
        }
    }

    /// Creates a quaternion from a 4x4 rotation matrix.
    ///
    /// This method only considers the upper 3x3 part of the matrix for the conversion.
    #[inline]
    pub fn from_rotation_matrix(m: &Mat4) -> Self {
        let m00 = m.cols[0].x;
        let m10 = m.cols[0].y;
        let m20 = m.cols[0].z;
        let m01 = m.cols[1].x;
        let m11 = m.cols[1].y;
        let m21 = m.cols[1].z;
        let m02 = m.cols[2].x;
        let m12 = m.cols[2].y;
        let m22 = m.cols[2].z;

        // Algorithm from http://www.euclideanspace.com/maths/geometry/rotations/conversions/matrixToQuaternion/index.htm
        let trace = m00 + m11 + m22;
        let mut q = Self::IDENTITY;

        if trace > 0.0 {
            let s = 2.0 * (trace + 1.0).sqrt();
            q.w = 0.25 * s;
            q.x = (m21 - m12) / s;
            q.y = (m02 - m20) / s;
            q.z = (m10 - m01) / s;
        } else if m00 > m11 && m00 > m22 {
            let s = 2.0 * (1.0 + m00 - m11 - m22).sqrt();
            q.w = (m21 - m12) / s;
            q.x = 0.25 * s;
            q.y = (m01 + m10) / s;
            q.z = (m02 + m20) / s;
        } else if m11 > m22 {
            let s = 2.0 * (1.0 + m11 - m00 - m22).sqrt();
            q.w = (m02 - m20) / s;
            q.x = (m01 + m10) / s;
            q.y = 0.25 * s;
            q.z = (m12 + m21) / s;
        } else {
            let s = 2.0 * (1.0 + m22 - m00 - m11).sqrt();
            q.w = (m10 - m01) / s;
            q.x = (m02 + m20) / s;
            q.y = (m12 + m21) / s;
            q.z = 0.25 * s;
        }
        q.normalize()
    }

    /// Calculates the squared length (magnitude) of the quaternion.
    #[inline]
    pub fn magnitude_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Calculates the length (magnitude) of the quaternion.
    #[inline]
    pub fn magnitude(&self) -> f32 {
        self.magnitude_squared().sqrt()
    }

    /// Returns a normalized version of the quaternion with a length of 1.
    /// If the quaternion has a near-zero magnitude, it returns the identity quaternion.
    pub fn normalize(&self) -> Self {
        let mag_sqrt = self.magnitude_squared();
        if mag_sqrt > EPSILON {
            let inv_mag = 1.0 / mag_sqrt.sqrt();
            Self {
                x: self.x * inv_mag,
                y: self.y * inv_mag,
                z: self.z * inv_mag,
                w: self.w * inv_mag,
            }
        } else {
            Self::IDENTITY
        }
    }

    /// Computes the conjugate of the quaternion, which negates the vector part.
    #[inline]
    pub fn conjugate(&self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    /// Computes the inverse of the quaternion.
    /// For a unit quaternion, the inverse is equal to its conjugate.
    #[inline]
    pub fn inverse(&self) -> Self {
        let mag_squared = self.magnitude_squared();
        if mag_squared > EPSILON {
            self.conjugate() * (1.0 / mag_squared)
        } else {
            Self::IDENTITY
        }
    }

    /// Computes the dot product of two quaternions.
    #[inline]
    pub fn dot(&self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Rotates a 3D vector by this quaternion.
    pub fn rotate_vec3(&self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let s: f32 = self.w;
        2.0 * u.dot(v) * u + (s * s - u.dot(u)) * v + 2.0 * s * u.cross(v)
    }

    /// Performs a Spherical Linear Interpolation (Slerp) between two quaternions.
    ///
    /// Slerp provides a smooth, constant-speed interpolation between two rotations,
    /// following the shortest path on the surface of a 4D sphere.
    ///
    /// *   `t` - The interpolation factor, clamped to the `[0.0, 1.0]` range.
    pub fn slerp(start: Self, end: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mut cos_theta = start.dot(end);
        let mut end_adjusted = end;

        // If the dot product is negative, the quaternions are more than 90 degrees apart.
        // To ensure the shortest path, negate one quaternion.
        // This is equivalent to using the conjugate of the quaternion.
        if cos_theta < 0.0 {
            cos_theta = -cos_theta;
            end_adjusted = -end;
        }

        if cos_theta > 1.0 - EPSILON {
            // Linear Interpolation: (1-t)*start + t*end_adjusted
            // Normalize the result to avoid drift due to floating point errors.
            let result = (start * (1.0 - t)) + (end_adjusted * t);
            result.normalize()
        } else {
            let angle = cos_theta.acos();
            let sin_theta_inv = 1.0 / angle.sin();
            let scale_start = ((1.0 - t) * angle).sin() * sin_theta_inv;
            let scale_end = (t * angle).sin() * sin_theta_inv;
            (start * scale_start) + (end_adjusted * scale_end)
        }
    }
}

// --- Operator Overloads ---

impl Default for Quaternion {
    /// Returns the identity quaternion, representing no rotation.
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul<Quaternion> for Quaternion {
    type Output = Self;
    /// Combines two rotations using the Hamilton product.
    /// Note that quaternion multiplication is not commutative.
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        }
    }
}

impl MulAssign<Quaternion> for Quaternion {
    /// Combines this rotation with another.
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl Mul<Vec3> for Quaternion {
    type Output = Vec3;
    /// Rotates a `Vec3` by this quaternion.
    #[inline]
    fn mul(self, rhs: Vec3) -> Self::Output {
        self.normalize().rotate_vec3(rhs)
    }
}

impl Add<Quaternion> for Quaternion {
    type Output = Self;
    /// Adds two quaternions component-wise.
    /// Note: This is not a standard rotation operation.
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
            w: self.w + rhs.w,
        }
    }
}

impl Sub<Quaternion> for Quaternion {
    type Output = Self;
    /// Subtracts two quaternions component-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
            w: self.w - rhs.w,
        }
    }
}

impl Mul<f32> for Quaternion {
    type Output = Self;
    /// Scales all components of the quaternion by a scalar.
    #[inline]
    fn mul(self, scalar: f32) -> Self::Output {
        Self {
            x: self.x * scalar,
            y: self.y * scalar,
            z: self.z * scalar,
            w: self.w * scalar,
        }
    }
}

impl Neg for Quaternion {
    type Output = Self;
    /// Negates all components of the quaternion.
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: -self.w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // Import Quaternion, EPSILON etc. from parent module
    use crate::math::vector::Vec4;
    use approx::assert_relative_eq; // For float comparisons

    fn quat_approx_eq(q1: Quaternion, q2: Quaternion) -> bool {
        let dot = q1.dot(q2).abs();
        approx::relative_eq!(dot, 1.0, epsilon = EPSILON * 10.0) // Use abs dot product
    }

    #[test]
    fn test_identity_and_default() {
        let q_ident = Quaternion::IDENTITY;
        let q_def = Quaternion::default();
        assert_eq!(q_ident, q_def);
        assert_relative_eq!(q_ident.x, 0.0);
        assert_relative_eq!(q_ident.y, 0.0);
        assert_relative_eq!(q_ident.z, 0.0);
        assert_relative_eq!(q_ident.w, 1.0);
        assert_relative_eq!(q_ident.magnitude(), 1.0, epsilon = EPSILON);
    }

    #[test]
    fn test_from_axis_angle() {
        let axis = Vec3::Y;
        let angle = std::f32::consts::FRAC_PI_2; // 90 degrees
        let q = Quaternion::from_axis_angle(axis, angle);

        let half_angle = angle * 0.5;
        let expected_s = half_angle.sin();
        let expected_c = half_angle.cos();

        assert_relative_eq!(q.x, 0.0 * expected_s, epsilon = EPSILON);
        assert_relative_eq!(q.y, 1.0 * expected_s, epsilon = EPSILON);
        assert_relative_eq!(q.z, 0.0 * expected_s, epsilon = EPSILON);
        assert_relative_eq!(q.w, expected_c, epsilon = EPSILON);
        assert_relative_eq!(q.magnitude(), 1.0, epsilon = EPSILON);
    }

    #[test]
    fn test_from_axis_angle_normalizes_axis() {
        let axis = Vec3::new(0.0, 5.0, 0.0); // Non-unit axis
        let angle = std::f32::consts::FRAC_PI_2;
        let q = Quaternion::from_axis_angle(axis, angle);

        let half_angle = angle * 0.5;
        let expected_s = half_angle.sin();
        let expected_c = half_angle.cos();

        assert_relative_eq!(q.x, 0.0 * expected_s, epsilon = EPSILON);
        assert_relative_eq!(q.y, 1.0 * expected_s, epsilon = EPSILON); // Normalized axis y=1.0
        assert_relative_eq!(q.z, 0.0 * expected_s, epsilon = EPSILON);
        assert_relative_eq!(q.w, expected_c, epsilon = EPSILON);
        assert_relative_eq!(q.magnitude(), 1.0, epsilon = EPSILON);
    }

    #[test]
    fn test_from_rotation_matrix_identity() {
        let m = Mat4::IDENTITY;
        let q = Quaternion::from_rotation_matrix(&m);
        assert!(quat_approx_eq(q, Quaternion::IDENTITY));
    }

    #[test]
    fn test_from_rotation_matrix_simple_rotations() {
        let angle = std::f32::consts::FRAC_PI_4; // 45 degrees

        // Rotation X
        let mx = Mat4::from_rotation_x(angle);
        let qx_expected = Quaternion::from_axis_angle(Vec3::X, angle);
        let qx_from_m = Quaternion::from_rotation_matrix(&mx);
        assert!(quat_approx_eq(qx_from_m, qx_expected));

        // Rotation Y
        let my = Mat4::from_rotation_y(angle);
        let qy_expected = Quaternion::from_axis_angle(Vec3::Y, angle);
        let qy_from_m = Quaternion::from_rotation_matrix(&my);
        assert!(quat_approx_eq(qy_from_m, qy_expected));

        // Rotation Z
        let mz = Mat4::from_rotation_z(angle);
        let qz_expected = Quaternion::from_axis_angle(Vec3::Z, angle);
        let qz_from_m = Quaternion::from_rotation_matrix(&mz);
        assert!(quat_approx_eq(qz_from_m, qz_expected));
    }

    #[test]
    fn test_matrix_to_quat_and_back() {
        let axis = Vec3::new(-1.0, 2.5, 0.7).normalize();
        let angle = 1.85; // Some arbitrary angle

        let q_orig = Quaternion::from_axis_angle(axis, angle);
        let m_from_q = Mat4::from_quat(q_orig);

        let q_from_m = Quaternion::from_rotation_matrix(&m_from_q);
        let m_from_q_again = Mat4::from_quat(q_from_m);

        // Compare original quaternion to the one extracted from matrix
        assert!(quat_approx_eq(q_orig, q_from_m));

        // Compare original matrix to the one rebuilt from the extracted quaternion
        // This requires mat4_approx_eq from matrix tests
        // assert!(mat4_approx_eq(m_from_q, m_from_q_again)); // Assuming mat4_approx_eq exists
        // For now, just check rotation behaviour
        let v = Vec3::new(1.0, 1.0, 1.0);
        let v_rot_orig = m_from_q * Vec4::from_vec3(v, 1.0);
        let v_rot_new = m_from_q_again * Vec4::from_vec3(v, 1.0);
        assert_relative_eq!(v_rot_orig.x, v_rot_new.x, epsilon = EPSILON);
        assert_relative_eq!(v_rot_orig.y, v_rot_new.y, epsilon = EPSILON);
        assert_relative_eq!(v_rot_orig.z, v_rot_new.z, epsilon = EPSILON);
    }

    #[test]
    fn test_conjugate_and_inverse_unit() {
        let axis = Vec3::new(1.0, 2.0, 3.0).normalize();
        let angle = 0.75;
        let q = Quaternion::from_axis_angle(axis, angle);
        let q_conj = q.conjugate();
        let q_inv = q.inverse();

        assert_relative_eq!(q_conj.x, q_inv.x, epsilon = EPSILON);
        assert_relative_eq!(q_conj.y, q_inv.y, epsilon = EPSILON);
        assert_relative_eq!(q_conj.z, q_inv.z, epsilon = EPSILON);
        assert_relative_eq!(q_conj.w, q_inv.w, epsilon = EPSILON);

        assert_relative_eq!(q_conj.x, -q.x, epsilon = EPSILON);
        assert_relative_eq!(q_conj.y, -q.y, epsilon = EPSILON);
        assert_relative_eq!(q_conj.z, -q.z, epsilon = EPSILON);
        assert_relative_eq!(q_conj.w, q.w, epsilon = EPSILON);
    }

    #[test]
    fn test_multiplication_identity() {
        let axis = Vec3::Y;
        let angle = std::f32::consts::FRAC_PI_2;
        let q = Quaternion::from_axis_angle(axis, angle);

        let res_qi = q * Quaternion::IDENTITY;
        let res_iq = Quaternion::IDENTITY * q;

        assert_relative_eq!(res_qi.x, q.x, epsilon = EPSILON);
        assert_relative_eq!(res_qi.y, q.y, epsilon = EPSILON);
        assert_relative_eq!(res_qi.z, q.z, epsilon = EPSILON);
        assert_relative_eq!(res_qi.w, q.w, epsilon = EPSILON);

        assert_relative_eq!(res_iq.x, q.x, epsilon = EPSILON);
        assert_relative_eq!(res_iq.y, q.y, epsilon = EPSILON);
        assert_relative_eq!(res_iq.z, q.z, epsilon = EPSILON);
        assert_relative_eq!(res_iq.w, q.w, epsilon = EPSILON);
    }

    #[test]
    fn test_multiplication_composition() {
        let rot_y = Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        let rot_x = Quaternion::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_2);
        let combined_rot = rot_x * rot_y; // Y then X

        let v_start = Vec3::Z;
        let v_after_y = rot_y * v_start;
        let v_after_x_then_y = rot_x * v_after_y;
        let v_combined = combined_rot * v_start;

        assert_relative_eq!(v_after_x_then_y.x, 1.0, epsilon = EPSILON);
        assert_relative_eq!(v_after_x_then_y.y, 0.0, epsilon = EPSILON);
        assert_relative_eq!(v_after_x_then_y.z, 0.0, epsilon = EPSILON);

        assert_relative_eq!(v_combined.x, v_after_x_then_y.x, epsilon = EPSILON);
        assert_relative_eq!(v_combined.y, v_after_x_then_y.y, epsilon = EPSILON);
        assert_relative_eq!(v_combined.z, v_after_x_then_y.z, epsilon = EPSILON);
    }

    #[test]
    fn test_multiplication_inverse() {
        let axis = Vec3::new(1.0, -2.0, 0.5).normalize();
        let angle = 1.2;
        let q = Quaternion::from_axis_angle(axis, angle);
        let q_inv = q.inverse();

        let result_forward = q * q_inv;
        let result_backward = q_inv * q;

        assert_relative_eq!(result_forward.x, Quaternion::IDENTITY.x, epsilon = EPSILON);
        assert_relative_eq!(result_forward.y, Quaternion::IDENTITY.y, epsilon = EPSILON);
        assert_relative_eq!(result_forward.z, Quaternion::IDENTITY.z, epsilon = EPSILON);
        assert_relative_eq!(result_forward.w, Quaternion::IDENTITY.w, epsilon = EPSILON);

        assert_relative_eq!(result_backward.x, Quaternion::IDENTITY.x, epsilon = EPSILON);
        assert_relative_eq!(result_backward.y, Quaternion::IDENTITY.y, epsilon = EPSILON);
        assert_relative_eq!(result_backward.z, Quaternion::IDENTITY.z, epsilon = EPSILON);
        assert_relative_eq!(result_backward.w, Quaternion::IDENTITY.w, epsilon = EPSILON);
    }

    #[test]
    fn test_rotate_vec3_and_operator() {
        let axis = Vec3::Y;
        let angle = std::f32::consts::FRAC_PI_2;
        let q = Quaternion::from_axis_angle(axis, angle);

        let v_in = Vec3::X;
        let v_out_method = q.rotate_vec3(v_in);
        let v_out_operator = q * v_in;
        let v_expected = Vec3::new(0.0, 0.0, -1.0);

        assert_relative_eq!(v_out_method.x, v_expected.x, epsilon = EPSILON);
        assert_relative_eq!(v_out_method.y, v_expected.y, epsilon = EPSILON);
        assert_relative_eq!(v_out_method.z, v_expected.z, epsilon = EPSILON);

        assert_relative_eq!(v_out_operator.x, v_expected.x, epsilon = EPSILON);
        assert_relative_eq!(v_out_operator.y, v_expected.y, epsilon = EPSILON);
        assert_relative_eq!(v_out_operator.z, v_expected.z, epsilon = EPSILON);
    }

    #[test]
    fn test_normalization() {
        let q_non_unit = Quaternion::new(1.0, 2.0, 3.0, 4.0);
        let q_norm = q_non_unit.normalize();
        assert_relative_eq!(q_norm.magnitude(), 1.0, epsilon = EPSILON);

        let q_mut = q_non_unit;
        let q_mut = q_mut.normalize();
        assert_relative_eq!(q_mut.magnitude(), 1.0, epsilon = EPSILON);

        assert_relative_eq!(q_mut.x, q_norm.x, epsilon = EPSILON);
        assert_relative_eq!(q_mut.y, q_norm.y, epsilon = EPSILON);
        assert_relative_eq!(q_mut.z, q_norm.z, epsilon = EPSILON);
        assert_relative_eq!(q_mut.w, q_norm.w, epsilon = EPSILON);
    }

    #[test]
    fn test_normalize_zero_quaternion() {
        let q_zero = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        let q_norm = q_zero.normalize();
        assert_eq!(q_norm, Quaternion::IDENTITY);
    }

    #[test]
    fn test_dot_product() {
        let angle = 0.5;
        let q1 = Quaternion::from_axis_angle(Vec3::X, angle);
        let q2 = Quaternion::from_axis_angle(Vec3::X, angle);
        let q3 = Quaternion::from_axis_angle(Vec3::Y, angle);
        let q4 = Quaternion::from_axis_angle(Vec3::X, -angle);

        assert_relative_eq!(q1.dot(q1), 1.0, epsilon = EPSILON);
        assert_relative_eq!(q1.dot(q2), 1.0, epsilon = EPSILON);
        assert!(q1.dot(q3).abs() < 1.0 - EPSILON);
        assert_relative_eq!(q1.dot(q4), angle.cos(), epsilon = EPSILON);
    }

    #[test]
    fn test_slerp_endpoints() {
        let q_start = Quaternion::IDENTITY;
        let q_end = Quaternion::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2);

        let q_t0 = Quaternion::slerp(q_start, q_end, 0.0);
        let q_t1 = Quaternion::slerp(q_start, q_end, 1.0);

        assert_relative_eq!(q_t0.x, q_start.x, epsilon = EPSILON);
        assert_relative_eq!(q_t0.y, q_start.y, epsilon = EPSILON);
        assert_relative_eq!(q_t0.z, q_start.z, epsilon = EPSILON);
        assert_relative_eq!(q_t0.w, q_start.w, epsilon = EPSILON);

        assert_relative_eq!(q_t1.x, q_end.x, epsilon = EPSILON);
        assert_relative_eq!(q_t1.y, q_end.y, epsilon = EPSILON);
        assert_relative_eq!(q_t1.z, q_end.z, epsilon = EPSILON);
        assert_relative_eq!(q_t1.w, q_end.w, epsilon = EPSILON);
    }

    #[test]
    fn test_slerp_midpoint() {
        let q_start = Quaternion::IDENTITY;
        let q_end = Quaternion::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2);
        let q_half = Quaternion::slerp(q_start, q_end, 0.5);
        let q_expected_half =
            Quaternion::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2 * 0.5);

        assert_relative_eq!(q_half.x, q_expected_half.x, epsilon = EPSILON);
        assert_relative_eq!(q_half.y, q_expected_half.y, epsilon = EPSILON);
        assert_relative_eq!(q_half.z, q_expected_half.z, epsilon = EPSILON);
        assert_relative_eq!(q_half.w, q_expected_half.w, epsilon = EPSILON);
        assert_relative_eq!(q_half.magnitude(), 1.0, epsilon = EPSILON);
    }

    #[test]
    fn test_slerp_short_path_handling() {
        let q_start = Quaternion::from_axis_angle(Vec3::Y, -30.0f32.to_radians());
        let q_end = Quaternion::from_axis_angle(Vec3::Y, 170.0f32.to_radians());
        assert!(q_start.dot(q_end) < 0.0);

        let q_mid = Quaternion::slerp(q_start, q_end, 0.5);
        let q_expected_mid = Quaternion::from_axis_angle(Vec3::Y, -110.0f32.to_radians()); // Midpoint on shortest path

        assert_relative_eq!(q_mid.dot(q_expected_mid).abs(), 1.0, epsilon = EPSILON);

        let v = Vec3::X;
        let v_rotated_mid = q_mid * v;
        let v_rotated_expected = q_expected_mid * v;
        assert_relative_eq!(v_rotated_mid.x, v_rotated_expected.x, epsilon = EPSILON);
        assert_relative_eq!(v_rotated_mid.y, v_rotated_expected.y, epsilon = EPSILON);
        assert_relative_eq!(v_rotated_mid.z, v_rotated_expected.z, epsilon = EPSILON);
    }

    #[test]
    fn test_slerp_near_identical_quaternions() {
        let angle1 = 0.00001;
        let angle2 = 0.00002;
        let q_close1 = Quaternion::from_axis_angle(Vec3::Y, angle1);
        let q_close2 = Quaternion::from_axis_angle(Vec3::Y, angle2);
        assert!(q_close1.dot(q_close2) > 1.0 - EPSILON);

        let q_mid = Quaternion::slerp(q_close1, q_close2, 0.5);
        let angle_mid = angle1 + (angle2 - angle1) * 0.5;
        let q_expected = Quaternion::from_axis_angle(Vec3::Y, angle_mid);

        assert_relative_eq!(q_mid.magnitude(), 1.0, epsilon = EPSILON * 10.0);

        let v = Vec3::X;
        let v_rotated = q_mid * v;
        let v_expected_rotated = q_expected * v;
        assert_relative_eq!(v_rotated.x, v_expected_rotated.x, epsilon = EPSILON * 10.0);
        assert_relative_eq!(v_rotated.y, v_expected_rotated.y, epsilon = EPSILON * 10.0);
        assert_relative_eq!(v_rotated.z, v_expected_rotated.z, epsilon = EPSILON * 10.0);
    }

    #[test]
    fn test_slerp_clamps_t() {
        let q_start = Quaternion::IDENTITY;
        let q_end = Quaternion::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2);

        let q_t_neg = Quaternion::slerp(q_start, q_end, -0.5); // t < 0
        let q_t_large = Quaternion::slerp(q_start, q_end, 1.5); // t > 1

        assert_relative_eq!(q_t_neg.x, q_start.x, epsilon = EPSILON);
        assert_relative_eq!(q_t_neg.y, q_start.y, epsilon = EPSILON);
        assert_relative_eq!(q_t_neg.z, q_start.z, epsilon = EPSILON);
        assert_relative_eq!(q_t_neg.w, q_start.w, epsilon = EPSILON);

        assert_relative_eq!(q_t_large.x, q_end.x, epsilon = EPSILON);
        assert_relative_eq!(q_t_large.y, q_end.y, epsilon = EPSILON);
        assert_relative_eq!(q_t_large.z, q_end.z, epsilon = EPSILON);
        assert_relative_eq!(q_t_large.w, q_end.w, epsilon = EPSILON);
    }
}
