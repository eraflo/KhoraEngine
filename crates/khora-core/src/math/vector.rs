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

//! Provides 2D, 3D, and 4D vector types and their associated operations.

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::EPSILON;
use std::ops::{Add, Div, Index, IndexMut, Mul, Neg, Sub};

// --- Vec2 ---

/// A 2-dimensional vector with `f32` components.
#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    Serialize,
    Deserialize,
    Encode,
    Decode,
)]
#[repr(C)]
pub struct Vec2 {
    /// The x component of the vector.
    pub x: f32,
    /// The y component of the vector.
    pub y: f32,
}

impl Vec2 {
    /// A vector with all components set to `0.0`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    /// A vector with all components set to `1.0`.
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };
    /// The unit vector pointing along the positive X-axis.
    pub const X: Self = Self { x: 1.0, y: 0.0 };
    /// The unit vector pointing along the positive Y-axis.
    pub const Y: Self = Self { x: 0.0, y: 1.0 };

    /// Creates a new `Vec2` with the specified components.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns a new vector with the absolute value of each component.
    #[inline]
    pub const fn abs(self) -> Self {
        Self {
            x: if self.x < 0.0 { -self.x } else { self.x },
            y: if self.y < 0.0 { -self.y } else { self.y },
        }
    }

    /// Calculates the squared length (magnitude) of the vector.
    /// This is faster than `length()` as it avoids a square root.
    #[inline]
    pub fn length_squared(&self) -> f32 {
        self.dot(*self)
    }

    /// Calculates the length (magnitude) of the vector.
    #[inline]
    pub fn length(&self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized version of the vector with a length of 1.
    /// If the vector's length is near zero, it returns `Vec2::ZERO`.
    #[inline]
    pub fn normalize(&self) -> Self {
        let len_sq = self.length_squared();
        if len_sq > EPSILON * EPSILON {
            *self * (1.0 / len_sq.sqrt())
        } else {
            Self::ZERO
        }
    }

    /// Calculates the dot product of this vector and another.
    #[inline]
    pub fn dot(&self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y
    }

    /// Performs a linear interpolation between two vectors.
    /// The interpolation factor `t` is clamped to the `[0.0, 1.0]` range.
    #[inline]
    pub fn lerp(start: Self, end: Self, t: f32) -> Self {
        start + (end - start) * t.clamp(0.0, 1.0)
    }
}

// --- Operator Overloads ---

impl Add for Vec2 {
    type Output = Self;
    /// Adds two vectors component-wise.
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    /// Subtracts two vectors component-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    /// Multiplies the vector by a scalar.
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    /// Multiplies a scalar by a vector.
    #[inline]
    fn mul(self, rhs: Vec2) -> Self::Output {
        rhs * self
    }
}

impl Mul<Vec2> for Vec2 {
    type Output = Self;
    /// Multiplies two vectors component-wise.
    #[inline]
    fn mul(self, rhs: Vec2) -> Self::Output {
        Self {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    /// Divides the vector by a scalar.
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        let inv_rhs = 1.0 / rhs;
        Self {
            x: self.x * inv_rhs,
            y: self.y * inv_rhs,
        }
    }
}

impl Neg for Vec2 {
    type Output = Self;
    /// Negates the vector.
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl Index<usize> for Vec2 {
    type Output = f32;
    /// Allows accessing a vector component by index (`v[0]`, `v[1]`).
    ///
    /// # Panics
    /// Panics if `index` is not 0 or 1.
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("Index out of bounds for Vec2"),
        }
    }
}

impl IndexMut<usize> for Vec2 {
    /// Allows mutably accessing a vector component by index (`v[0] = ...`).
    ///
    /// # Panics
    /// Panics if `index` is not 0 or 1.
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => panic!("Index out of bounds for Vec2"),
        }
    }
}

// --- Vector3D ---

/// A 3-dimensional vector with `f32` components.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    Serialize,
    Deserialize,
    Encode,
    Decode,
)]
#[repr(C)]
pub struct Vec3 {
    /// The x component of the vector.
    pub x: f32,
    /// The y component of the vector.
    pub y: f32,
    /// The z component of the vector.
    pub z: f32,
}

impl Vec3 {
    /// A vector with all components set to `0.0`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// A vector with all components set to `1.0`.
    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    /// The unit vector pointing along the positive X-axis.
    pub const X: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    /// The unit vector pointing along the positive Y-axis.
    pub const Y: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    /// The unit vector pointing along the positive Z-axis.
    pub const Z: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    /// Creates a new `Vec3` with the specified components.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Returns a new vector with the absolute value of each component.
    #[inline]
    pub const fn abs(self) -> Self {
        Self {
            x: if self.x < 0.0 { -self.x } else { self.x },
            y: if self.y < 0.0 { -self.y } else { self.y },
            z: if self.z < 0.0 { -self.z } else { self.z },
        }
    }

    /// Calculates the squared length (magnitude) of the vector.
    #[inline]
    pub fn length_squared(&self) -> f32 {
        self.dot(*self)
    }

    /// Calculates the length (magnitude) of the vector.
    #[inline]
    pub fn length(&self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized version of the vector with a length of 1.
    #[inline]
    pub fn normalize(&self) -> Self {
        let len_sq = self.length_squared();
        if len_sq > EPSILON * EPSILON {
            // Use squared length to avoid sqrt
            // Multiply by inverse sqrt for potentially better performance
            *self * (1.0 / len_sq.sqrt())
        } else {
            Self::ZERO
        }
    }

    /// Calculates the dot product of this vector and another.
    #[inline]
    pub fn dot(&self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Computes the cross product of this vector and another.
    #[inline]
    pub fn cross(&self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Calculates the squared distance between this vector and another.
    #[inline]
    pub fn distance_squared(&self, other: Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }

    /// Calculates the distance between this vector and another.
    #[inline]
    pub fn distance(&self, other: Self) -> f32 {
        self.distance_squared(other).sqrt()
    }

    /// Performs a linear interpolation between two vectors.
    #[inline]
    pub fn lerp(start: Self, end: Self, t: f32) -> Self {
        Self {
            x: start.x + (end.x - start.x) * t,
            y: start.y + (end.y - start.y) * t,
            z: start.z + (end.z - start.z) * t,
        }
    }

    /// Retrieves a component of the vector by its index.
    ///
    /// # Panics
    /// Panics if `index` is not 0, 1, or 2.
    #[inline]
    pub fn get(&self, index: usize) -> f32 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => panic!("Index out of bounds for Vec3"),
        }
    }
}

// --- Operator Overloads ---

impl Default for Vec3 {
    /// Returns `Vec3::ZERO`.
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl Add for Vec3 {
    type Output = Self;
    /// Adds two vectors component-wise.
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl Sub for Vec3 {
    type Output = Self;
    /// Subtracts two vectors component-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    /// Multiplies the vector by a scalar.
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    /// Multiplies a scalar by a vector.
    #[inline]
    fn mul(self, rhs: Vec3) -> Self::Output {
        rhs * self
    }
}

impl Mul<Vec3> for Vec3 {
    type Output = Self;
    /// Multiplies two vectors component-wise.
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
            z: self.z * rhs.z,
        }
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;
    /// Divides the vector by a scalar.
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        let inv_rhs = 1.0 / rhs;
        Self {
            x: self.x * inv_rhs,
            y: self.y * inv_rhs,
            z: self.z * inv_rhs,
        }
    }
}

impl Neg for Vec3 {
    type Output = Self;
    /// Negates the vector.
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

impl Index<usize> for Vec3 {
    type Output = f32;
    /// Allows accessing a vector component by index.
    /// # Panics
    /// Panics if `index` is not 0, 1, or 2.
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Index out of bounds for Vec3"),
        }
    }
}

impl IndexMut<usize> for Vec3 {
    /// Allows mutably accessing a vector component by index.
    /// # Panics
    /// Panics if `index` is not 0, 1, or 2.
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Index out of bounds for Vec3"),
        }
    }
}

// --- Vector4D ---

/// A 4-dimensional vector with `f32` components, often used for homogeneous coordinates.
///
/// In 3D graphics, `Vec4` is primarily used to represent points (`w`=1.0) and
/// vectors (`w`=0.0) in homogeneous space, allowing them to be transformed by a `Mat4`.
#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    Serialize,
    Deserialize,
    Encode,
    Decode,
)]
#[repr(C)]
pub struct Vec4 {
    /// The x component of the vector.
    pub x: f32,
    /// The y component of the vector.
    pub y: f32,
    /// The z component of the vector.
    pub z: f32,
    /// The w component, used for homogeneous coordinates.
    pub w: f32,
}

impl Vec4 {
    /// A vector with all components set to `0.0`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    /// A vector with all components set to `1.0`.
    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        w: 1.0,
    };
    /// The unit vector pointing along the positive X-axis.
    pub const X: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    /// The unit vector pointing along the positive Y-axis.
    pub const Y: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
        w: 0.0,
    };
    /// The unit vector pointing along the positive Z-axis.
    pub const Z: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
        w: 0.0,
    };
    /// The unit vector pointing along the positive W-axis.
    pub const W: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Creates a new `Vec4` with the specified components.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Returns a new vector with the absolute value of each component.
    #[inline]
    pub const fn abs(self) -> Self {
        Self {
            x: if self.x < 0.0 { -self.x } else { self.x },
            y: if self.y < 0.0 { -self.y } else { self.y },
            z: if self.z < 0.0 { -self.z } else { self.z },
            w: if self.w < 0.0 { -self.w } else { self.w },
        }
    }

    /// Creates a `Vec4` from a `Vec3` and a `w` component.
    #[inline]
    pub fn from_vec3(v: Vec3, w: f32) -> Self {
        Self::new(v.x, v.y, v.z, w)
    }

    /// Returns the `[x, y, z]` components of the vector as a `Vec3`, discarding `w`.
    #[inline]
    pub fn truncate(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// Calculates the dot product of this vector and another.
    #[inline]
    pub fn dot(&self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Retrieves a component of the vector by its index.
    ///
    /// # Panics
    /// Panics if `index` is not between 0 and 3.
    #[inline]
    pub fn get(&self, index: usize) -> f32 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            3 => self.w,
            _ => panic!("Index out of bounds for Vec4"),
        }
    }
}

// --- Operator Overloads ---

impl Add for Vec4 {
    type Output = Self;
    /// Adds two vectors component-wise.
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

impl Sub for Vec4 {
    type Output = Self;
    /// Subtracts two vectors component-wise.
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

impl Mul<f32> for Vec4 {
    type Output = Self;
    /// Multiplies the vector by a scalar.
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
            w: self.w * rhs,
        }
    }
}

impl Mul<Vec4> for f32 {
    type Output = Vec4;
    /// Multiplies a scalar by a vector.
    #[inline]
    fn mul(self, rhs: Vec4) -> Self::Output {
        rhs * self
    }
}

impl Mul<Vec4> for Vec4 {
    type Output = Self;
    /// Multiplies two vectors component-wise.
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
            z: self.z * rhs.z,
            w: self.w * rhs.w,
        }
    }
}

impl Div<f32> for Vec4 {
    type Output = Self;
    /// Divides the vector by a scalar.
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        let inv_rhs = 1.0 / rhs;
        Self {
            x: self.x * inv_rhs,
            y: self.y * inv_rhs,
            z: self.z * inv_rhs,
            w: self.w * inv_rhs,
        }
    }
}

impl Neg for Vec4 {
    type Output = Self;
    /// Negates the vector.
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

impl Index<usize> for Vec4 {
    type Output = f32;
    /// Allows accessing a vector component by index.
    /// # Panics
    /// Panics if `index` is not between 0 and 3.
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            3 => &self.w,
            _ => panic!("Index out of bounds for Vec4"),
        }
    }
}

impl IndexMut<usize> for Vec4 {
    /// Allows mutably accessing a vector component by index.
    /// # Panics
    /// Panics if `index` is not between 0 and 3.
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            3 => &mut self.w,
            _ => panic!("Index out of bounds for Vec4"),
        }
    }
}

/// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*; // Import Vec3 from the parent module
    use crate::math::approx_eq;

    fn vec2_approx_eq(a: Vec2, b: Vec2) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
    }

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    // Test Vec2

    #[test]
    fn test_vec2_new() {
        let v = Vec2::new(1.0, 2.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
    }

    #[test]
    fn test_vec2_abs() {
        let v = Vec2::new(-1.0, 2.0);
        assert_eq!(v.abs(), Vec2::new(1.0, 2.0));
    }

    #[test]
    fn test_vec2_constants() {
        assert_eq!(Vec2::ZERO, Vec2::new(0.0, 0.0));
        assert_eq!(Vec2::ONE, Vec2::new(1.0, 1.0));
        assert_eq!(Vec2::X, Vec2::new(1.0, 0.0));
        assert_eq!(Vec2::Y, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn test_vec2_ops() {
        let v1 = Vec2::new(1.0, 2.0);
        let v2 = Vec2::new(3.0, 4.0);
        assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
        assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
        assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(3.0 * v1, Vec2::new(3.0, 6.0));
        assert_eq!(v1 * v2, Vec2::new(3.0, 8.0)); // Component-wise
        assert_eq!(-v1, Vec2::new(-1.0, -2.0));
        assert!(vec2_approx_eq(
            Vec2::new(4.0, 6.0) / 2.0,
            Vec2::new(2.0, 3.0)
        ));
    }

    #[test]
    fn test_vec2_dot() {
        let v1 = Vec2::new(1.0, 2.0);
        let v2 = Vec2::new(3.0, 4.0);
        assert!(approx_eq(v1.dot(v2), 1.0 * 3.0 + 2.0 * 4.0)); // 3 + 8 = 11
    }

    #[test]
    fn test_vec2_length() {
        let v = Vec2::new(3.0, 4.0);
        assert!(approx_eq(v.length_squared(), 25.0));
        assert!(approx_eq(v.length(), 5.0));
        assert!(approx_eq(Vec2::ZERO.length(), 0.0));
    }

    #[test]
    fn test_vec2_normalize() {
        let v1 = Vec2::new(3.0, 0.0);
        let norm_v1 = v1.normalize();
        assert!(vec2_approx_eq(norm_v1, Vec2::X));
        assert!(approx_eq(norm_v1.length(), 1.0));

        let v_zero = Vec2::ZERO;
        assert_eq!(v_zero.normalize(), Vec2::ZERO);
    }

    #[test]
    fn test_vec2_lerp() {
        let start = Vec2::new(0.0, 10.0);
        let end = Vec2::new(10.0, 0.0);
        assert!(vec2_approx_eq(Vec2::lerp(start, end, 0.0), start));
        assert!(vec2_approx_eq(Vec2::lerp(start, end, 1.0), end));
        assert!(vec2_approx_eq(
            Vec2::lerp(start, end, 0.5),
            Vec2::new(5.0, 5.0)
        ));
        // Test clamping
        assert!(vec2_approx_eq(Vec2::lerp(start, end, -0.5), start));
        assert!(vec2_approx_eq(Vec2::lerp(start, end, 1.5), end));
    }

    #[test]
    fn test_vec2_index() {
        let mut v = Vec2::new(5.0, 6.0);
        assert_eq!(v[0], 5.0);
        assert_eq!(v[1], 6.0);
        v[0] = 10.0;
        assert_eq!(v.x, 10.0);
    }

    #[test]
    #[should_panic]
    fn test_vec2_index_out_of_bounds() {
        let v = Vec2::new(1.0, 2.0);
        let _ = v[2]; // Should panic
    }

    // Test Vec3

    #[test]
    fn test_new() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_vec3_abs() {
        let v = Vec3::new(-1.0, 2.0, -3.0);
        assert_eq!(v.abs(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(Vec3::ZERO.abs(), Vec3::ZERO);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(Vec3::ONE, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(Vec3::X, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(Vec3::Y, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Vec3::Z, Vec3::new(0.0, 0.0, 1.0));
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
        assert!(approx_eq(v1.distance(v2), 3.0 * (3.0_f32).sqrt()));
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

        assert!(vec3_approx_eq(Vec3::lerp(start, end, 0.0), start));
        assert!(vec3_approx_eq(Vec3::lerp(start, end, 1.0), end));
        assert!(vec3_approx_eq(
            Vec3::lerp(start, end, 0.5),
            Vec3::new(5.0, 5.0, 5.0)
        ));
    }

    // Test Vec4

    #[test]
    fn test_vec4_new() {
        let v = Vec4::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
        assert_eq!(v.w, 4.0);
    }

    #[test]
    fn test_vec4_abs() {
        let v = Vec4::new(-1.0, 2.0, -3.0, -0.5);
        assert_eq!(v.abs(), Vec4::new(1.0, 2.0, 3.0, 0.5));
    }

    #[test]
    fn test_vec4_from_vec3() {
        let v3 = Vec3::new(1.0, 2.0, 3.0);
        let v4 = Vec4::from_vec3(v3, 4.0);
        assert_eq!(v4, Vec4::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn test_vec4_truncate() {
        let v4 = Vec4::new(1.0, 2.0, 3.0, 4.0);
        let v3 = v4.truncate();
        assert_eq!(v3, Vec3::new(1.0, 2.0, 3.0));
    }
}
