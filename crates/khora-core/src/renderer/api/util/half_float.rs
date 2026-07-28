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

//! IEEE-754 binary16 ("half") encoding for HDR pixel data.
//!
//! `Rgba16Float` is the engine's HDR texture layout: it holds values well above
//! `1.0` (sun, sky, emissive) and, unlike 32-bit float, is filterable on every
//! backend without an optional device feature. Producers of HDR pixels — the
//! image decoder for `.hdr`/`.exr`, and any procedurally generated environment
//! — pack their `f32` channels through here.

/// Largest finite IEEE-754 binary16 value.
pub const F16_MAX: f32 = 65504.0;
/// Smallest positive *normal* binary16 value (2^-14).
const F16_MIN_NORMAL: f32 = 6.103_515_6e-5;

/// Encodes `value` as IEEE-754 binary16 (half) bits.
///
/// Mantissa bits beyond half precision are truncated and half-subnormals flush
/// to signed zero: the inputs are HDR radiance values that later get convolved
/// into small cubemaps, so the error is far below anything the result can
/// represent.
///
/// Non-finite and out-of-range inputs **saturate to the largest finite half**
/// rather than becoming `Inf`. A blown-out pixel in an authored `.hdr` must not
/// inject `Inf`/`NaN` into the lighting chain, where it would poison an entire
/// irradiance convolution through a single sample.
pub fn f32_to_f16_bits(value: f32) -> u16 {
    let clamped = if value.is_nan() {
        0.0
    } else {
        value.clamp(-F16_MAX, F16_MAX)
    };
    let bits = clamped.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    if clamped.abs() < F16_MIN_NORMAL {
        return sign;
    }
    // Re-bias the exponent (f32 bias 127 → f16 bias 15) and drop the low 13
    // mantissa bits. Clamping above guarantees the exponent stays in [1, 30],
    // so the field never reaches the Inf/NaN encoding.
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = ((bits >> 13) & 0x03ff) as u16;
    sign | ((exponent as u16) << 10) | mantissa
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reference_values() {
        assert_eq!(f32_to_f16_bits(0.0), 0x0000);
        assert_eq!(f32_to_f16_bits(1.0), 0x3c00);
        assert_eq!(f32_to_f16_bits(-2.0), 0xc000);
        assert_eq!(f32_to_f16_bits(F16_MAX), 0x7bff);
    }

    #[test]
    fn saturates_instead_of_producing_non_finite() {
        assert_eq!(f32_to_f16_bits(1.0e9), 0x7bff);
        assert_eq!(f32_to_f16_bits(f32::INFINITY), 0x7bff);
        assert_eq!(f32_to_f16_bits(f32::NEG_INFINITY), 0xfbff);
        assert_eq!(f32_to_f16_bits(f32::NAN), 0x0000);
    }

    #[test]
    fn flushes_subnormals_to_signed_zero() {
        assert_eq!(f32_to_f16_bits(1.0e-8), 0x0000);
        assert_eq!(f32_to_f16_bits(-1.0e-8), 0x8000);
    }
}
