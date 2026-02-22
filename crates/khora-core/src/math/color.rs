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

//! Defines the `LinearRgba` color type and associated operations.

use crate::math::vector::Vec4;
use std::ops::{Add, Div, Mul, Sub};

/// Represents a color in a **linear RGBA** color space using `f32` components.
///
/// This struct is the standard color representation within Khora.  
/// Using a linear color space is crucial for correct lighting, shading, and blending.
/// The `f32` components allow for High Dynamic Range (HDR) colors, where component
/// values can exceed `1.0`.
///
/// `#[repr(C)]` ensures a consistent memory layout, which is important when passing
/// color data to graphics APIs.
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct LinearRgba {
    /// The red component in linear space.
    pub r: f32,
    /// The green component in linear space.
    pub g: f32,
    /// The blue component in linear space.
    pub b: f32,
    /// The alpha (opacity) component (linear, but not gamma-corrected).
    pub a: f32,
}

impl LinearRgba {
    // --- Common Color Constants ---

    /// Opaque red (`[1.0, 0.0, 0.0, 1.0]`).
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    /// Opaque green (`[0.0, 1.0, 0.0, 1.0]`).
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    /// Opaque blue (`[0.0, 0.0, 1.0, 1.0]`).
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    /// Opaque yellow (`[1.0, 1.0, 0.0, 1.0]`).
    pub const YELLOW: Self = Self::rgb(1.0, 1.0, 0.0);
    /// Opaque cyan (`[0.0, 1.0, 1.0, 1.0]`).
    pub const CYAN: Self = Self::rgb(0.0, 1.0, 1.0);
    /// Opaque magenta (`[1.0, 0.0, 1.0, 1.0]`).
    pub const MAGENTA: Self = Self::rgb(1.0, 0.0, 1.0);
    /// Opaque white (`[1.0, 1.0, 1.0, 1.0]`).
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Opaque black (`[0.0, 0.0, 0.0, 1.0]`).
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Fully transparent black (`[0.0, 0.0, 0.0, 0.0]`).
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// Creates a new `LinearRgba` with explicit RGBA values.
    #[inline]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a new opaque `LinearRgba` (alpha = 1.0).
    #[inline]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
}

// --- Helper functions for sRGB conversion ---
/// Converts an sRGB component to linear space.
#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a linear component to sRGB space.
#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// --- Conversions ---
impl LinearRgba {
    /// Creates a `LinearRgba` from a [`Vec4`].
    #[inline]
    pub fn from_vec4(v: Vec4) -> Self {
        Self {
            r: v.x,
            g: v.y,
            b: v.z,
            a: v.w,
        }
    }

    /// Converts this `LinearRgba` to a [`Vec4`].
    #[inline]
    pub fn to_vec4(&self) -> Vec4 {
        Vec4::new(self.r, self.g, self.b, self.a)
    }

    /// Creates a `LinearRgba` from an sRGB hex string (`#RRGGBB` or `#RRGGBBAA`).
    ///
    /// The RGB channels are converted to linear space.
    /// Alpha is normalized but not gamma corrected.
    ///
    /// # Panics
    /// Panics if the hex string is malformed.
    ///
    /// # Example
    /// ```
    /// use khora_core::math::color::LinearRgba;
    /// let color = LinearRgba::from_hex("#6495ED");
    /// ```
    #[inline]
    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
        let a = if hex.len() > 6 {
            u8::from_str_radix(&hex[6..8], 16).unwrap_or(255) as f32 / 255.0
        } else {
            1.0
        };
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a,
        }
    }

    /// Converts this linear color to an sRGB hex string (`#RRGGBBAA`).
    ///
    /// The RGB channels are gamma corrected to sRGB.
    /// Alpha is kept linear.
    #[inline]
    pub fn to_hex(&self) -> String {
        let r = linear_to_srgb(self.r).clamp(0.0, 1.0);
        let g = linear_to_srgb(self.g).clamp(0.0, 1.0);
        let b = linear_to_srgb(self.b).clamp(0.0, 1.0);
        let a = self.a.clamp(0.0, 1.0);

        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
            (a * 255.0).round() as u8
        )
    }

    /// Creates a `LinearRgba` by converting from normalized sRGB components.
    #[inline]
    pub fn from_srgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a: 1.0,
        }
    }

    /// Converts this linear color to sRGB components.
    #[inline]
    pub fn to_srgb(&self) -> Self {
        Self {
            r: linear_to_srgb(self.r),
            g: linear_to_srgb(self.g),
            b: linear_to_srgb(self.b),
            a: self.a,
        }
    }
}

// --- Manipulations ---
impl LinearRgba {
    /// Returns a new color with the same RGB components but a different alpha.
    #[inline]
    pub fn with_alpha(&self, a: f32) -> Self {
        Self { a, ..*self }
    }

    /// Linearly interpolates between two colors.
    /// The factor `t` is clamped to `[0.0, 1.0]`.
    #[inline]
    pub fn lerp(start: Self, end: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: start.r + (end.r - start.r) * t,
            g: start.g + (end.g - start.g) * t,
            b: start.b + (end.b - start.b) * t,
            a: start.a + (end.a - start.a) * t,
        }
    }
}

// --- Operator Overloads ---

impl Default for LinearRgba {
    /// Returns opaque white by default.
    #[inline]
    fn default() -> Self {
        Self::WHITE
    }
}

impl Add for LinearRgba {
    type Output = Self;
    /// Adds two colors component-wise.
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r + rhs.r,
            g: self.g + rhs.g,
            b: self.b + rhs.b,
            a: self.a + rhs.a,
        }
    }
}

impl Sub for LinearRgba {
    type Output = Self;
    /// Subtracts two colors component-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r - rhs.r,
            g: self.g - rhs.g,
            b: self.b - rhs.b,
            a: self.a - rhs.a,
        }
    }
}

impl Mul<f32> for LinearRgba {
    type Output = Self;
    /// Multiplies all components by a scalar.
    #[inline]
    fn mul(self, scalar: f32) -> Self::Output {
        Self {
            r: self.r * scalar,
            g: self.g * scalar,
            b: self.b * scalar,
            a: self.a * scalar,
        }
    }
}

impl Mul<LinearRgba> for f32 {
    type Output = LinearRgba;
    /// Multiplies a scalar by a color.
    #[inline]
    fn mul(self, color: LinearRgba) -> Self::Output {
        color * self
    }
}

impl Mul for LinearRgba {
    type Output = Self;
    /// Multiplies two colors component-wise (modulation).
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r * rhs.r,
            g: self.g * rhs.g,
            b: self.b * rhs.b,
            a: self.a * rhs.a,
        }
    }
}

impl Div<f32> for LinearRgba {
    type Output = Self;
    /// Divides all components by a scalar.
    #[inline]
    fn div(self, scalar: f32) -> Self::Output {
        let inv_scalar = 1.0 / scalar;
        self * inv_scalar
    }
}

// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::approx_eq;

    fn color_approx_eq(a: LinearRgba, b: LinearRgba) -> bool {
        approx_eq(a.r, b.r) && approx_eq(a.g, b.g) && approx_eq(a.b, b.b) && approx_eq(a.a, b.a)
    }

    #[test]
    fn test_color_hex_conversion() {
        let hex = "#FF5733FF";
        let color = LinearRgba::from_hex(hex);
        let expected_g = srgb_to_linear(0x57 as f32 / 255.0);
        // Expected linear values
        assert!(approx_eq(color.r, 1.0));
        assert!(approx_eq(color.g, expected_g));
        assert!(approx_eq(color.b, srgb_to_linear(0x33 as f32 / 255.0)));
        assert!(approx_eq(color.a, 1.0));

        let hex_converted = color.to_hex();
        assert_eq!(hex_converted, "#FF5733FF");
    }

    #[test]
    fn test_from_srgb_and_to_srgb() {
        let srgb_color = LinearRgba::from_srgb(0.5, 0.5, 0.5);
        let expected_linear = srgb_to_linear(0.5);
        assert!(approx_eq(srgb_color.r, expected_linear));
        assert!(approx_eq(srgb_color.g, expected_linear));
        assert!(approx_eq(srgb_color.b, expected_linear));

        let back_to_srgb = srgb_color.to_srgb();
        assert!(approx_eq(back_to_srgb.r, 0.5));
        assert!(approx_eq(back_to_srgb.g, 0.5));
        assert!(approx_eq(back_to_srgb.b, 0.5));
    }

    #[test]
    fn test_with_alpha() {
        let color = LinearRgba::RED.with_alpha(0.5);
        assert!(approx_eq(color.r, 1.0));
        assert!(approx_eq(color.g, 0.0));
        assert!(approx_eq(color.b, 0.0));
        assert!(approx_eq(color.a, 0.5));
    }

    #[test]
    fn test_lerp() {
        let red = LinearRgba::RED;
        let blue = LinearRgba::BLUE;
        let mid = LinearRgba::lerp(red, blue, 0.5);
        assert!(approx_eq(mid.r, 0.5));
        assert!(approx_eq(mid.g, 0.0));
        assert!(approx_eq(mid.b, 0.5));
        assert!(approx_eq(mid.a, 1.0));
    }

    #[test]
    fn test_vec4_conversion() {
        let color = LinearRgba::new(0.1, 0.2, 0.3, 0.4);
        let v = color.to_vec4();
        assert!(approx_eq(v.x, 0.1));
        assert!(approx_eq(v.y, 0.2));
        assert!(approx_eq(v.z, 0.3));
        assert!(approx_eq(v.w, 0.4));

        let color2 = LinearRgba::from_vec4(v);
        assert!(color_approx_eq(color, color2));
    }

    #[test]
    fn test_add_sub() {
        let c1 = LinearRgba::new(0.2, 0.3, 0.4, 0.5);
        let c2 = LinearRgba::new(0.1, 0.1, 0.1, 0.1);
        let sum = c1 + c2;
        assert!(approx_eq(sum.r, 0.3));
        assert!(approx_eq(sum.g, 0.4));
        assert!(approx_eq(sum.b, 0.5));
        assert!(approx_eq(sum.a, 0.6));

        let diff = c1 - c2;
        assert!(approx_eq(diff.r, 0.1));
        assert!(approx_eq(diff.g, 0.2));
        assert!(approx_eq(diff.b, 0.3));
        assert!(approx_eq(diff.a, 0.4));
    }

    #[test]
    fn test_mul_div() {
        let c = LinearRgba::new(0.2, 0.3, 0.4, 0.5);
        let scaled = c * 2.0;
        assert!(approx_eq(scaled.r, 0.4));
        assert!(approx_eq(scaled.g, 0.6));
        assert!(approx_eq(scaled.b, 0.8));
        assert!(approx_eq(scaled.a, 1.0));

        let div = scaled / 2.0;
        assert!(approx_eq(div.r, 0.2));
        assert!(approx_eq(div.g, 0.3));
        assert!(approx_eq(div.b, 0.4));
        assert!(approx_eq(div.a, 0.5));
    }

    #[test]
    fn test_component_mul() {
        let c1 = LinearRgba::new(0.2, 0.5, 0.8, 1.0);
        let c2 = LinearRgba::new(0.5, 0.5, 0.5, 0.5);
        let product = c1 * c2;
        assert!(approx_eq(product.r, 0.1));
        assert!(approx_eq(product.g, 0.25));
        assert!(approx_eq(product.b, 0.4));
        assert!(approx_eq(product.a, 0.5));
    }

    #[test]
    fn test_default() {
        let c = LinearRgba::default();
        assert_eq!(c, LinearRgba::WHITE);
    }
}
