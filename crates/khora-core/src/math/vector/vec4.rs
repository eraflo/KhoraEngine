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

//! A 4-component vector.

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::vec3::Vec3;
use std::ops::{Add, Div, Index, IndexMut, Mul, Neg, Sub};

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

    /// Converts the vector to a `[f32; 4]` array.
    #[inline]
    pub fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

impl From<Vec4> for [f32; 4] {
    #[inline]
    fn from(v: Vec4) -> Self {
        [v.x, v.y, v.z, v.w]
    }
}

impl From<[f32; 4]> for Vec4 {
    #[inline]
    fn from(v: [f32; 4]) -> Self {
        Self::new(v[0], v[1], v[2], v[3])
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
