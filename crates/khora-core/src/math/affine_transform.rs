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

/// A basis axis shorter than this fraction of the longest one is flattened:
/// [`AffineTransform::rotation`] rebuilds it from the two others, or gives
/// up when two are flattened.
const RELATIVE_DEGENERACY: f32 = 1e-6;

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
    /// The basis columns are divided by their lengths first, so any scale —
    /// uniform or not — is left out: a matrix-to-quaternion conversion fed a
    /// scaled basis returns a unit quaternion with the **wrong angle** (0.6 ×
    /// a quarter turn comes out as ~74°). A mirrored basis (negative
    /// determinant) has no rotation that reproduces it; the mirror is put on
    /// the first axis, consistent with [`scale`](Self::scale) reporting
    /// magnitudes.
    ///
    /// How small a scale is never matters, only how the axes compare: an
    /// axis is flattened when it is shorter than a millionth of the longest.
    /// One flattened axis is rebuilt as the right-handed cross product of
    /// the two others, so a transform squashed flat keeps its rotation. While
    /// the flattened axis keeps a direction, a mirror is read from it and put
    /// on the first axis as above; an axis scaled to exactly zero has none,
    /// and a mirror on the surviving axes then turns into a half turn. Two
    /// or more flattened axes, two parallel
    /// survivors, or a non-finite basis leave no rotation to recover and
    /// yield the identity.
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
        let mut axes = [self.right(), self.up(), self.forward()];
        let components = axes.iter().flat_map(|axis| [axis.x, axis.y, axis.z]);
        if components.clone().any(|c| !c.is_finite()) {
            return Quaternion::IDENTITY;
        }
        // Measured against its own largest component, the basis neither
        // underflows when squared nor meets an absolute threshold: a
        // millimetre node keeps its rotation like a metre one.
        let largest = components.fold(0.0f32, |largest, c| largest.max(c.abs()));
        if largest == 0.0 {
            return Quaternion::IDENTITY;
        }
        // Component by component: `Vec3 / f32` multiplies by the reciprocal,
        // which is infinite for a subnormal `largest`.
        for axis in &mut axes {
            *axis = Vec3::new(axis.x / largest, axis.y / largest, axis.z / largest);
        }
        let lengths = axes.map(|axis| axis.length());
        let longest = lengths[0].max(lengths[1]).max(lengths[2]);
        let flat = lengths.map(|length| length <= RELATIVE_DEGENERACY * longest);
        match flat.iter().filter(|&&is_flat| is_flat).count() {
            0 => {
                for (axis, length) in axes.iter_mut().zip(lengths) {
                    *axis = *axis / length;
                }
                if axes[0].cross(axes[1]).dot(axes[2]) < 0.0 {
                    axes[0] = -axes[0];
                }
            }
            1 => {
                let missing = flat.iter().position(|&is_flat| is_flat).unwrap_or(0);
                // A basis flattened along one axis still carries its rotation
                // in the two others; the lost one is their right-handed cross
                // product (right = up × forward, up = forward × right,
                // forward = right × up).
                let (next, after) = ((missing + 1) % 3, (missing + 2) % 3);
                let rebuilt = axes[next].cross(axes[after]);
                let rebuilt_length = rebuilt.length();
                if rebuilt_length <= RELATIVE_DEGENERACY * lengths[next] * lengths[after] {
                    return Quaternion::IDENTITY;
                }
                // A short axis still points somewhere: if it points against
                // the rebuilt one the basis is mirrored, and the mirror goes
                // to the first axis as it does for an unflattened basis — so
                // a mirrored pose does not turn over as an axis shrinks. Only
                // an axis scaled to exactly zero leaves the mirror unknown.
                let mirrored = rebuilt.dot(axes[missing]) < 0.0;
                axes[missing] = rebuilt / rebuilt_length;
                axes[next] = axes[next] / lengths[next];
                axes[after] = axes[after] / lengths[after];
                if mirrored && missing != 0 {
                    axes[missing] = -axes[missing];
                    axes[0] = -axes[0];
                }
            }
            _ => return Quaternion::IDENTITY,
        }
        let [right, up, forward] = axes;
        let basis = Mat4::from_cols(
            Vec4::new(right.x, right.y, right.z, 0.0),
            Vec4::new(up.x, up.y, up.z, 0.0),
            Vec4::new(forward.x, forward.y, forward.z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
        Quaternion::from_rotation_matrix(&basis)
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
        let translation = Vec3::lerp(self.translation(), other.translation(), alpha);
        let rotation = Quaternion::slerp(self.rotation(), other.rotation(), alpha);
        let scale = Vec3::lerp(self.scale(), other.scale(), alpha);

        let mat = Mat4::from_translation(translation)
            * Mat4::from_quat(rotation)
            * Mat4::from_scale(scale);
        Self(mat)
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
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    use std::f32::consts::PI;

    /// The angle, in degrees, of the rotation that takes `a` to `b`.
    ///
    /// Read from the relative rotation `d = a⁻¹·b` as
    /// `2·atan2(|d.xyz|, |d.w|)`: unlike `acos` of a dot product — whose
    /// f32 resolution near 1 is ~0.04° — it resolves tiny angles, so a 0.01°
    /// difference is told apart from none. `q` and `-q` are the same
    /// rotation, hence `|d.w|`.
    fn angle_between_degrees(a: Quaternion, b: Quaternion) -> f32 {
        let d = a.normalize().conjugate() * b.normalize();
        let vector = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        (2.0 * vector.atan2(d.w.abs())).to_degrees()
    }

    /// The angle helper resolves a hundredth of a degree, reads `q` and `-q`
    /// as the same rotation, and measures a half turn as 180°.
    #[test]
    fn the_angle_between_two_rotations_resolves_a_hundredth_of_a_degree() {
        let base = oblique_rotation();
        let axis = Vec3::new(-0.3, 0.4, 1.0).normalize();
        assert!(angle_between_degrees(base, base) < 1e-4);
        let negated = Quaternion::new(-base.x, -base.y, -base.z, -base.w);
        assert!(angle_between_degrees(base, negated) < 1e-4);
        for degrees in [0.01f32, 0.005, 1.0, 90.0, 180.0] {
            let turned = base * Quaternion::from_axis_angle(axis, degrees.to_radians());
            let measured = angle_between_degrees(base, turned);
            assert!(
                (measured - degrees).abs() < 1e-3 * degrees.max(1.0),
                "a {degrees}° turn measured as {measured}°"
            );
        }
    }

    /// Translation ∘ rotation ∘ scale, the order `Transform::to_mat4` composes.
    fn trs(translation: Vec3, rotation: Quaternion, scale: Vec3) -> AffineTransform {
        AffineTransform(
            Mat4::from_translation(translation)
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(scale),
        )
    }

    fn assert_unit_and_finite(q: Quaternion) {
        assert!(
            q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite(),
            "the extracted rotation must be finite, got {q:?}"
        );
        assert!(
            (q.magnitude() - 1.0).abs() < 1e-4,
            "the extracted rotation must be a unit quaternion, got norm {}",
            q.magnitude()
        );
    }

    /// **A scaled crate turned a quarter came out turned 73.7°.** The upper 3×3
    /// went to the quaternion conversion with its scale still in it, so the
    /// trace mixed a scaled diagonal with an unscaled 1. Every consumer —
    /// physics sync, render flows, interpolation — read the wrong angle.
    #[test]
    fn rotation_of_a_uniformly_scaled_quarter_turn_is_the_quarter_turn() {
        let yaw = Quaternion::from_axis_angle(Vec3::Y, PI / 2.0);
        let transform = trs(Vec3::new(1.0, 2.0, 3.0), yaw, Vec3::new(0.6, 0.6, 0.6));

        let extracted = transform.rotation();

        assert_unit_and_finite(extracted);
        let error = angle_between_degrees(extracted, yaw);
        assert!(
            error < 0.01,
            "scale 0.6 + 90° yaw must extract a 90° yaw; off by {error}° ({extracted:?})"
        );
    }

    /// A non-uniform scale under an oblique rotation is the general case: each
    /// basis column has its own length, and the rotation must come out of it
    /// unchanged.
    #[test]
    fn rotation_of_a_non_uniformly_scaled_oblique_rotation_is_that_rotation() {
        let rotation = Quaternion::from_axis_angle(Vec3::new(1.0, 2.0, -0.5).normalize(), 1.1);
        let transform = trs(
            Vec3::new(-4.0, 0.5, 7.0),
            rotation,
            Vec3::new(1.0, 2.0, 3.0),
        );

        let extracted = transform.rotation();

        assert_unit_and_finite(extracted);
        let error = angle_between_degrees(extracted, rotation);
        assert!(
            error < 0.01,
            "scale (1, 2, 3) must not leak into the rotation; off by {error}° ({extracted:?})"
        );
    }

    /// A scaled, unrotated transform has no rotation. This already held — by
    /// accident, the diagonal is all the matrix has — and must keep holding.
    #[test]
    fn rotation_of_a_scale_alone_is_the_identity() {
        let transform = trs(
            Vec3::new(3.0, 0.0, 0.0),
            Quaternion::IDENTITY,
            Vec3::new(0.6, 0.6, 0.6),
        );

        let extracted = transform.rotation();

        assert_unit_and_finite(extracted);
        let error = angle_between_degrees(extracted, Quaternion::IDENTITY);
        assert!(
            error < 0.01,
            "scale alone has no rotation; got {error}° ({extracted:?})"
        );
    }

    /// A mirror is not a rotation; it belongs to the scale, which already
    /// reports magnitudes. Mirroring the first axis under a rotation must give
    /// back that rotation, the mirror folded into the first column.
    #[test]
    fn rotation_of_a_first_axis_mirror_is_the_unmirrored_rotation() {
        let yaw = Quaternion::from_axis_angle(Vec3::Y, 40f32.to_radians());
        let transform = trs(Vec3::ZERO, yaw, Vec3::new(-2.0, 1.0, 1.0));

        let extracted = transform.rotation();

        assert_unit_and_finite(extracted);
        let error = angle_between_degrees(extracted, yaw);
        assert!(
            error < 0.01,
            "a mirror on the first axis goes to the scale; the rotation stays 40° yaw, \
             off by {error}° ({extracted:?})"
        );
    }

    /// Whatever axis is mirrored, the result is a usable rotation: unit and
    /// finite, never a NaN handed to a solver.
    #[test]
    fn rotation_of_a_mirrored_transform_is_a_unit_finite_quaternion() {
        let rotation = Quaternion::from_axis_angle(Vec3::new(0.3, 1.0, 0.2).normalize(), 0.7);
        let transform = trs(Vec3::ZERO, rotation, Vec3::new(1.5, 0.5, -1.0));

        assert_unit_and_finite(transform.rotation());
    }

    /// The oblique rotation the robustness tests below recover: no basis
    /// column lies on a world axis, so no component of the answer is free.
    fn oblique_rotation() -> Quaternion {
        Quaternion::from_axis_angle(Vec3::new(1.0, 2.0, -0.5).normalize(), 1.1)
    }

    /// Asserts `transform.rotation()` is a unit, finite quaternion within
    /// 0.01° of `expected`.
    fn assert_rotation_is(transform: AffineTransform, expected: Quaternion, what: &str) {
        let extracted = transform.rotation();
        assert_unit_and_finite(extracted);
        let error = angle_between_degrees(extracted, expected);
        assert!(
            error < 0.01,
            "{what}: off by {error}° ({extracted:?} where {expected:?} was expected)"
        );
    }

    /// A millimetre asset nested in a centimetre node has a 1e-6 uniform
    /// scale. Every column still carries the rotation exactly (direction
    /// precision is relative to the column's own length), so the rotation is
    /// recovered, not dropped to the identity by an absolute length threshold.
    #[test]
    fn rotation_under_a_tiny_uniform_scale_is_that_rotation() {
        let rotation = oblique_rotation();
        assert_rotation_is(
            trs(
                Vec3::new(2.0, -1.0, 0.5),
                rotation,
                Vec3::new(1e-6, 1e-6, 1e-6),
            ),
            rotation,
            "uniform scale 1e-6",
        );
    }

    /// A tiny non-uniform scale is the same case: the columns are compared to
    /// each other, not to an absolute length.
    #[test]
    fn rotation_under_a_tiny_non_uniform_scale_is_that_rotation() {
        let rotation = oblique_rotation();
        assert_rotation_is(
            trs(Vec3::ZERO, rotation, Vec3::new(1e-6, 3e-6, 2e-6)),
            rotation,
            "non-uniform scale (1e-6, 3e-6, 2e-6)",
        );
    }

    /// A 1e-20 scale is well inside f32's normal range, but squaring its
    /// components to measure a length underflows. The basis is rescaled before
    /// it is measured, so the rotation still comes out.
    #[test]
    fn rotation_under_a_scale_whose_squared_length_underflows_is_that_rotation() {
        let rotation = oblique_rotation();
        assert_rotation_is(
            trs(Vec3::ZERO, rotation, Vec3::new(1e-20, 1e-20, 1e-20)),
            rotation,
            "uniform scale 1e-20",
        );
    }

    /// Flattening one axis to zero loses that column, not the rotation: the
    /// missing axis is the right-handed cross product of the two others.
    /// Holds for each of the three axes, whatever the survivors' lengths.
    #[test]
    fn rotation_with_one_flattened_axis_is_that_rotation() {
        let rotation = oblique_rotation();
        for scale in [
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 2.0, 0.5),
            Vec3::new(3.0, 0.0, 0.25),
            Vec3::new(0.5, 4.0, 0.0),
        ] {
            assert_rotation_is(
                trs(Vec3::new(-3.0, 1.0, 2.0), rotation, scale),
                rotation,
                &format!("one flattened axis, scale {scale:?}"),
            );
        }
    }

    /// An axis ten million times shorter than the longest one counts as
    /// flattened too (the tolerance is relative) and is rebuilt from the two
    /// others.
    #[test]
    fn rotation_with_one_nearly_flattened_axis_is_that_rotation() {
        let rotation = oblique_rotation();
        for scale in [
            Vec3::new(1e-7, 1.0, 1.0),
            Vec3::new(1.0, 1e-7, 1.0),
            Vec3::new(1.0, 1.0, 1e-7),
        ] {
            assert_rotation_is(
                trs(Vec3::ZERO, rotation, scale),
                rotation,
                &format!("one nearly flattened axis, scale {scale:?}"),
            );
        }
    }

    /// A flattened axis next to a mirrored one still gives a usable rotation:
    /// unit and finite, never a NaN handed to a solver.
    #[test]
    fn rotation_with_a_flattened_and_a_mirrored_axis_is_a_unit_finite_quaternion() {
        let rotation = oblique_rotation();
        for scale in [
            Vec3::new(-1.0, 1.0, 0.0),
            Vec3::new(0.0, -2.0, 1.0),
            Vec3::new(1.0, 0.0, -0.5),
            Vec3::new(-1.0, -1.0, 0.0),
        ] {
            assert_unit_and_finite(trs(Vec3::ZERO, rotation, scale).rotation());
        }
    }

    /// Two flattened axes leave a single line: the twist around it is
    /// undefined, so there is no rotation to recover and the answer is the
    /// identity.
    #[test]
    fn rotation_with_two_flattened_axes_is_the_identity() {
        let rotation = oblique_rotation();
        for scale in [
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ] {
            assert_rotation_is(
                trs(Vec3::ZERO, rotation, scale),
                Quaternion::IDENTITY,
                &format!("two flattened axes, scale {scale:?}"),
            );
        }
    }

    /// A transform flattened to a point (all three axes zero) has no rotation
    /// and reads as the identity.
    #[test]
    fn rotation_with_all_axes_flattened_is_the_identity() {
        assert_rotation_is(
            trs(Vec3::new(1.0, 2.0, 3.0), oblique_rotation(), Vec3::ZERO),
            Quaternion::IDENTITY,
            "all three axes flattened",
        );
    }

    /// One flattened column and two surviving columns that are parallel span
    /// a line, not a plane: the missing axis cannot be rebuilt, so the answer
    /// is the identity.
    #[test]
    fn rotation_with_one_flattened_axis_and_two_parallel_survivors_is_the_identity() {
        let transform = AffineTransform(Mat4::from_cols(
            Vec4::new(1.0, 2.0, 0.5, 0.0),
            Vec4::new(-2.0, -4.0, -1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ));
        assert_rotation_is(
            transform,
            Quaternion::IDENTITY,
            "parallel survivors around a flattened axis",
        );
    }

    /// A basis carrying a NaN or an infinity has no rotation to read: the
    /// answer is the identity, finite, rather than a NaN that spreads to every
    /// consumer.
    #[test]
    fn rotation_of_a_non_finite_basis_is_the_identity() {
        let cases = [
            (
                "NaN in the right column",
                Vec4::new(f32::NAN, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
            ),
            (
                "NaN in the forward column",
                Vec4::new(0.0, 0.0, -1.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(1.0, f32::NAN, 0.0, 0.0),
            ),
            (
                "+inf in the up column",
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, f32::INFINITY, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
            ),
            (
                "-inf in the right column",
                Vec4::new(f32::NEG_INFINITY, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
            ),
        ];
        for (what, right, up, forward) in cases {
            let transform = AffineTransform(Mat4::from_cols(
                right,
                up,
                forward,
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ));
            assert_rotation_is(transform, Quaternion::IDENTITY, what);
        }
    }

    /// A basis whose largest component is subnormal (below `1 / f32::MAX`,
    /// about 2.9e-39) still carries its rotation: its columns are compared to
    /// each other, never to an absolute size, so the rotation comes out.
    #[test]
    fn rotation_under_a_subnormal_uniform_scale_is_that_rotation() {
        let rotation = oblique_rotation();
        for scale in [1e-39f32, 1e-40] {
            assert_rotation_is(
                trs(Vec3::ZERO, rotation, Vec3::new(scale, scale, scale)),
                rotation,
                &format!("uniform scale {scale:e}"),
            );
        }
    }

    /// A scale whose squared column length overflows f32 (1e30² is past
    /// `f32::MAX`) is the mirror image of the underflow case: the basis is
    /// rescaled before it is measured, so the rotation still comes out.
    #[test]
    fn rotation_under_a_scale_whose_squared_length_overflows_is_that_rotation() {
        let rotation = oblique_rotation();
        for scale in [
            Vec3::new(1e30, 1e30, 1e30),
            Vec3::new(1e30, 2e30, 3e30),
            Vec3::new(3e38, 3e38, 3e38),
            Vec3::new(1e30, 1e30, 0.0),
        ] {
            assert_rotation_is(
                trs(Vec3::ZERO, rotation, scale),
                rotation,
                &format!("huge scale {scale:?}"),
            );
        }
    }

    /// Rotations at and near a half turn take the quaternion conversion off
    /// its positive-trace branch; with one axis flattened and rebuilt, each
    /// branch still returns the rotation.
    #[test]
    fn rotation_near_a_half_turn_with_one_flattened_axis_is_that_rotation() {
        let axis = Vec3::new(1.0, 2.0, -0.5).normalize();
        for angle in [PI - 1e-3, PI, PI + 0.3, 2.0] {
            let rotation = Quaternion::from_axis_angle(axis, angle);
            for scale in [
                Vec3::new(0.0, 1.0, 2.0),
                Vec3::new(1.5, 0.0, 1.0),
                Vec3::new(1.0, 0.5, 0.0),
            ] {
                // Compared through the axes it turns: the angle between two
                // quaternions is too coarse in f32 (one ulp of their dot
                // product is ~0.02°) to resolve a near half turn.
                let extracted = trs(Vec3::ZERO, rotation, scale).rotation();
                assert_unit_and_finite(extracted);
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    let error = (extracted * axis - rotation * axis).length();
                    assert!(
                        error < 1e-5,
                        "angle {angle}, scale {scale:?}: {axis:?} turned to {:?} where \
                         {:?} was expected",
                        extracted * axis,
                        rotation * axis
                    );
                }
            }
        }
    }

    /// A mirrored transform whose third axis shrinks keeps the rotation it
    /// had: the shrinking column still carries its direction (and so the
    /// basis its handedness) until it is exactly zero, so crossing the
    /// flattening tolerance must not turn the rotation over by a half turn.
    #[test]
    fn rotation_of_a_mirrored_transform_does_not_turn_over_as_an_axis_nearly_flattens() {
        let rotation = oblique_rotation();
        for (above, below) in [
            (Vec3::new(-1.0, 1.0, 1e-5), Vec3::new(-1.0, 1.0, 1e-7)),
            (Vec3::new(1.0, -1.0, 1e-5), Vec3::new(1.0, -1.0, 1e-7)),
            (Vec3::new(1.0, 1.0, -1e-5), Vec3::new(1.0, 1.0, -1e-7)),
        ] {
            let before = trs(Vec3::ZERO, rotation, above).rotation();
            let after = trs(Vec3::ZERO, rotation, below).rotation();
            let jump = angle_between_degrees(before, after);
            assert!(
                jump < 0.01,
                "scale {above:?} -> {below:?} turned the rotation by {jump}° \
                 ({before:?} -> {after:?})"
            );
        }
    }

    /// A rank-deficient basis with no short column — a rotated child under a
    /// parent flattened on Z, whose three columns all lie in one plane — is
    /// not a rotation, but what comes out is still a usable one: unit and
    /// finite, never a NaN handed to a solver.
    #[test]
    fn rotation_of_a_planar_basis_with_no_short_column_is_a_unit_finite_quaternion() {
        let flattened_parent = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.0));
        for child in [
            oblique_rotation(),
            Quaternion::from_axis_angle(Vec3::X, PI / 4.0),
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 1.0).normalize(), PI),
        ] {
            let transform = AffineTransform(flattened_parent * Mat4::from_quat(child));
            assert_unit_and_finite(transform.rotation());
        }
    }
}
