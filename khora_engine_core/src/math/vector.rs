use std::ops::{Add, Sub, Mul, Div, Neg};



// Deriving Debug, Clone, and PartialEq for convenience in debugging, cloning, and comparisons.
// The Copy trait is derived because Vec3 is a lightweight struct with no heap allocation, 
// making it safe and efficient to duplicate.
/// A simple 3D vector for mathematical operations.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };
    
    pub const X: Self = Self { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z: Self = Self { x: 0.0, y: 0.0, z: 1.0 };

    pub const NEG_X: Self = Self { x: -1.0, y: 0.0, z: 0.0 };
    pub const NEG_Y: Self = Self { x: 0.0, y: -1.0, z: 0.0 };
    pub const NEG_Z: Self = Self { x: 0.0, y: 0.0, z: -1.0 };

    pub const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const DOWN: Self = Self { x: 0.0, y: -1.0, z: 0.0 };
    pub const LEFT: Self = Self { x: -1.0, y: 0.0, z: 0.0 };
    pub const RIGHT: Self = Self { x: 1.0, y: 0.0, z: 0.0 };
    pub const FORWARD: Self = Self { x: 0.0, y: 0.0, z: 1.0 };
    pub const BACK: Self = Self { x: 0.0, y: 0.0, z: -1.0 };

    /// Creates a new Vec3 instance with the given x, y, and z components.
    /// # Arguments
    /// * `x` - The x component of the vector.
    /// * `y` - The y component of the vector.
    /// * `z` - The z component of the vector.
    /// # Returns
    /// * A new Vec3 instance with the specified components.
    #[inline]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Returns the length squared of the vector.
    /// # Returns
    /// * The squared length of the vector.
    #[inline]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Returns the length of the vector.
    /// # Returns
    /// * The length of the vector.
    #[inline]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Normalizes the vector, making it a unit vector.
    /// # Returns
    /// * A new Vec3 instance that is the normalized version of the original vector.
    /// # Panics
    /// * Panics if the vector is a zero vector (length is 0).
    #[inline]
    pub fn normalize(self) -> Self {
        let len_sq = self.length_squared();
        if len_sq > f32::EPSILON * f32::EPSILON { // Use squared length to avoid sqrt
            // Multiply by inverse sqrt for potentially better performance
            self * (1.0 / len_sq.sqrt())
        } else {
            Self::ZERO
        }
    }

    /// Returns the dot product of this vector and another vector.
    /// # Arguments
    /// * `other` - The other vector to compute the dot product with.
    /// # Returns
    /// * The dot product of the two vectors.
    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Returns the cross product of this vector and another vector.
    /// # Arguments
    /// * `other` - The other vector to compute the cross product with.
    /// # Returns
    /// * A new Vec3 instance that is the cross product of the two vectors.
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Returns the distance between this vector and another vector.
    /// # Arguments
    /// * `other` - The other vector to compute the distance to.
    /// # Returns
    /// * The distance between the two vectors.
    #[inline]
    pub fn distance(self, other: Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Returns linear interpolation between this vector and another vector.
    /// # Arguments
    /// * `other` - The other vector to interpolate with.
    /// * `t` - The interpolation factor (0.0 to 1.0).
    /// # Returns
    /// * A new Vec3 instance that is the result of the interpolation.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
    
}


// --- Operator Overloads ---

/// Implement Add for Vec3
impl Add for Vec3 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

/// Implement Sub for Vec3
impl Sub for Vec3 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

/// Scalar multiplication (Vec3 * f32)
impl Mul<f32> for Vec3 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

/// Allow scalar multiplication (f32 * Vec3)
impl Mul<Vec3> for f32 {
    type Output = Vec3;
    #[inline]
    fn mul(self, rhs: Vec3) -> Self::Output {
        rhs * self // Reuse the Vec3 * f32 implementation
    }
}

/// Component-wise multiplication (often useful for colors or scaling)
impl Mul<Vec3> for Vec3 {
     type Output = Self;
     #[inline]
     fn mul(self, rhs: Vec3) -> Self::Output {
         Self {
             x: self.x * rhs.x,
             y: self.y * rhs.y,
             z: self.z * rhs.z,
         }
     }
}

/// Component-wise division (often useful for colors or scaling)
impl Div<f32> for Vec3 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        // Consider how to handle division by zero if necessary,
        // here we rely on standard f32 division behavior (NaN/Infinity)
        let inv_rhs = 1.0 / rhs;
        Self {
            x: self.x * inv_rhs,
            y: self.y * inv_rhs,
            z: self.z * inv_rhs,
        }
    }
}

/// Implement Neg for Vec3
impl Neg for Vec3 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}


/// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*; // Import Vec3 from the parent module

    // Helper for approximate float equality comparison
    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6 // Use a small epsilon
    }

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    #[test]
    fn test_new() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(Vec3::ONE, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(Vec3::X, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(Vec3::Y, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Vec3::Z, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::UP, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Vec3::DOWN, Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(Vec3::LEFT, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(Vec3::RIGHT, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(Vec3::FORWARD, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::BACK, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn test_add() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(v1 + v2, Vec3::new(5.0, 7.0, 9.0));
    }

    #[test]
    fn test_sub() {
        let v1 = Vec3::new(5.0, 7.0, 9.0);
        let v2 = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v1 - v2, Vec3::new(4.0, 5.0, 6.0));
    }

     #[test]
     fn test_scalar_mul() {
         let v = Vec3::new(1.0, 2.0, 3.0);
         assert_eq!(v * 2.0, Vec3::new(2.0, 4.0, 6.0));
         assert_eq!(3.0 * v, Vec3::new(3.0, 6.0, 9.0)); // Test f32 * Vec3
     }

     #[test]
     fn test_component_mul() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(v1 * v2, Vec3::new(4.0, 10.0, 18.0));
     }


     #[test]
     fn test_scalar_div() {
         let v = Vec3::new(2.0, 4.0, 6.0);
         assert_eq!(v / 2.0, Vec3::new(1.0, 2.0, 3.0));
     }

     #[test]
     fn test_neg() {
         let v = Vec3::new(1.0, -2.0, 3.0);
         assert_eq!(-v, Vec3::new(-1.0, 2.0, -3.0));
     }


    #[test]
    fn test_length() {
        let v1 = Vec3::new(3.0, 4.0, 0.0);
        assert!(approx_eq(v1.length_squared(), 25.0));
        assert!(approx_eq(v1.length(), 5.0));

        let v2 = Vec3::ZERO;
        assert!(approx_eq(v2.length_squared(), 0.0));
        assert!(approx_eq(v2.length(), 0.0));
    }

    #[test]
    fn test_dot() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, -5.0, 6.0);
        // 1*4 + 2*(-5) + 3*6 = 4 - 10 + 18 = 12
        assert!(approx_eq(v1.dot(v2), 12.0));

        // Orthogonal vectors
        assert!(approx_eq(Vec3::X.dot(Vec3::Y), 0.0));
    }

    #[test]
    fn test_distance() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);
        // Distance = sqrt((4-1)^2 + (5-2)^2 + (6-3)^2) = sqrt(9 + 9 + 9) = sqrt(27) = 3*sqrt(3)
        assert!(approx_eq(v1.distance(v2), (3.0 * (3.0_f32).sqrt())));
    }

    #[test]
    fn test_cross() {
        // Standard basis vectors
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);

        // Anti-commutative property
        assert_eq!(Vec3::Y.cross(Vec3::X), -Vec3::Z);

        // Parallel vectors
        assert_eq!(Vec3::X.cross(Vec3::X), Vec3::ZERO);
    }

     #[test]
     fn test_normalize() {
         let v1 = Vec3::new(3.0, 0.0, 0.0);
         let norm_v1 = v1.normalize();
         assert!(vec3_approx_eq(norm_v1, Vec3::X));
         assert!(approx_eq(norm_v1.length(), 1.0));

         let v2 = Vec3::new(1.0, 1.0, 1.0);
         let norm_v2 = v2.normalize();
         assert!(approx_eq(norm_v2.length(), 1.0)); // Check length is 1

         // Test normalizing zero vector
         let v_zero = Vec3::ZERO;
         assert_eq!(v_zero.normalize(), Vec3::ZERO);
     }

     #[test]
    fn test_lerp() {
        let start = Vec3::new(0.0, 0.0, 0.0);
        let end = Vec3::new(10.0, 10.0, 10.0);

        assert!(vec3_approx_eq(start.lerp(end, 0.0), start));
        assert!(vec3_approx_eq(start.lerp(end, 1.0), end));
        assert!(vec3_approx_eq(start.lerp(end, 0.5), Vec3::new(5.0, 5.0, 5.0)));
    }
}