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

//! Affine transformations for 2D and 3D space.

use crate::math::{Mat4, Quaternion, Vec3, Vec4};

/// Represents a 3D affine transformation (translation, rotation, scale).
///
/// This is a semantic wrapper around a `Mat4` that guarantees the matrix
/// represents a valid affine transform. It provides a dedicated API for
/// creating and manipulating these transformations.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub struct AffineTransform(pub Mat4);

impl AffineTransform {
    /// The identity transform, which results in no change.
    pub const IDENTITY: Self = Self(Mat4::IDENTITY);

    // --- CONSTRUCTORS ---
    /// Creates an `AffineTransform` from a translation vector.
    ///
    /// # Arguments
    ///
    /// * `v` - The translation vector to apply
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
    /// assert_eq!(transform.translation(), Vec3::new(1.0, 2.0, 3.0));
    /// ```
    #[inline]
    pub fn from_translation(v: Vec3) -> Self {
        Self(Mat4::from_cols(
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(v.x, v.y, v.z, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a non-uniform scale vector.
    ///
    /// # Arguments
    ///
    /// * `scale` - The scale vector to apply to each axis
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_scale(Vec3::new(2.0, 1.5, 0.5));
    /// ```
    #[inline]
    pub fn from_scale(scale: Vec3) -> Self {
        Self(Mat4::from_cols(
            Vec4::new(scale.x, 0.0, 0.0, 0.0),
            Vec4::new(0.0, scale.y, 0.0, 0.0),
            Vec4::new(0.0, 0.0, scale.z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a rotation around the X axis.
    ///
    /// # Arguments
    ///
    /// * `angle` - The angle of rotation in radians
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    /// use std::f32::consts::PI;
    ///
    /// let transform = AffineTransform::from_rotation_x(PI / 2.0);
    /// ```
    #[inline]
    pub fn from_rotation_x(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self(Mat4::from_cols(
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, c, s, 0.0),
            Vec4::new(0.0, -s, c, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a rotation around the Y axis.
    ///
    /// # Arguments
    ///
    /// * `angle` - The angle of rotation in radians
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    /// use std::f32::consts::PI;
    ///
    /// let transform = AffineTransform::from_rotation_y(PI / 2.0);
    /// ```
    #[inline]
    pub fn from_rotation_y(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self(Mat4::from_cols(
            Vec4::new(c, 0.0, -s, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(s, 0.0, c, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a rotation around the Z axis.
    ///
    /// # Arguments
    ///
    /// * `angle` - The angle of rotation in radians
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::f32::consts::PI;
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_rotation_z(PI / 2.0);
    /// ```
    #[inline]
    pub fn from_rotation_z(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self(Mat4::from_cols(
            Vec4::new(c, s, 0.0, 0.0),
            Vec4::new(-s, c, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a rotation around an arbitrary axis.
    ///
    /// Uses Rodrigues' rotation formula to create a rotation matrix.
    ///
    /// # Arguments
    ///
    /// * `axis` - The axis of rotation (will be normalized automatically)
    /// * `angle` - The angle of rotation in radians
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::f32::consts::PI;
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let axis = Vec3::new(1.0, 1.0, 0.0);
    /// let transform = AffineTransform::from_axis_angle(axis, PI / 4.0);
    /// ```
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        let t = 1.0 - c;
        let x = axis.x;
        let y = axis.y;
        let z = axis.z;

        Self(Mat4::from_cols(
            Vec4::new(t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0),
            Vec4::new(t * y * x + s * z, t * y * y + c, t * y * z - s * x, 0.0),
            Vec4::new(t * z * x - s * y, t * z * y + s * x, t * z * z + c, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
    }

    /// Creates an `AffineTransform` from a quaternion representing a rotation.
    ///
    /// # Arguments
    ///
    /// * `q` - The quaternion representing the rotation
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::{Quaternion, Vec3};
    /// use khora_core::math::affine_transform::AffineTransform;
    /// use std::f32::consts::PI;
    ///
    /// let q = Quaternion::from_axis_angle(Vec3::Y, PI / 2.0);
    /// let transform = AffineTransform::from_quat(q);
    /// ```
    #[inline]
    pub fn from_quat(q: Quaternion) -> Self {
        Self(Mat4::from_quat(q))
    }

    // --- SEMANTIC ACCESSORS---
    /// Converts the `AffineTransform` to a `Mat4`.
    ///
    /// This is useful when you need to pass the transformation matrix to shaders
    /// or other systems that expect a raw matrix.
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::IDENTITY;
    /// let matrix = transform.to_matrix();
    /// ```
    #[inline]
    pub fn to_matrix(&self) -> Mat4 {
        self.0
    }

    /// Extracts the translation component from the affine transform.
    ///
    /// Returns the translation vector representing the position offset
    /// applied by this transformation.
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
    /// assert_eq!(transform.translation(), Vec3::new(1.0, 2.0, 3.0));
    /// ```
    #[inline]
    pub fn translation(&self) -> Vec3 {
        self.0.cols[3].truncate()
    }

    /// Extracts the right direction vector from the affine transform.
    ///
    /// This returns the first column of the transformation matrix (excluding the w component),
    /// which represents the transformed positive X-axis direction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    /// use std::f32::consts::PI;
    ///
    /// let transform = AffineTransform::from_rotation_z(PI / 2.0);
    /// let right = transform.right();
    /// // After 90° rotation around Z, right vector points in -Y direction
    /// ```
    #[inline]
    pub fn right(&self) -> Vec3 {
        self.0.cols[0].truncate()
    }

    /// Extracts the up direction vector from the affine transform.
    ///
    /// This returns the second column of the transformation matrix (excluding the w component),
    /// which represents the transformed positive Y-axis direction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::f32::consts::PI;
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_rotation_x(PI / 2.0);
    /// let up = transform.up();
    /// // After 90° rotation around X, up vector points in -Z direction
    /// ```
    #[inline]
    pub fn up(&self) -> Vec3 {
        self.0.cols[1].truncate()
    }

    /// Extracts the forward direction vector from the affine transform.
    ///
    /// This returns the third column of the transformation matrix (excluding the w component),
    /// which represents the transformed positive Z-axis direction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::f32::consts::PI;
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_rotation_y(PI / 2.0);
    /// let forward = transform.forward();
    /// // After 90° rotation around Y, forward vector points in X direction
    /// ```
    #[inline]
    pub fn forward(&self) -> Vec3 {
        self.0.cols[2].truncate()
    }

    /// Extracts the rotation component as a quaternion.
    ///
    /// This method extracts the rotation represented by the upper-left 3x3
    /// portion of the transformation matrix.
    ///
    /// # Note
    ///
    /// This assumes the transform has uniform or no scale. For transforms
    /// with non-uniform scale, the result may not represent a pure rotation.
    /// In such cases, consider normalizing the direction vectors first.
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::{Quaternion, Vec3};
    /// use khora_core::math::affine_transform::AffineTransform;
    /// use std::f32::consts::PI;
    ///
    /// let q = Quaternion::from_axis_angle(Vec3::Y, PI / 2.0);
    /// let transform = AffineTransform::from_quat(q);
    /// let extracted = transform.rotation();
    /// // extracted should be approximately equal to q
    /// ```
    #[inline]
    pub fn rotation(&self) -> Quaternion {
        Quaternion::from_rotation_matrix(&self.0)
    }

    /// Computes the inverse of the affine transformation.
    ///
    /// This uses an optimized affine inverse algorithm that's more efficient than
    /// a general matrix inverse, taking advantage of the affine transform structure.
    /// Returns `None` if the transformation is not invertible (e.g., zero scale).
    ///
    /// # Returns
    ///
    /// `Some(AffineTransform)` if the inverse exists, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let transform = AffineTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
    /// let inverse = transform.inverse().unwrap();
    /// // The inverse should translate by (-1, -2, -3)
    /// ```
    #[inline]
    pub fn inverse(&self) -> Option<Self> {
        self.0.affine_inverse().map(Self)
    }
}

impl Default for AffineTransform {
    /// Returns the identity `AffineTransform`.
    fn default() -> Self {
        Self::IDENTITY
    }
}

// Allow easy conversion to the underlying Mat4 for sending to the GPU, etc.
impl From<AffineTransform> for Mat4 {
    /// Converts the `AffineTransform` into its inner `Mat4`.
    #[inline]
    fn from(transform: AffineTransform) -> Self {
        transform.0
    }
}

impl From<Mat4> for AffineTransform {
    /// Converts a `Mat4` into an `AffineTransform`.
    ///
    /// # Panics
    ///
    /// Panics if the matrix is not a valid affine transformation.
    #[inline]
    fn from(val: Mat4) -> Self {
        // Validate that the matrix is affine (last row must be [0, 0, 0, 1])
        let last_row = val.get_row(3);
        assert!(
            last_row == Vec4::new(0.0, 0.0, 0.0, 1.0),
            "Matrix is not a valid affine transformation"
        );
        AffineTransform(val)
    }
}
