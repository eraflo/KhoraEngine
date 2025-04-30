use super::vector::{Vec3, Vec4};
use std::ops::Mul;


#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Mat4 {
    pub cols: [Vec4; 4],
}

impl Mat4 {
    
    /// Create the Identity matrix.
    pub const IDENTITY: Self = Self {
        cols: [
            Vec4::X,
            Vec4::Y,
            Vec4::Z,
            Vec4::W
        ],
    };

    /// Create a constant 0 matrix.
    /// This is a 4x4 matrix with all elements set to 0, except the last element which is set to 1.
    /// This is useful for representing a zero transformation in 3D space.
    pub const ZERO: Self = Self { cols: [Vec4::ZERO; 4] };

    /// Create a new matrix from 4 columns.
    /// # Arguments
    /// * `c0` - The first column of the matrix.
    /// * `c1` - The second column of the matrix.
    /// * `c2` - The third column of the matrix.
    /// * `c3` - The fourth column of the matrix.
    /// # Returns
    /// * A new matrix with the given columns.
    #[inline]
    pub fn from_cols(c0: Vec4, c1: Vec4, c2: Vec4, c3: Vec4) -> Self {
        Self { cols: [c0, c1, c2, c3] }
    }

    /// Create a translation matrix.
    /// # Arguments
    /// * `translation` - The translation vector.
    /// # Returns
    /// * A new translation matrix.
    #[inline]
    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            cols: [
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(translation.x, translation.y, translation.z, 1.0),
            ],
        }
    }

    /// Create a scaling matrix.
    /// # Arguments
    /// * `scale` - The scaling vector.
    /// # Returns
    /// * A new scaling matrix.
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

    /// Create a rotation matrix around the X axis.
    /// # Arguments
    /// * `angle` - The angle in radians.
    /// # Returns
    /// * A new rotation matrix.
    #[inline]
    pub fn from_rotation_x(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, c, -s, 0.0),
                Vec4::new(0.0, s, c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Create a rotation matrix around the Y axis.
    /// # Arguments
    /// * `angle` - The angle in radians.
    /// # Returns
    /// * A new rotation matrix.
    #[inline]
    pub fn from_rotation_y(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(c, 0.0, s, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(-s, 0.0, c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Create a rotation matrix around the Z axis.
    /// # Arguments
    /// * `angle` - The angle in radians.
    /// # Returns
    /// * A new rotation matrix.
    #[inline]
    pub fn from_rotation_z(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            cols: [
                Vec4::new(c, -s, 0.0, 0.0),
                Vec4::new(s, c, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Returns the rotation matrix from the given axis and angle.
    /// # Arguments
    /// * `axis` - The axis of rotation.
    /// * `angle` - The angle in radians.
    /// # Returns
    /// * A new rotation matrix.
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let c = angle.cos(); // Cosine of the angle
        let s = angle.sin(); // Sine of the angle
        let t = 1.0 - c; // 1 - Cosine of the angle
        let x = axis.x; // X component of the axis
        let y = axis.y; // Y component of the axis
        let z = axis.z; // Z component of the axis

        Self {
            // Create the rotation matrix using the axis-angle formula
            cols: [
                Vec4::new(t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0),
                Vec4::new(t * y * x + s * z, t * y * y + c, t * y * z - s * x, 0.0),
                Vec4::new(t * z * x - s * y, t * z * y + s * x, t * z * z + c, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Creates a right-handed perspective projection matrix with a depth range of [0, 1].
    /// # Arguments
    /// * `fov_y_radians`: Vertical field of view in radians.
    /// * `aspect_ratio`: Width divided by height of the viewport.
    /// * `z_near`: Distance to the near clipping plane (must be positive).
    /// * `z_far`: Distance to the far clipping plane (must be positive and > z_near).
    /// # Returns
    /// * A new perspective projection matrix.
    #[inline]
    pub fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Self {
        assert!(z_near > 0.0 && z_far > z_near, "z_near must be > 0, z_far must be > z_near");

        let tan_half_fovy = (fov_y_radians / 2.0).tan();
        let f = 1.0 / tan_half_fovy;
        let aa = f / aspect_ratio;
        let bb = f;
        let cc = z_far / (z_near - z_far);       // zero-to-one depth mapping
        let dd = (z_near * z_far) / (z_near - z_far); // zero-to-one depth mapping

        Self::from_cols(
            Vec4::new(aa, 0.0, 0.0, 0.0),
            Vec4::new(0.0, bb, 0.0, 0.0),
            Vec4::new(0.0, 0.0, cc, -1.0), // Note the -1.0 in W for RH perspective
            Vec4::new(0.0, 0.0, dd, 0.0),
        )
    }

    /// Creates a right-handed orthographic projection matrix with a depth range of [0, 1].
    /// # Arguments
    /// * `left`: Left clipping plane.
    /// * `right`: Right clipping plane.
    /// * `bottom`: Bottom clipping plane.
    /// * `top`: Top clipping plane.
    /// * `z_near`: Distance to the near clipping plane (must be positive).
    /// * `z_far`: Distance to the far clipping plane (must be positive and > z_near).
    /// # Returns
    /// * A new orthographic projection matrix.
    #[inline]
    pub fn orthographic_rh_zo(left: f32, right: f32, bottom: f32, top: f32, z_near: f32, z_far: f32) -> Self {
        let rml = right - left; // rml : right minus left
        let rpl = right + left; // rpl : right plus left
        let tmb = top - bottom; // tmb : top minus bottom
        let tpb = top + bottom; // tpb : top plus bottom
        let fmn = z_far - z_near; // fmn : far minus near

        let aa = 2.0 / rml; // compute the scale factor for x-axis
        let bb = 2.0 / tmb; // compute the scale factor for y-axis
        let cc = -1.0 / fmn; // zero-to-one depth mapping
        let dd = -rpl / rml; // compute the translation factor for x-axis
        let ee = -tpb / tmb; // compute the translation factor for y-axis
        let ff = -z_near / fmn; // zero-to-one depth mapping

        // Create the orthographic projection matrix
        Self::from_cols(
            Vec4::new(aa, 0.0, 0.0, 0.0),
            Vec4::new(0.0, bb, 0.0, 0.0),
            Vec4::new(0.0, 0.0, cc, 0.0),
            Vec4::new(dd, ee, ff, 1.0),
        )
    }

    /// Creates a right-handed view matrix for a camera looking at a target point.
    /// # Arguments
    /// * `eye`: The position of the camera in world space.
    /// * `target`: The point in world space that the camera is looking at.
    /// * `up`: The up direction of the camera in world space.
    /// # Returns
    /// * A new view matrix.
    /// # Note
    /// * The `up` vector should be normalized. If it is not, the resulting matrix may not be orthogonal.
    /// * The `target` vector should not be equal to the `eye` vector. If they are equal, the resulting matrix will be invalid.
    #[inline]
    pub fn look_at_rh(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let f = (target - eye).normalize(); // Forward (negative Z axis of camera)
        let s = f.cross(up).normalize();   // Right (X axis of camera)
        let u = s.cross(f);                // Up (Y axis of camera)

        // The view matrix is the inverse of the camera's transformation matrix. (T * R).
        // The camera's transformation matrix is a combination of translation (T) and rotation (R).
        // The inverse of a rotation matrix is its transpose. (R^T).
        // The inverse of a translation matrix is the negation of the translation vector. (T^-1 = -T).
        // Therefore, the inverse of the camera's transformation matrix is:
        // Inv(T * R) = Inv(R) * Inv(T) = Transpose(R) * (-T).
        // The view matrix is constructed by taking the transpose of the rotation matrix and applying the negation of the translation vector.
        // The resulting matrix is a right-handed view matrix.
        // The last column of the view matrix is the negation of the translation vector, which is the position of the camera in world space.
        // The last row of the view matrix is the homogeneous coordinate, which is set to 1.0.
        // The resulting matrix is a 4x4 matrix that transforms points from world space to camera space.
        // Final view matrix formula:
        // ViewMatrix = Transpose(R) * Inv(T)
        // where R is the rotation matrix and T is the translation matrix.
        Self::from_cols(
            Vec4::new(s.x, u.x, -f.x, 0.0), // Row 0 of Transpose(R)
            Vec4::new(s.y, u.y, -f.y, 0.0), // Row 1 of Transpose(R)
            Vec4::new(s.z, u.z, -f.z, 0.0), // Row 2 of Transpose(R)
            Vec4::new(-eye.dot(s), -eye.dot(u), eye.dot(f), 1.0), // Apply Inv(T)
        )
    }

    /// Returns the transpose of the matrix.
    /// The transpose of a matrix is obtained by swapping its rows and columns.
    /// # Returns
    /// * A new matrix that is the transpose of the original matrix.
    #[inline]
    pub fn transpose(&self) -> Self {
        Self::from_cols(
            Vec4::new(self.cols[0].x, self.cols[1].x, self.cols[2].x, self.cols[3].x),
            Vec4::new(self.cols[0].y, self.cols[1].y, self.cols[2].y, self.cols[3].y),
            Vec4::new(self.cols[0].z, self.cols[1].z, self.cols[2].z, self.cols[3].z),
            Vec4::new(self.cols[0].w, self.cols[1].w, self.cols[2].w, self.cols[3].w),
        )
    }

    /// Returns the determinant of the matrix.
    /// The determinant is a scalar value that can be used to determine if the matrix is invertible.
    /// # Returns
    /// * The determinant of the matrix.
    pub fn determinant(&self) -> f32 {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        let c3 = self.cols[3];

        let m00 = c1.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c1.z * c3.w - c3.z * c1.w) + c3.y * (c1.z * c2.w - c2.z * c1.w);
        let m01 = c0.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c0.z * c3.w - c3.z * c0.w) + c3.y * (c0.z * c2.w - c2.z * c0.w);
        let m02 = c0.y * (c1.z * c3.w - c3.z * c1.w) - c1.y * (c0.z * c3.w - c3.z * c0.w) + c3.y * (c0.z * c1.w - c1.z * c0.w);
        let m03 = c0.y * (c1.z * c2.w - c2.z * c1.w) - c1.y * (c0.z * c2.w - c2.z * c0.w) + c2.y * (c0.z * c1.w - c1.z * c0.w);

        c0.x * m00 - c1.x * m01 + c2.x * m02 - c3.x * m03
    }

    /// Returns the inverse of the matrix.
    /// /// The inverse of a matrix is a matrix that, when multiplied with the original matrix, yields the identity matrix.
    /// # Returns
    /// * An `Option<Self>` that is `Some` if the matrix is invertible, or `None` if it is not.
    pub fn inverse(&self) -> Option<Self> {
        let c0 = self.cols[0];
        let c1 = self.cols[1];
        let c2 = self.cols[2];
        let c3 = self.cols[3];

        // Compute cofactors (elements of the adjugate matrix's transpose)
        let a00 = c1.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c1.z * c3.w - c3.z * c1.w) + c3.y * (c1.z * c2.w - c2.z * c1.w);
        let a01 = -(c1.x * (c2.z * c3.w - c3.z * c2.w) - c2.x * (c1.z * c3.w - c3.z * c1.w) + c3.x * (c1.z * c2.w - c2.z * c1.w));
        let a02 = c1.x * (c2.y * c3.w - c3.y * c2.w) - c2.x * (c1.y * c3.w - c3.y * c1.w) + c3.x * (c1.y * c2.w - c2.y * c1.w);
        let a03 = -(c1.x * (c2.y * c3.z - c3.y * c2.z) - c2.x * (c1.y * c3.z - c3.y * c1.z) + c3.x * (c1.y * c2.z - c2.y * c1.z));

        let a10 = -(c0.y * (c2.z * c3.w - c3.z * c2.w) - c2.y * (c0.z * c3.w - c3.z * c0.w) + c3.y * (c0.z * c2.w - c2.z * c0.w));
        let a11 = c0.x * (c2.z * c3.w - c3.z * c2.w) - c2.x * (c0.z * c3.w - c3.z * c0.w) + c3.x * (c0.z * c2.w - c2.z * c0.w);
        let a12 = -(c0.x * (c2.y * c3.w - c3.y * c2.w) - c2.x * (c0.y * c3.w - c3.y * c0.w) + c3.x * (c0.y * c2.w - c2.y * c0.w));
        let a13 = c0.x * (c2.y * c3.z - c3.y * c2.z) - c2.x * (c0.y * c3.z - c3.y * c1.z) + c3.x * (c0.y * c2.z - c2.y * c0.z);

        let a20 = c0.y * (c1.z * c3.w - c3.z * c1.w) - c1.y * (c0.z * c3.w - c3.z * c0.w) + c3.y * (c0.z * c1.w - c1.z * c0.w);
        let a21 = -(c0.x * (c1.z * c3.w - c3.z * c1.w) - c1.x * (c0.z * c3.w - c3.z * c0.w) + c3.x * (c0.z * c1.w - c1.z * c0.w));
        let a22 = c0.x * (c1.y * c3.w - c3.y * c1.w) - c1.x * (c0.y * c3.w - c3.y * c0.w) + c3.x * (c0.y * c1.w - c1.y * c0.w);
        let a23 = -(c0.x * (c1.y * c3.z - c3.y * c1.z) - c1.x * (c0.y * c3.z - c3.y * c0.z) + c3.x * (c0.y * c1.z - c1.y * c0.z));

        let a30 = -(c0.y * (c1.z * c2.w - c2.z * c1.w) - c1.y * (c0.z * c2.w - c2.z * c0.w) + c2.y * (c0.z * c1.w - c1.z * c0.w));
        let a31 = c0.x * (c1.z * c2.w - c2.z * c1.w) - c1.x * (c0.z * c2.w - c2.z * c0.w) + c2.x * (c0.z * c1.w - c1.z * c0.w);
        let a32 = -(c0.x * (c1.y * c2.w - c2.y * c1.w) - c1.x * (c0.y * c2.w - c2.y * c0.w) + c2.x * (c0.y * c1.w - c1.y * c0.w));
        let a33 = c0.x * (c1.y * c2.z - c2.y * c1.z) - c1.x * (c0.y * c2.z - c2.y * c0.z) + c2.x * (c0.y * c1.z - c1.y * c0.z);

        let det = c0.x * a00 + c1.x * a10 + c2.x * a20 + c3.x * a30;

        if det.abs() < f32::EPSILON { // Check if determinant is close to zero
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


    /// Calculates the inverse of an affine transformation matrix (composed of Translate, Rotate, Scale).
    /// Faster and more numerically stable than general inverse for this common case.
    /// # Arguments
    /// * `self` - The affine transformation matrix.
    /// # Returns
    /// * `None` if the scaling part is zero (singular) else `Some(Self)` with the inverse matrix.
    #[inline]
    pub fn affine_inverse(&self) -> Option<Self> {
        // Extract upper 3x3 (rotation/scale part) and translation part
        let c0 = self.cols[0].truncate(); // Vec3
        let c1 = self.cols[1].truncate(); // Vec3
        let c2 = self.cols[2].truncate(); // Vec3
        let translation = self.cols[3].truncate(); // Vec3

        // Calculate determinant of upper 3x3
        let det3x3 = c0.x * (c1.y * c2.z - c2.y * c1.z) -
                     c1.x * (c0.y * c2.z - c2.y * c0.z) +
                     c2.x * (c0.y * c1.z - c1.y * c0.z);

        if det3x3.abs() < 1e-8 { return None; } // Singular if scale is zero

        let inv_det3x3 = 1.0 / det3x3;

        // Calculate inverse of upper 3x3 using cofactors
        let inv00 = (c1.y * c2.z - c2.y * c1.z) * inv_det3x3;
        let inv10 = (c2.y * c0.z - c0.y * c2.z) * inv_det3x3;
        let inv20 = (c0.y * c1.z - c1.y * c0.z) * inv_det3x3;

        let inv01 = (c2.x * c1.z - c1.x * c2.z) * inv_det3x3;
        let inv11 = (c0.x * c2.z - c2.x * c0.z) * inv_det3x3;
        let inv21 = (c1.x * c0.z - c0.x * c1.z) * inv_det3x3;

        let inv02 = (c1.x * c2.y - c2.x * c1.y) * inv_det3x3;
        let inv12 = (c2.x * c0.y - c0.x * c2.y) * inv_det3x3;
        let inv22 = (c0.x * c1.y - c1.x * c0.y) * inv_det3x3;

        // Inverse translation = - (Inverse(Upper3x3) * Translation)
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

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// --- Operators Overloading ---

/// Matrix * Matrix multiplication.
impl Mul<Mat4> for Mat4 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Mat4) -> Self::Output {
        // Result column j = self * rhs column j
        Self::from_cols(
            self * rhs.cols[0],
            self * rhs.cols[1],
            self * rhs.cols[2],
            self * rhs.cols[3],
        )
    }
}

/// Matrix * Vec4 multiplication (transforming a point/vector).
impl Mul<Vec4> for Mat4 {
    type Output = Vec4;
    #[inline]
    fn mul(self, rhs: Vec4) -> Self::Output {
        // Based on column-major definition: result = col0*x + col1*y + col2*z + col3*w
        self.cols[0] * rhs.x + self.cols[1] * rhs.y + self.cols[2] * rhs.z + self.cols[3] * rhs.w
    }
}



// --- Tests ---


#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    // Helper for approximate float equality comparison
    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5 // Slightly larger epsilon for matrix ops
    }

    fn vec4_approx_eq(a: Vec4, b: Vec4) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z) && approx_eq(a.w, b.w)
    }


    fn mat4_approx_eq(a: Mat4, b: Mat4) -> bool {
        vec4_approx_eq(a.cols[0], b.cols[0]) &&
        vec4_approx_eq(a.cols[1], b.cols[1]) &&
        vec4_approx_eq(a.cols[2], b.cols[2]) &&
        vec4_approx_eq(a.cols[3], b.cols[3])
    }

    #[test]
    fn test_identity() {
        assert_eq!(Mat4::default(), Mat4::IDENTITY);
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert!(mat4_approx_eq(m * Mat4::IDENTITY, m));
        assert!(mat4_approx_eq(Mat4::IDENTITY * m, m));
    }

    #[test]
    fn test_translation() {
        let t = Vec3::new(1.0, 2.0, 3.0);
        let m = Mat4::from_translation(t);
        let p = Vec4::new(10.0, 11.0, 12.0, 1.0); // Point
        let v = Vec4::new(10.0, 11.0, 12.0, 0.0); // Vector

        let expected_p = Vec4::new(11.0, 13.0, 15.0, 1.0);
        let expected_v = Vec4::new(10.0, 11.0, 12.0, 0.0); // Translation shouldn't affect vectors

        assert!(vec4_approx_eq(m * p, expected_p));
        assert!(vec4_approx_eq(m * v, expected_v));
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
        let expected_p = Vec4::new(0.0, 0.0, -1.0, 1.0); // Should rotate to -Z axis in RH
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
        assert!(mat4_approx_eq(identity, Mat4::IDENTITY), "M * inv(M) should be Identity");

        // Check singular matrix (e.g., scale with zero)
        let singular = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
        assert!(singular.inverse().is_none(), "Singular matrix inverse should be None");
    }

    #[test]
    fn test_affine_inverse() {
        let t = Mat4::from_translation(Vec3::new(1., 2., 3.));
        let r = Mat4::from_rotation_y(PI / 3.0);
        let s = Mat4::from_scale(Vec3::new(1., 2., 0.5));
        let m = t * r * s; // Combined affine transform

        let inv_m = m.inverse().expect("Matrix should be invertible");
        let affine_inv_m = m.affine_inverse().expect("Matrix should be affine invertible");

        // Check if affine inverse matches general inverse for this case
        assert!(mat4_approx_eq(inv_m, affine_inv_m), "Affine inverse should match general inverse");

        // Check M * inv(M) == Identity using affine inverse
        let identity = m * affine_inv_m;
        assert!(mat4_approx_eq(identity, Mat4::IDENTITY), "M * affine_inv(M) should be Identity");

        // Test singular affine matrix
        let singular_s = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
        let singular_m = t * singular_s;
        assert!(singular_m.affine_inverse().is_none(), "Singular affine matrix inverse should be None");
    }

    // Add tests for perspective_rh_zo, orthographic_rh_zo, look_at_rh
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
        assert!(approx_eq(m.cols[0].x, 1.0 / (right - left)));
        assert!(approx_eq(m.cols[1].y, 1.0 / (top - bottom)));
        assert!(approx_eq(m.cols[2].z, -2.0 / (far - near)));
        assert!(approx_eq(m.cols[3].z, -(far + near) / (far - near)));
    }

    #[test]
    fn test_look_at_rh() {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let target = Vec3::new(0.0, 0.0, 0.0);
        let up = Vec3::new(0.0, 1.0, 0.0);

        let m = Mat4::look_at_rh(eye, target, up);
        assert!(approx_eq(m.cols[2].z, -1.0)); // Forward direction
        assert!(approx_eq(m.cols[3].z, -5.0)); // Translation
    }

    #[test]
    fn test_look_at_rh_invalid() {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let target = Vec3::new(0.0, 0.0, 5.0); // Same as eye
        let up = Vec3::new(0.0, 1.0, 0.0);

        // This should panic or return None (depending on implementation)
        assert!(Mat4::look_at_rh(eye, target, up).cols[3].z.is_nan());
    }
}