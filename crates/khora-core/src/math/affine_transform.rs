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

use serde::{Deserialize, Serialize};

use crate::math::{Mat4, Quaternion, Vec3, Vec4};

/// Represents a 3D affine transformation (translation, rotation, scale).
///
/// This is a semantic wrapper around a `Mat4` that guarantees the matrix
/// represents a valid affine transform. It provides a dedicated API for
/// creating and manipulating these transformations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

    /// Extracts the non-negative scale factors along each local axis.
    ///
    /// Returns the Euclidean lengths of the three basis columns (right, up,
    /// forward). For a transform built as translation ∘ rotation ∘ scale this
    /// recovers the original scale vector. Mirror/negative scales are reported
    /// as their magnitudes (sign is folded into the rotation extraction).
    #[inline]
    pub fn scale(&self) -> Vec3 {
        Vec3::new(
            self.right().length(),
            self.up().length(),
            self.forward().length(),
        )
    }

    /// Interpolates between two affine transforms for smooth rendering
    /// between discrete simulation steps.
    ///
    /// Decomposes both transforms into translation / rotation / scale and
    /// blends each channel independently: translation and scale by linear
    /// interpolation, rotation by spherical interpolation (shortest-path
    /// `slerp`). `alpha` is clamped to `[0, 1]`; `alpha = 0` returns `self`
    /// (the *previous* transform) and `alpha = 1` returns `other` (the
    /// *current* transform).
    ///
    /// This is a render-only blend: it never feeds back into the authoritative
    /// simulation state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use khora_core::math::Vec3;
    /// use khora_core::math::affine_transform::AffineTransform;
    ///
    /// let prev = AffineTransform::from_translation(Vec3::ZERO);
    /// let curr = AffineTransform::from_translation(Vec3::new(10.0, 0.0, 0.0));
    ///
    /// // The endpoints round-trip the inputs; the midpoint blends them.
    /// assert_eq!(prev.interpolate(&curr, 0.0).translation(), Vec3::ZERO);
    /// assert_eq!(prev.interpolate(&curr, 1.0).translation(), curr.translation());
    ///
    /// let mid = prev.interpolate(&curr, 0.5).translation();
    /// assert!((mid - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-5);
    /// ```
    #[inline]
    pub fn interpolate(&self, other: &Self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        let (from_rotation, from_scale) = self.signed_rotation_scale();
        let (to_rotation, to_scale) = other.signed_rotation_scale();
        let translation = Vec3::lerp(self.translation(), other.translation(), alpha);
        let rotation = Quaternion::slerp(from_rotation, to_rotation, alpha);
        let scale = Vec3::lerp(from_scale, to_scale, alpha);

        let mat = Mat4::from_translation(translation)
            * Mat4::from_quat(rotation)
            * Mat4::from_scale(scale);
        Self(mat)
    }

    /// The rotation and the *signed* scale this transform is made of: a
    /// mirror (negative determinant) is a negative scale on the first axis,
    /// so rotation ∘ scale rebuilds it — where [`Self::rotation`] and
    /// [`Self::scale`] cannot, scale being non-negative there.
    fn signed_rotation_scale(&self) -> (Quaternion, Vec3) {
        let columns = [self.right(), self.up(), self.forward()];
        let mirrored = columns[0].dot(columns[1].cross(columns[2])) < 0.0;
        let mut scale = Vec3::new(
            columns[0].length(),
            columns[1].length(),
            columns[2].length(),
        );
        if mirrored {
            scale.x = -scale.x;
        }
        let signed = [scale.x, scale.y, scale.z];
        let known: [Option<Vec3>; 3] = std::array::from_fn(|i| {
            (signed[i] != 0.0 && signed[i].is_finite()).then(|| columns[i] * (1.0 / signed[i]))
        });
        let axes = complete_basis(known);
        let rotation = Quaternion::from_rotation_matrix(&Mat4::from_cols(
            Vec4::new(axes[0].x, axes[0].y, axes[0].z, 0.0),
            Vec4::new(axes[1].x, axes[1].y, axes[1].z, 0.0),
            Vec4::new(axes[2].x, axes[2].y, axes[2].z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ))
        .normalize();
        (rotation, scale)
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

/// Three perpendicular unit axes keeping the `known` ones: an axis scaled to
/// zero has no direction of its own, so it is rebuilt from the others
/// (right-handed), and a flattened pose keeps the rotation it shows.
fn complete_basis(known: [Option<Vec3>; 3]) -> [Vec3; 3] {
    match known {
        [Some(x), Some(y), Some(z)] => [x, y, z],
        [None, Some(y), Some(z)] => [y.cross(z), y, z],
        [Some(x), None, Some(z)] => [x, z.cross(x), z],
        [Some(x), Some(y), None] => [x, y, x.cross(y)],
        [Some(x), None, None] => {
            let y = any_perpendicular(x);
            [x, y, x.cross(y)]
        }
        [None, Some(y), None] => {
            let z = any_perpendicular(y);
            [y.cross(z), y, z]
        }
        [None, None, Some(z)] => {
            let x = any_perpendicular(z);
            [x, z.cross(x), z]
        }
        [None, None, None] => [Vec3::X, Vec3::Y, Vec3::Z],
    }
}

/// A unit vector perpendicular to the unit vector `v`.
fn any_perpendicular(v: Vec3) -> Vec3 {
    let helper = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    v.cross(helper).normalize()
}

#[cfg(test)]
mod interpolation_tests {
    use super::*;
    use crate::math::Quaternion;
    use std::f32::consts::PI;

    fn vec3_close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn interpolate_endpoints_return_inputs() {
        let prev = AffineTransform::from_translation(Vec3::new(0.0, 0.0, 0.0));
        let curr = AffineTransform::from_translation(Vec3::new(10.0, 0.0, 0.0));

        let at_zero = prev.interpolate(&curr, 0.0);
        let at_one = prev.interpolate(&curr, 1.0);

        assert!(vec3_close(at_zero.translation(), prev.translation()));
        assert!(vec3_close(at_one.translation(), curr.translation()));
    }

    #[test]
    fn interpolate_midpoint_blends_translation_and_scale() {
        let prev = AffineTransform(
            Mat4::from_translation(Vec3::new(0.0, 0.0, 0.0))
                * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0)),
        );
        let curr = AffineTransform(
            Mat4::from_translation(Vec3::new(4.0, 8.0, 2.0))
                * Mat4::from_scale(Vec3::new(3.0, 3.0, 3.0)),
        );

        let mid = prev.interpolate(&curr, 0.5);

        assert!(vec3_close(mid.translation(), Vec3::new(2.0, 4.0, 1.0)));
        assert!(vec3_close(mid.scale(), Vec3::new(2.0, 2.0, 2.0)));
    }

    #[test]
    fn interpolate_midpoint_blends_rotation() {
        let y_axis = Vec3::new(0.0, 1.0, 0.0);
        let prev = AffineTransform::from_quat(Quaternion::IDENTITY);
        let curr = AffineTransform::from_quat(Quaternion::from_axis_angle(y_axis, PI / 2.0));

        let mid = prev.interpolate(&curr, 0.5);
        let expected = Quaternion::from_axis_angle(y_axis, PI / 4.0);

        // Rotating a forward vector by the interpolated quaternion must match
        // rotating it by the half-angle rotation (slerp is constant-speed).
        let probe = Vec3::new(0.0, 0.0, 1.0);
        let got = mid.rotation() * probe;
        let want = expected * probe;
        assert!(vec3_close(got, want), "got {got:?}, want {want:?}");
    }

    #[test]
    fn interpolate_clamps_alpha() {
        let prev = AffineTransform::from_translation(Vec3::ZERO);
        let curr = AffineTransform::from_translation(Vec3::new(10.0, 0.0, 0.0));

        let below = prev.interpolate(&curr, -1.0);
        let above = prev.interpolate(&curr, 2.0);

        assert!(vec3_close(below.translation(), prev.translation()));
        assert!(vec3_close(above.translation(), curr.translation()));
    }

    // ─── Mirrored poses ─────────────────────────────────────────────────

    /// Translation ∘ rotation ∘ scale, the order `Transform::to_mat4` composes.
    fn trs(translation: Vec3, rotation: Quaternion, scale: Vec3) -> AffineTransform {
        AffineTransform(
            Mat4::from_translation(translation)
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(scale),
        )
    }

    /// Determinant of the upper 3×3: negative when the transform mirrors.
    fn determinant(transform: &AffineTransform) -> f32 {
        crate::math::Mat3::from_mat4(&transform.0).determinant()
    }

    /// Asserts every element of the two matrices agrees within 1e-4.
    fn assert_matrix_close(actual: &AffineTransform, expected: &AffineTransform, what: &str) {
        for (column, (a, e)) in actual.0.cols.iter().zip(expected.0.cols.iter()).enumerate() {
            let differences = [a.x - e.x, a.y - e.y, a.z - e.z, a.w - e.w];
            assert!(
                differences.iter().all(|d| d.abs() < 1e-4),
                "{what}: column {column} is {a:?} where {expected:?} was expected (whole matrix {actual:?})",
                expected = e,
                actual = actual.0,
            );
        }
    }

    /// A pose mirrored along X with a rotation about Y.
    fn mirrored_x_pose() -> AffineTransform {
        trs(
            Vec3::new(1.0, 2.0, 3.0),
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), PI / 6.0),
            Vec3::new(-1.0, 1.0, 1.0),
        )
    }

    /// A pose mirrored along Z with a rotation about an oblique axis.
    fn mirrored_z_pose() -> AffineTransform {
        trs(
            Vec3::new(4.0, 0.0, -2.0),
            Quaternion::from_axis_angle(Vec3::new(1.0, 1.0, 0.0).normalize(), 70f32.to_radians()),
            Vec3::new(1.0, 1.0, -1.0),
        )
    }

    /// Two mirrored ends interpolate to a mirrored pose at every alpha in
    /// between: blending must not flip the mesh back inside-out.
    #[test]
    fn interpolating_between_two_mirrored_poses_stays_mirrored() {
        let prev = mirrored_x_pose();
        let curr = mirrored_z_pose();
        assert!(determinant(&prev) < 0.0 && determinant(&curr) < 0.0);

        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let blended = prev.interpolate(&curr, alpha);
            let det = determinant(&blended);
            assert!(
                det < 0.0,
                "alpha {alpha}: determinant {det} — the blended pose is no longer mirrored"
            );
        }
    }

    /// At alpha 0 and 1 the interpolation of two mirrored poses gives back
    /// each end, rotation and mirror included.
    #[test]
    fn interpolating_mirrored_poses_returns_each_end_at_alpha_zero_and_one() {
        let prev = mirrored_x_pose();
        let curr = mirrored_z_pose();

        assert_matrix_close(&prev.interpolate(&curr, 0.0), &prev, "alpha 0");
        assert_matrix_close(&prev.interpolate(&curr, 1.0), &curr, "alpha 1");
    }

    /// A pose that both rotates and mirrors survives being interpolated with
    /// itself, at any alpha.
    #[test]
    fn a_rotated_mirror_round_trips_through_interpolation() {
        let pose = trs(
            Vec3::new(-3.0, 0.5, 7.0),
            Quaternion::from_axis_angle(Vec3::new(0.3, -0.5, 0.8).normalize(), 2.1),
            Vec3::new(1.0, -1.0, 1.0),
        );

        for alpha in [0.0, 0.5, 1.0] {
            assert_matrix_close(
                &pose.interpolate(&pose, alpha),
                &pose,
                &format!("alpha {alpha}"),
            );
        }
    }

    /// The scale is blended signed: two poses mirrored along X with
    /// different magnitudes blend to the mirrored mean, not to its absolute
    /// value.
    #[test]
    fn interpolating_mirrored_scales_blends_the_signed_scale() {
        let prev = trs(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(-2.0, 1.0, 3.0));
        let curr = trs(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(-4.0, 1.0, 1.0));

        let mid = prev.interpolate(&curr, 0.5);

        assert_matrix_close(
            &mid,
            &trs(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(-3.0, 1.0, 2.0)),
            "alpha 0.5",
        );
    }

    // ─── Degenerate and non-uniform poses ───────────────────────────────

    /// Asserts no element of the matrix is NaN or infinite.
    fn assert_matrix_finite(transform: &AffineTransform, what: &str) {
        assert!(
            transform
                .0
                .cols
                .iter()
                .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite() && c.w.is_finite()),
            "{what}: the matrix holds a non-finite value: {:?}",
            transform.0
        );
    }

    /// A rotated pose scaled differently on each axis comes back unchanged at
    /// alpha 0 and 1: the rotation is read from the normalised columns, not
    /// from the scaled ones.
    #[test]
    fn a_rotated_non_uniformly_scaled_pose_returns_each_end_at_alpha_zero_and_one() {
        let prev = trs(
            Vec3::new(1.0, -2.0, 0.5),
            Quaternion::from_axis_angle(Vec3::new(0.4, 1.0, -0.2).normalize(), 1.3),
            Vec3::new(0.5, 3.0, 1.5),
        );
        let curr = trs(
            Vec3::new(-4.0, 1.0, 2.0),
            Quaternion::from_axis_angle(Vec3::new(1.0, 0.0, 0.3).normalize(), -0.8),
            Vec3::new(2.0, 0.25, 4.0),
        );

        assert_matrix_close(&prev.interpolate(&curr, 0.0), &prev, "alpha 0");
        assert_matrix_close(&prev.interpolate(&curr, 1.0), &curr, "alpha 1");
    }

    /// A pose scaled to nothing (an entity popping in from zero size)
    /// interpolates to a finite matrix: no division by a zero scale leaks a
    /// NaN into the drawn pose.
    #[test]
    fn a_pose_scaled_to_zero_interpolates_without_nan() {
        let prev = trs(Vec3::new(1.0, 2.0, 3.0), Quaternion::IDENTITY, Vec3::ZERO);
        let curr = trs(
            Vec3::new(1.0, 2.0, 3.0),
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.9),
            Vec3::new(1.0, 1.0, 1.0),
        );

        for alpha in [0.0, 0.25, 0.5, 1.0] {
            let blended = prev.interpolate(&curr, alpha);
            assert_matrix_finite(&blended, &format!("alpha {alpha}"));
        }
        assert_matrix_close(&prev.interpolate(&curr, 0.0), &prev, "alpha 0");
        assert_matrix_close(&prev.interpolate(&curr, 1.0), &curr, "alpha 1");
    }

    /// A rotated pose flattened to zero along one axis comes back unchanged
    /// at every alpha when interpolated with itself: the two axes that still
    /// have a size keep their direction.
    #[test]
    fn a_rotated_pose_flattened_on_one_axis_round_trips_through_interpolation() {
        let pose = trs(
            Vec3::new(2.0, 0.0, -1.0),
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), PI / 6.0),
            Vec3::new(0.0, 1.0, 1.0),
        );

        for alpha in [0.0, 0.5, 1.0] {
            assert_matrix_close(
                &pose.interpolate(&pose, alpha),
                &pose,
                &format!("alpha {alpha}"),
            );
        }
    }
}
