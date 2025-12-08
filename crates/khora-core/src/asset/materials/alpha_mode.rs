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

//! Defines transparency and blending modes for materials.

/// Specifies how a material handles transparency and alpha blending.
///
/// This enum is critical for the rendering system to make intelligent decisions about
/// render pass ordering and pipeline selection. Different alpha modes have significant
/// performance implications:
///
/// - `Opaque`: Fastest, no transparency calculations
/// - `Mask`: Fast, no sorting required, uses alpha testing
/// - `Blend`: Slowest, requires depth sorting for correct rendering
///
/// # Examples
///
/// ```
/// use khora_core::asset::AlphaMode;
///
/// // Opaque material (default for most objects)
/// let opaque = AlphaMode::Opaque;
///
/// // Alpha masking (e.g., foliage, chain-link fences)
/// let masked = AlphaMode::Mask(0.5); // Discard pixels with alpha < 0.5
///
/// // Full blending (e.g., glass, water)
/// let blended = AlphaMode::Blend;
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlphaMode {
    /// The material is fully opaque with no transparency.
    ///
    /// This is the default and most performant mode. Fragments are always written
    /// to the framebuffer without alpha testing or blending.
    Opaque,

    /// The material uses alpha testing to create binary transparency.
    ///
    /// Fragments with an alpha value below the specified threshold are discarded.
    /// This mode is useful for rendering vegetation, chain-link fences, or other
    /// objects with hard transparency edges. It's significantly faster than
    /// `Blend` because it doesn't require depth sorting.
    ///
    /// The f32 value is the alpha cutoff threshold (typically 0.5).
    Mask(f32),

    /// The material uses full alpha blending.
    ///
    /// This mode produces smooth transparency but requires objects to be rendered
    /// in back-to-front order for correct results. The RenderAgent may choose
    /// different rendering strategies based on the number of blend-mode objects
    /// in the scene to balance quality and performance.
    Blend,
}

impl Default for AlphaMode {
    fn default() -> Self {
        Self::Opaque
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alpha_mode_default() {
        let default_mode = AlphaMode::default();
        assert_eq!(default_mode, AlphaMode::Opaque);
    }

    #[test]
    fn test_alpha_mode_opaque() {
        let opaque = AlphaMode::Opaque;
        assert_eq!(opaque, AlphaMode::Opaque);
    }

    #[test]
    fn test_alpha_mode_mask() {
        let masked = AlphaMode::Mask(0.5);
        match masked {
            AlphaMode::Mask(cutoff) => assert_eq!(cutoff, 0.5),
            _ => panic!("Expected AlphaMode::Mask"),
        }
    }

    #[test]
    fn test_alpha_mode_blend() {
        let blended = AlphaMode::Blend;
        assert_eq!(blended, AlphaMode::Blend);
    }

    #[test]
    fn test_alpha_mode_equality() {
        assert_eq!(AlphaMode::Opaque, AlphaMode::Opaque);
        assert_eq!(AlphaMode::Mask(0.5), AlphaMode::Mask(0.5));
        assert_eq!(AlphaMode::Blend, AlphaMode::Blend);

        assert_ne!(AlphaMode::Opaque, AlphaMode::Blend);
        assert_ne!(AlphaMode::Mask(0.5), AlphaMode::Mask(0.6));
    }

    #[test]
    fn test_alpha_mode_clone() {
        let original = AlphaMode::Mask(0.75);
        let cloned = original;
        assert_eq!(original, cloned);
    }
}
