use super::vector::Vec4;
use std::ops::{Add, Sub, Mul, Div};

/// Represents a color in Linear RGBA color space using f32 components.
/// Allows for HDR values (components > 1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct LinearRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl LinearRgba {
    // --- Common Color Constants ---
    pub const RED: Self = Self { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Self = Self { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE: Self = Self { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const YELLOW: Self = Self { r: 1.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const CYAN: Self = Self { r: 0.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const MAGENTA: Self = Self { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    /// Creates a new LinearRgba color.
    /// ## Arguments
    /// * `r` - Red component (0.0 to 1.0).
    /// * `g` - Green component (0.0 to 1.0).
    /// * `b` - Blue component (0.0 to 1.0).
    /// * `a` - Alpha component (0.0 to 1.0).
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a new opaque LinearRgba color (alpha = 1.0).
    /// ## Arguments
    /// * `r` - Red component (0.0 to 1.0).
    /// * `g` - Green component (0.0 to 1.0).
    /// * `b` - Blue component (0.0 to 1.0).
    /// ## Returns
    /// * A new LinearRgba color with alpha set to 1.0.
    #[inline]
    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Creates a LinearRgba color from a Vec4.
    /// ## Arguments
    /// * `v` - A Vec4 representing the color (x, y, z, w).
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    pub fn from_vec4(v: Vec4) -> Self {
        Self { r: v.x, g: v.y, b: v.z, a: v.w }
    }

    /// Converts this LinearRgba color to a Vec4.
    /// ## Returns
    /// * A Vec4 representing the color (x, y, z, w).
    #[inline]
    pub fn to_vec4(&self) -> Vec4 {
        Vec4::new(self.r, self.g, self.b, self.a)
    }

    /// Creates a LinearRgba color from a hex string.
    /// ## Arguments
    /// * `hex` - A hex string representing the color (e.g., "#FF5733").
    /// ## Returns
    /// * A new LinearRgba color.
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
        Self { r, g, b, a }
    }

    /// Converts this LinearRgba color to a hex string.
    /// ## Returns
    /// * A hex string representing the color (e.g., "#FF5733").
    #[inline]
    pub fn to_hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", (self.r * 255.0) as u8, (self.g * 255.0) as u8, (self.b * 255.0) as u8, (self.a * 255.0) as u8)
    }

    /// Creates a LinearRgba color from sRGB values.
    /// ## Arguments
    /// * `r` - Red component (0.0 to 1.0).
    /// * `g` - Green component (0.0 to 1.0).
    /// * `b` - Blue component (0.0 to 1.0).
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    pub fn from_srgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            r: r.powf(2.2),
            g: g.powf(2.2),
            b: b.powf(2.2),
            a: 1.0,
        }
    }

    /// Converts this LinearRgba color to sRGB.
    /// ## Returns
    /// * A new LinearRgba color in sRGB space.
    #[inline]
    pub fn to_srgb(&self) -> Self {
        Self {
            r: self.r.powf(1.0 / 2.2),
            g: self.g.powf(1.0 / 2.2),
            b: self.b.powf(1.0 / 2.2),
            a: self.a,
        }
    }

    /// Linear interpolation between two colors.
    /// ## Arguments
    /// * `start` - The starting color.
    /// * `end` - The ending color.
    /// * `t` - The interpolation factor (0.0 to 1.0).
    /// ## Returns
    /// * A new LinearRgba color that is the result of the interpolation.
    #[inline]
    pub fn lerp(start: Self, end: Self, t: f32) -> Self {
        let t_clamped = t.clamp(0.0, 1.0); // Utiliser clamp directement ici
        Self {
            r: start.r + (end.r - start.r) * t_clamped,
            g: start.g + (end.g - start.g) * t_clamped,
            b: start.b + (end.b - start.b) * t_clamped,
            a: start.a + (end.a - start.a) * t_clamped,
        }
    }

    /// Clamps the color components to the range [0.0, 1.0].
    /// ## Returns
    /// * A new LinearRgba color with clamped components.
    #[inline]
    fn clamp(&self) -> Self {
        Self {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
            a: self.a.clamp(0.0, 1.0),
        }
    }
}


// --- Operator Overloads ---


impl Default for LinearRgba {
    /// Default color is opaque white.
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    fn default() -> Self {
        Self::WHITE
    }
}


impl Add for LinearRgba {
    type Output = Self;
    /// Adds two LinearRgba colors component-wise.
    /// ## Arguments
    /// * `rhs` - The other LinearRgba color.
    /// ## Returns
    /// * A new LinearRgba color.
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
    /// Subtracts two LinearRgba colors component-wise.
    /// ## Arguments
    /// * `rhs` - The other LinearRgba color.
    /// ## Returns
    /// * A new LinearRgba color.
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

// Scalar multiplication (scales all components including alpha)
impl Mul<f32> for LinearRgba {
    type Output = Self;
    /// Multiplies a LinearRgba color by a scalar.
    /// ## Arguments
    /// * `scalar` - The scalar value.
    /// ## Returns
    /// * A new LinearRgba color.
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
    /// Multiplies a scalar by a LinearRgba color.
    /// ## Arguments
    /// * `color` - The LinearRgba color.
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    fn mul(self, color: LinearRgba) -> Self::Output {
        color * self // Reuse LinearRgba * f32
    }
}

// Component-wise multiplication (modulation)
impl Mul<LinearRgba> for LinearRgba {
    type Output = Self;
    /// Multiplies two LinearRgba colors component-wise.
    /// ## Arguments
    /// * `rhs` - The other LinearRgba color.
    /// ## Returns
    /// * A new LinearRgba color.
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
    /// Divides a LinearRgba color by a scalar.
    /// ## Arguments
    /// * `scalar` - The scalar value.
    /// ## Returns
    /// * A new LinearRgba color.
    #[inline]
    fn div(self, scalar: f32) -> Self::Output {
        let inv_scalar = 1.0 / scalar;
        Self {
            r: self.r * inv_scalar,
            g: self.g * inv_scalar,
            b: self.b * inv_scalar,
            a: self.a * inv_scalar,
        }
    }
}


// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{vector::Vec4, approx_eq};

    fn color_approx_eq(a: LinearRgba, b: LinearRgba) -> bool {
        approx_eq(a.r, b.r) && approx_eq(a.g, b.g) && approx_eq(a.b, b.b) && approx_eq(a.a, b.a)
    }

    #[test]
    fn test_color_new_and_rgb() {
        let c1 = LinearRgba::new(0.1, 0.2, 0.3, 0.4);
        assert_eq!(c1.r, 0.1);
        assert_eq!(c1.g, 0.2);
        assert_eq!(c1.b, 0.3);
        assert_eq!(c1.a, 0.4);

        let c2 = LinearRgba::rgb(0.5, 0.6, 0.7);
        assert_eq!(c2.r, 0.5);
        assert_eq!(c2.g, 0.6);
        assert_eq!(c2.b, 0.7);
        assert_eq!(c2.a, 1.0); // Alpha should be 1.0
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(LinearRgba::RED, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(LinearRgba::GREEN, LinearRgba::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(LinearRgba::BLUE, LinearRgba::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(LinearRgba::WHITE, LinearRgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(LinearRgba::BLACK, LinearRgba::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(LinearRgba::TRANSPARENT, LinearRgba::new(0.0, 0.0, 0.0, 0.0));
        assert_eq!(LinearRgba::YELLOW, LinearRgba::new(1.0, 1.0, 0.0, 1.0));
        assert_eq!(LinearRgba::CYAN, LinearRgba::new(0.0, 1.0, 1.0, 1.0));
        assert_eq!(LinearRgba::MAGENTA, LinearRgba::new(1.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn test_color_default() {
        assert_eq!(LinearRgba::default(), LinearRgba::WHITE);
    }

    #[test]
    fn test_color_vec4_conversion() {
        let c = LinearRgba::new(0.1, 0.2, 0.3, 0.4);
        let v = c.to_vec4();
        assert_eq!(v, Vec4::new(0.1, 0.2, 0.3, 0.4));

        let v2 = Vec4::new(0.5, 0.6, 0.7, 0.8);
        let c2 = LinearRgba::from_vec4(v2);
        assert_eq!(c2, LinearRgba::new(0.5, 0.6, 0.7, 0.8));
    }

    #[test]
    fn test_color_ops() {
        let c1 = LinearRgba::new(0.1, 0.2, 0.3, 0.8); // Use new for alpha <= 1.0 for clarity
        let c2 = LinearRgba::new(0.4, 0.5, 0.6, 0.5);

        // Check component-wise addition (including alpha)
        let expected_add = LinearRgba::new(0.5, 0.7, 0.9, 1.3); // 0.8 + 0.5 = 1.3
        assert!(color_approx_eq(c1 + c2, expected_add));

        // Check component-wise subtraction (including alpha)
        let expected_sub = LinearRgba::new(0.4 - 0.1, 0.5 - 0.2, 0.6 - 0.3, 0.5 - 0.8); // c2 - c1
        assert!(color_approx_eq(c2 - c1, expected_sub));

        // Check scalar multiplication (including alpha)
        let expected_mul_scalar = LinearRgba::new(0.1 * 2.0, 0.2 * 2.0, 0.3 * 2.0, 0.8 * 2.0); // c1 * 2.0
        assert!(color_approx_eq(c1 * 2.0, expected_mul_scalar));
        assert!(color_approx_eq(0.5 * c2, LinearRgba::new(0.2, 0.25, 0.3, 0.25))); // 0.5 * c2

        // Check component-wise multiplication (modulation)
        let expected_mul_comp = LinearRgba::new(0.1 * 0.4, 0.2 * 0.5, 0.3 * 0.6, 0.8 * 0.5); // c1 * c2
        assert!(color_approx_eq(c1 * c2, expected_mul_comp));

        // Check scalar division
        let expected_div_scalar = LinearRgba::new(0.4 / 2.0, 0.5 / 2.0, 0.6 / 2.0, 0.5 / 2.0); // c2 / 2.0
        assert!(color_approx_eq(c2 / 2.0, expected_div_scalar));
    }

    #[test]
    fn test_color_hex_conversion() {
        let hex = "#FF5733FF";
        let color = LinearRgba::from_hex(hex);
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.34117648);
        assert_eq!(color.b, 0.2);
        assert_eq!(color.a, 1.0);

        let hex_converted = color.to_hex();
        assert_eq!(hex_converted, "#FF5733FF");
    }

    #[test]
    fn test_color_srgb_conversion() {
        let srgb = LinearRgba::from_srgb(0.5, 0.5, 0.5);

        let back_to_srgb = srgb.to_srgb();

        assert!(approx_eq(back_to_srgb.r, 0.5));
        assert!(approx_eq(back_to_srgb.g, 0.5));
        assert!(approx_eq(back_to_srgb.b, 0.5));
        assert_eq!(back_to_srgb.a, 1.0);

        let expected_linear = 0.5f32.powf(2.2);
        assert!(approx_eq(srgb.r, expected_linear));
        assert!(approx_eq(srgb.g, expected_linear));
        assert!(approx_eq(srgb.b, expected_linear));
    }

    #[test]
    fn test_color_lerp() {
        let start = LinearRgba::BLACK;
        let end = LinearRgba::WHITE;
        let mid = LinearRgba::new(0.5, 0.5, 0.5, 1.0);

        assert!(color_approx_eq(LinearRgba::lerp(start, end, 0.0), start));
        assert!(color_approx_eq(LinearRgba::lerp(start, end, 1.0), end));
        assert!(color_approx_eq(LinearRgba::lerp(start, end, 0.5), mid));
        // Test clamping
        assert!(color_approx_eq(LinearRgba::lerp(start, end, -0.5), start));
        assert!(color_approx_eq(LinearRgba::lerp(start, end, 1.5), end));
    }
}