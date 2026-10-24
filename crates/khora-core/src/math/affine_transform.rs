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

/// Two unit directions whose cross product is shorter than this are taken as
/// parallel: [`AffineTransform::rotation`] cannot draw a perpendicular from
/// one to the other.
const PARALLEL_SINE: f32 = 1e-6;

/// An axis shorter than this fraction of the longest one carries no
/// direction: f32 matrix products leave a residue of a few units in the last
/// place (~1e-7) of the longest axis in an axis meant to be zero — a quarter
/// turn under a parent flattened to zero leaves one — so below this neither
/// its direction nor its sign is information, only rounding.
const NOISE_FLOOR: f32 = 1e-6;

/// `v` made unit whatever its magnitude — `None` when it is zero or not
/// finite.
///
/// Divided by its largest component first, `v` neither underflows nor
/// overflows when squared (`Vec3::normalize` also gives up below an absolute
/// length), so a millimetre axis is as good as a metre one.
fn unit(v: Vec3) -> Option<Vec3> {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return None;
    }
    let largest = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if largest == 0.0 {
        return None;
    }
    // Component by component: `Vec3 / f32` multiplies by the reciprocal,
    // which is infinite for a subnormal `largest`.
    let v = Vec3::new(v.x / largest, v.y / largest, v.z / largest);
    Some(v / v.length())
}

/// The part of the unit vector `v` perpendicular to the unit vector `onto`,
/// made unit — `None` when `v` is (nearly) parallel to `onto`.
fn unit_rejection(v: Vec3, onto: Vec3) -> Option<Vec3> {
    let rejection = v - onto * v.dot(onto);
    if rejection.length() <= PARALLEL_SINE {
        return None;
    }
    unit(rejection)
}

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
    /// How small a scale is never matters, only how the axes compare: an axis
    /// keeps its direction, and its mirror is read from it, down to a
    /// millionth of the longest axis. Below that it is f32 rounding residue
    /// (a quarter turn under a parent flattened to zero leaves one), with no
    /// direction or sign to read; it is rebuilt as the right-handed cross
    /// product of the two others, so a transform squashed flat keeps its
    /// rotation (a mirror on the surviving axes then has nothing to be read
    /// from and turns into a half turn). Two such axes, two parallel
    /// survivors, or a non-finite basis leave no rotation to recover and
    /// yield the identity.
    ///
    /// A sheared basis — a rotated child under a non-uniformly scaled parent
    /// — has no rotation that reproduces it, and the angle its author gave the
    /// child cannot be read back from the world matrix. The rotation keeps the
    /// forward (Z) axis exactly, then the up (Y) axis made perpendicular to
    /// it: a camera looks exactly where its forward axis points. For an
    /// orthogonal basis this changes nothing.
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
        let Some(axes) = self.unit_basis() else {
            return Quaternion::IDENTITY;
        };
        // Orthonormalised forward first: a sheared basis (a rotated child
        // under a non-uniformly scaled parent) keeps exactly where its forward
        // axis points — where a camera looks — and its up as near as a
        // perpendicular allows. The right axis is derived, which puts any
        // mirror on it. Up parallel to forward leaves the twist to the right
        // axis instead. Each pair is made perpendicular twice, so the basis
        // stays orthonormal even when up nearly folds onto forward.
        let [right, up, forward] = axes;
        let right = if let Some(up) = unit_rejection(up, forward) {
            unit(up.cross(forward))
        } else {
            unit_rejection(right, forward)
                .and_then(|right| unit(forward.cross(right)))
                .and_then(|up| unit(up.cross(forward)))
        };
        let Some(right) = right else {
            return Quaternion::IDENTITY;
        };
        let up = forward.cross(right);
        let basis = Mat4::from_cols(
            Vec4::new(right.x, right.y, right.z, 0.0),
            Vec4::new(up.x, up.y, up.z, 0.0),
            Vec4::new(forward.x, forward.y, forward.z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
        Quaternion::from_rotation_matrix(&basis)
    }

    /// The unit world direction a local direction points along.
    ///
    /// This is the linear part of the transform (rotation and scale, never
    /// the translation) applied to `local`, normalised: under a non-uniform
    /// scale a direction leans the way the stretched basis leans, exactly as
    /// the geometry it lights or looks at does, however short `local` or the
    /// axes are. Only when that image is no longer than the basis's rounding
    /// residue (`local` lies along an axis scaled to zero, see
    /// [`rotation`](Self::rotation)) or the basis is not finite is the
    /// direction `rotation()` applied to `local`. A zero or non-finite `local`
    /// gives zero.
    #[inline]
    pub fn transform_direction(&self, local: Vec3) -> Vec3 {
        let Some(local) = unit(local) else {
            return Vec3::ZERO;
        };
        let columns = [self.right(), self.up(), self.forward()];
        let components = columns.iter().flat_map(|c| [c.x, c.y, c.z]);
        if components.clone().any(|c| !c.is_finite()) {
            return self.rotation() * local;
        }
        // Summed on the basis divided by its largest component, the image
        // neither overflows nor underflows; its direction is unchanged.
        let largest = components.fold(0.0f32, |largest, c| largest.max(c.abs()));
        if largest == 0.0 {
            return self.rotation() * local;
        }
        let columns = columns.map(|c| Vec3::new(c.x / largest, c.y / largest, c.z / largest));
        let image = columns[0] * local.x + columns[1] * local.y + columns[2] * local.z;
        let longest = columns.iter().map(|c| c.length()).fold(0.0f32, f32::max);
        // An image at the rounding residue of the basis points nowhere
        // (`local` lies along an axis scaled to zero).
        if image.length() <= NOISE_FLOOR * longest {
            return self.rotation() * local;
        }
        unit(image).unwrap_or_else(|| self.rotation() * local)
    }

    /// The three basis axes made unit, an axis scaled to exactly zero rebuilt
    /// from the two others — `None` when the basis is not finite, two axes
    /// are zero, or the survivors of a zero axis are parallel.
    fn unit_basis(&self) -> Option<[Vec3; 3]> {
        let columns = [self.right(), self.up(), self.forward()];
        if columns
            .iter()
            .any(|c| !(c.x.is_finite() && c.y.is_finite() && c.z.is_finite()))
        {
            return None;
        }
        // Lengths compared on the basis divided by its largest component, so
        // the comparison neither underflows nor overflows.
        let largest = columns
            .iter()
            .flat_map(|c| [c.x.abs(), c.y.abs(), c.z.abs()])
            .fold(0.0f32, f32::max);
        if largest == 0.0 {
            return None;
        }
        let lengths =
            columns.map(|c| Vec3::new(c.x / largest, c.y / largest, c.z / largest).length());
        let longest = lengths[0].max(lengths[1]).max(lengths[2]);
        let units: [Option<Vec3>; 3] = std::array::from_fn(|i| {
            (lengths[i] > NOISE_FLOOR * longest)
                .then(|| unit(columns[i]))
                .flatten()
        });
        let mut zero = units.iter().enumerate().filter(|(_, u)| u.is_none());
        match (zero.next(), zero.next()) {
            (None, _) => Some(units.map(|u| u.unwrap_or(Vec3::ZERO))),
            (Some((missing, _)), None) => {
                // An axis scaled to (nearly) zero has no direction, and no
                // sign to read a mirror from, but the two others
                // still carry the rotation; it is their right-handed cross
                // product (right = up × forward, up = forward × right,
                // forward = right × up).
                let next = units[(missing + 1) % 3]?;
                let after = units[(missing + 2) % 3]?;
                let rebuilt = next.cross(after);
                if rebuilt.length() <= PARALLEL_SINE {
                    return None;
                }
                let mut axes = units.map(|u| u.unwrap_or(Vec3::ZERO));
                axes[missing] = unit(rebuilt)?;
                Some(axes)
            }
            (Some(_), Some(_)) => None,
        }
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
    /// basis its handedness) down to the noise floor, a millionth of the
    /// longest axis, so shrinking it from a thousandth to a hundred-thousandth
    /// must not turn the rotation over by a half turn.
    #[test]
    fn rotation_of_a_mirrored_transform_does_not_turn_over_as_an_axis_nearly_flattens() {
        let rotation = oblique_rotation();
        for (above, below) in [
            (Vec3::new(-1.0, 1.0, 1e-3), Vec3::new(-1.0, 1.0, 1e-5)),
            (Vec3::new(1.0, -1.0, 1e-3), Vec3::new(1.0, -1.0, 1e-5)),
            (Vec3::new(1.0, 1.0, -1e-3), Vec3::new(1.0, 1.0, -1e-5)),
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

    // --- A sheared basis keeps where it points ------------------------------

    /// Asserts two directions agree within 1e-5, component by component.
    fn assert_direction_is(actual: Vec3, expected: Vec3, what: &str) {
        let error = (actual - expected).length();
        assert!(
            error < 1e-5,
            "{what}: {actual:?} where {expected:?} was expected (off by {error})"
        );
    }

    /// Asserts the rotation of `transform` takes local +Z exactly onto the
    /// normalised world Z column (a camera looks where its axis points), and
    /// local +Y into the plane of the world Y and Z columns, perpendicular
    /// to Z, on Y's side.
    fn assert_forward_exact(transform: AffineTransform, what: &str) {
        let extracted = transform.rotation();
        assert_unit_and_finite(extracted);
        let turned = Mat4::from_quat(extracted);
        let world_y = transform.up();
        let world_z = transform.forward();
        let forward = world_z.normalize();

        assert_direction_is(
            turned.transform_vector(Vec3::Z),
            forward,
            &format!("{what}: +Z must land on the world Z column"),
        );

        let up = turned.transform_vector(Vec3::Y);
        let plane_normal = world_y.cross(world_z).normalize();
        assert!(
            up.dot(plane_normal).abs() < 1e-5,
            "{what}: +Y turned to {up:?}, out of the plane of the world Y {world_y:?} \
             and Z {world_z:?} columns"
        );
        assert!(
            up.dot(forward).abs() < 1e-5,
            "{what}: +Y turned to {up:?}, not perpendicular to the forward {forward:?}"
        );
        assert!(
            up.dot(world_y) > 0.0,
            "{what}: +Y turned to {up:?}, away from the world Y column {world_y:?}"
        );
    }

    /// A child pitched 30° under a parent stretched to twice its height has a
    /// sheared world basis: its Y and Z columns are no longer perpendicular.
    /// The rotation keeps its forward exact — a camera there looks where its
    /// world Z axis points — and its up in the plane the Y and Z columns span.
    #[test]
    fn rotation_of_a_pitched_child_under_a_stretched_parent_keeps_its_forward_exact() {
        let parent = Mat4::from_scale(Vec3::new(1.0, 2.0, 1.0));
        let child = Mat4::from_quat(Quaternion::from_axis_angle(Vec3::X, 30f32.to_radians()));
        let transform = AffineTransform(parent * child);
        assert!(
            transform.up().dot(transform.forward()).abs() > 0.1,
            "the basis under test must be sheared"
        );

        assert_forward_exact(transform, "30° pitch under a (1, 2, 1) parent");
    }

    /// A strong shear — an obliquely turned child under a parent scaled
    /// (1, 5, 0.3) — still gives a unit, finite rotation whose forward is
    /// exactly the world Z column.
    #[test]
    fn rotation_under_a_strong_shear_keeps_its_forward_exact() {
        let parent = Mat4::from_scale(Vec3::new(1.0, 5.0, 0.3));
        let transform = AffineTransform(
            Mat4::from_translation(Vec3::new(2.0, -1.0, 3.0))
                * parent
                * Mat4::from_quat(oblique_rotation()),
        );

        assert_forward_exact(transform, "oblique child under a (1, 5, 0.3) parent");
    }

    /// When the Y column is parallel to the Z column, Y says nothing about
    /// the twist around the forward axis: the forward stays exact and the X
    /// column, made perpendicular to it, gives the twist.
    #[test]
    fn rotation_with_up_parallel_to_forward_takes_its_twist_from_the_right_axis() {
        let world_x = Vec3::new(1.0, 0.5, -0.2);
        let world_z = Vec3::new(0.3, -0.4, 1.0);
        let world_y = world_z * 2.0;
        let transform = AffineTransform(Mat4::from_cols(
            Vec4::new(world_x.x, world_x.y, world_x.z, 0.0),
            Vec4::new(world_y.x, world_y.y, world_y.z, 0.0),
            Vec4::new(world_z.x, world_z.y, world_z.z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ));

        let extracted = transform.rotation();
        assert_unit_and_finite(extracted);
        let turned = Mat4::from_quat(extracted);
        let forward = world_z.normalize();
        let right = (world_x - forward * world_x.dot(forward)).normalize();

        assert_direction_is(
            turned.transform_vector(Vec3::Z),
            forward,
            "+Z lands on the world Z column",
        );
        assert_direction_is(
            turned.transform_vector(Vec3::X),
            right,
            "+X lands on the world X column made perpendicular to Z",
        );
        assert_direction_is(
            turned.transform_vector(Vec3::Y),
            forward.cross(right),
            "+Y completes a right-handed basis",
        );
    }

    // --- transform_direction ------------------------------------------------

    /// Under a rotation alone (the translation aside), a direction turns
    /// exactly as the rotation turns it.
    #[test]
    fn transform_direction_under_a_rotation_alone_is_the_rotated_direction() {
        let rotation = oblique_rotation();
        let transform = trs(Vec3::new(4.0, -2.0, 7.0), rotation, Vec3::ONE);
        for local in [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::NEG_Y,
            Vec3::new(1.0, -1.0, 2.0).normalize(),
        ] {
            assert_direction_is(
                transform.transform_direction(local),
                rotation * local,
                &format!("local {local:?} under a rotation"),
            );
        }
    }

    /// Under a non-uniform scale an off-axis direction leans with the
    /// stretched basis: it is the linear part applied to it, normalised — not
    /// the rotation applied to it.
    #[test]
    fn transform_direction_under_a_non_uniform_scale_leans_with_the_basis() {
        let local = Vec3::new(1.0, -1.0, 0.0).normalize();
        for transform in [
            trs(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(2.0, 1.0, 1.0)),
            trs(
                Vec3::new(-3.0, 1.0, 0.5),
                oblique_rotation(),
                Vec3::new(2.0, 1.0, 1.0),
            ),
        ] {
            let expected = transform.0.transform_vector(local).normalize();
            assert_direction_is(
                transform.transform_direction(local),
                expected,
                "an off-axis direction under a (2, 1, 1) scale",
            );
        }
        assert_direction_is(
            trs(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(2.0, 1.0, 1.0))
                .transform_direction(local),
            Vec3::new(2.0, -1.0, 0.0).normalize(),
            "(1, -1, 0) under a (2, 1, 1) scale",
        );
    }

    /// A direction along a flattened axis has no image under the linear part;
    /// it still points somewhere: the rotation applied to it, unit and
    /// finite.
    #[test]
    fn transform_direction_along_a_flattened_axis_is_the_rotated_direction() {
        let transform = trs(
            Vec3::new(1.0, 2.0, 3.0),
            oblique_rotation(),
            Vec3::new(1.0, 1.0, 0.0),
        );

        let direction = transform.transform_direction(Vec3::Z);

        assert!(
            direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite(),
            "a direction along a flattened axis must be finite, got {direction:?}"
        );
        assert!(
            (direction.length() - 1.0).abs() < 1e-5,
            "a direction along a flattened axis must be unit, got length {}",
            direction.length()
        );
        assert_direction_is(
            direction,
            transform.rotation() * Vec3::Z,
            "local Z along an axis flattened to zero",
        );
    }

    /// A zero direction has no direction to transform: it stays zero, under
    /// any transform, flattened ones included.
    #[test]
    fn transform_direction_of_zero_is_zero() {
        for transform in [
            AffineTransform::IDENTITY,
            trs(Vec3::new(1.0, 2.0, 3.0), oblique_rotation(), Vec3::ONE),
            trs(Vec3::ZERO, oblique_rotation(), Vec3::new(2.0, 1.0, 0.5)),
            trs(Vec3::ZERO, oblique_rotation(), Vec3::new(1.0, 1.0, 0.0)),
            trs(Vec3::ZERO, oblique_rotation(), Vec3::ZERO),
        ] {
            assert_eq!(
                transform.transform_direction(Vec3::ZERO),
                Vec3::ZERO,
                "zero under {transform:?}"
            );
        }
    }

    /// A basis carrying a NaN has no finite image of any direction; the
    /// direction falls back to the rotation, the identity for such a basis,
    /// rather than a NaN that spreads to every light.
    #[test]
    fn transform_direction_under_a_non_finite_basis_is_finite() {
        let transform = AffineTransform(Mat4::from_cols(
            Vec4::new(f32::NAN, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ));
        let local = Vec3::new(1.0, -1.0, 0.5).normalize();

        assert_direction_is(
            transform.transform_direction(local),
            transform.rotation() * local,
            "a direction under a NaN basis",
        );
    }

    // --- Orthogonal bases and the limits of a shear -------------------------

    /// An orthogonal basis — any non-uniform scale, a mirror on any axis or
    /// on all three, rotations near and at a half turn — keeps the rotation
    /// it had before the forward-first orthonormalisation: each column made
    /// unit, the first one negated when the basis is mirrored.
    #[test]
    fn rotation_of_an_orthogonal_basis_puts_a_mirror_on_any_axis_onto_the_first() {
        let axis = Vec3::new(1.0, 2.0, -0.5).normalize();
        for angle in [1.1f32, PI - 1e-3, PI, PI + 0.3] {
            let rotation = Quaternion::from_axis_angle(axis, angle);
            for scale in [
                Vec3::new(0.5, 3.0, 1.5),
                Vec3::new(1.0, -1.0, 1.0),
                Vec3::new(2.0, 1.0, -0.5),
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(-3.0, 0.25, 1.0),
                Vec3::new(1.0, -2.0, -4.0),
            ] {
                let transform = trs(Vec3::new(1.0, -2.0, 3.0), rotation, scale);
                let mut expected = [transform.right(), transform.up(), transform.forward()]
                    .map(|column| column.normalize());
                if expected[0].dot(expected[1].cross(expected[2])) < 0.0 {
                    expected[0] = -expected[0];
                }
                let extracted = transform.rotation();
                assert_unit_and_finite(extracted);
                for (local, world) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(expected) {
                    assert_direction_is(
                        extracted * local,
                        world,
                        &format!("angle {angle}, scale {scale:?}: {local:?}"),
                    );
                }
            }
        }
    }

    /// As the stretch of a parent vanishes, the rotation of its rotated child
    /// tends to the child's own: no jump as the basis turns orthogonal, and
    /// the deviation shrinks with the stretch.
    #[test]
    fn rotation_under_a_vanishing_stretch_tends_to_the_child_rotation() {
        let rotation = oblique_rotation();
        for stretch in [1e-2f32, 1e-3, 1e-4, 1e-5, 0.0] {
            let parent = Mat4::from_scale(Vec3::new(1.0, 1.0 + stretch, 1.0));
            let transform = AffineTransform(parent * Mat4::from_quat(rotation));
            let error = angle_between_degrees(transform.rotation(), rotation);
            assert!(
                error <= stretch.to_degrees() + 0.01,
                "a {stretch} stretch turned the rotation by {error}°"
            );
        }
    }

    /// The unit direction of `v`, divided by its own length: unlike
    /// `Vec3::normalize`, never zero for a short but non-zero `v`.
    fn unit(v: Vec3) -> Vec3 {
        v / v.length()
    }

    /// A camera whose forward axis shrinks toward zero under a stretched
    /// parent keeps looking down that axis: above the noise floor (a
    /// millionth of the longest axis) the shrinking column still has its own
    /// direction, and the view must not swing to the cross product of the two
    /// other, sheared, columns.
    #[test]
    fn rotation_keeps_its_forward_on_a_nearly_flattened_forward_axis_under_a_stretch() {
        let parent = Mat4::from_scale(Vec3::new(1.0, 2.0, 1.0));
        let pitch = Mat4::from_quat(Quaternion::from_axis_angle(Vec3::X, 30f32.to_radians()));
        for depth in [1e-3f32, 1e-5] {
            let transform =
                AffineTransform(parent * pitch * Mat4::from_scale(Vec3::new(1.0, 1.0, depth)));
            assert_direction_is(
                transform.rotation() * Vec3::Z,
                unit(transform.forward()),
                &format!("30° pitch, Z scaled {depth:e}, under a (1, 2, 1) parent: +Z"),
            );
        }
    }

    /// A parent squashed toward zero on one axis folds its rotated child's
    /// up and forward columns toward one line. Until they are parallel within
    /// the tolerance, the up column made perpendicular to the forward one is
    /// a short difference of two nearly equal vectors, carrying rounding
    /// error along the forward axis; the rotation still keeps its forward
    /// exactly on the world Z column at every squash.
    #[test]
    fn rotation_keeps_its_forward_exact_as_a_squashed_parent_folds_up_onto_forward() {
        let child = Quaternion::from_axis_angle(Vec3::Y, 30f32.to_radians())
            * Quaternion::from_axis_angle(Vec3::X, 45f32.to_radians());
        let (worst_squash, worst_error) = [1e-3f32, 1e-4, 1e-5, 3e-6, 1e-6, 7e-7, 5e-7, 1e-7, 0.0]
            .into_iter()
            .map(|squash| {
                let transform = AffineTransform(
                    Mat4::from_scale(Vec3::new(1.0, squash, 1.0)) * Mat4::from_quat(child),
                );
                let error = (transform.rotation() * Vec3::Z - unit(transform.forward())).length();
                (squash, error)
            })
            .fold(
                (0.0, 0.0),
                |worst, case| if case.1 > worst.1 { case } else { worst },
            );
        assert!(
            worst_error < 1e-5,
            "under a parent squashed to {worst_squash:e} on Y, +Z turns {worst_error} away \
             from the world Z column"
        );
    }

    /// A light direction too long or too short to square in f32 — components
    /// near 1e30 or below 1e-23 — is still a direction: it is the unit image
    /// of that direction, never zero.
    #[test]
    fn transform_direction_of_a_huge_or_tiny_local_is_its_unit_image() {
        let transform = trs(
            Vec3::new(1.0, 2.0, 3.0),
            oblique_rotation(),
            Vec3::new(1.0, 2.0, 0.5),
        );
        for (local, direction) in [
            (
                Vec3::new(1e30, -1e30, 2e30),
                Vec3::new(1.0, -1.0, 2.0).normalize(),
            ),
            (Vec3::new(3e38, 0.0, 0.0), Vec3::X),
            (
                Vec3::new(1e-25, 2e-25, 0.0),
                Vec3::new(1.0, 2.0, 0.0).normalize(),
            ),
            (Vec3::new(0.0, 0.0, 1e-40), Vec3::Z),
        ] {
            assert_direction_is(
                transform.transform_direction(local),
                transform.0.transform_vector(direction).normalize(),
                &format!("local {local:?}"),
            );
        }
    }

    /// A child turned a quarter turn under a parent scaled to exactly zero on
    /// one axis: the child axis the turn lays along the flattened one is
    /// zero, but the quarter turn computed in f32 leaves a residue of a few
    /// 1e-8 in it. That residue has no direction of its own; the rotation is
    /// the child's, its lost axis rebuilt from the two others, never one read
    /// from the sign of a rounding error.
    #[test]
    fn rotation_of_a_quarter_turned_child_under_a_parent_flattened_to_zero_is_that_turn() {
        for (parent_scale, child) in [
            (
                Vec3::new(1.0, 0.0, 1.0),
                Quaternion::from_axis_angle(Vec3::X, PI / 2.0),
            ),
            (
                Vec3::new(0.0, 1.0, 1.0),
                Quaternion::from_axis_angle(Vec3::Z, PI / 2.0),
            ),
        ] {
            let transform = AffineTransform(
                Mat4::from_scale(parent_scale)
                    * Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
                    * Mat4::from_quat(child),
            );
            assert_rotation_is(
                transform,
                child,
                &format!("quarter turn under a parent scaled {parent_scale:?}"),
            );
        }
    }

    /// The inverse of a turned, scaled and moved transform undoes it: the
    /// inverse composed with the transform is the identity, whatever the
    /// rotation. A parent's inverse is how a pose is taken back into the
    /// parent's frame, so an inverse that turned the wrong way would put a
    /// released body or a collider test in the wrong place.
    #[test]
    fn the_inverse_of_a_turned_transform_undoes_it() {
        for transform in [
            AffineTransform::from_rotation_z(0.9),
            trs(Vec3::new(1.0, -2.0, 3.0), oblique_rotation(), Vec3::ONE),
            trs(
                Vec3::new(10.0, -4.0, 7.0),
                oblique_rotation(),
                Vec3::new(1.0, 3.0, 0.5),
            ),
        ] {
            let inverse = transform.inverse().expect("an invertible transform");
            let product = inverse.0 * transform.0;
            for (column, expected) in product.cols.iter().zip(Mat4::IDENTITY.cols.iter()) {
                for (x, y) in [
                    (column.x, expected.x),
                    (column.y, expected.y),
                    (column.z, expected.z),
                    (column.w, expected.w),
                ] {
                    assert!(
                        (x - y).abs() < 1e-5,
                        "inverse · transform is {:?}, not the identity, for {transform:?}",
                        product.cols
                    );
                }
            }
        }
    }

    /// With up exactly parallel to forward, the twist comes from the right
    /// axis — and when that axis nearly folds onto forward too, its part
    /// perpendicular to forward is a short difference of nearly equal
    /// vectors. The rotation still keeps its forward exactly on the world Z
    /// column.
    #[test]
    fn rotation_keeps_its_forward_exact_when_right_nearly_folds_onto_a_parallel_up() {
        let forward = Vec3::new(0.3, -0.4, 1.0);
        let lean = Vec3::new(1.0, 0.5, 0.05);
        for fold in [1e-2f32, 1e-3, 1e-4, 1e-5, 3e-6, 2e-6] {
            let right = forward + lean * fold;
            let up = forward * 2.0;
            let transform = AffineTransform(Mat4::from_cols(
                Vec4::new(right.x, right.y, right.z, 0.0),
                Vec4::new(up.x, up.y, up.z, 0.0),
                Vec4::new(forward.x, forward.y, forward.z, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ));
            let extracted = transform.rotation();
            assert_unit_and_finite(extracted);
            assert_direction_is(
                extracted * Vec3::Z,
                unit(forward),
                &format!("forward with the right axis folded to {fold:e}"),
            );
        }
    }
}
