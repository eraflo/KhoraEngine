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

//! Defines emissive materials for self-illuminating surfaces.

use crate::{
    asset::{Asset, Material},
    math::LinearRgba,
};

use super::AlphaMode;

/// A material that emits light without being affected by scene lighting.
///
/// Emissive materials are perfect for objects that should glow or appear self-illuminated,
/// such as neon signs, magical effects, UI elements, LED displays, or light sources.
/// They render at full brightness regardless of lighting conditions.
///
/// # HDR Support
///
/// The `intensity` parameter allows values greater than 1.0, enabling High Dynamic Range
/// (HDR) emissive effects. This is particularly useful for bloom post-processing effects
/// where bright emissive surfaces can "bleed" light into surrounding areas.
///
/// # Examples
///
/// ```
/// use khora_core::asset::EmissiveMaterial;
/// use khora_core::math::LinearRgba;
///
/// // Create a bright red glowing sign
/// let neon_sign = EmissiveMaterial {
///     emissive_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
///     intensity: 2.0,  // HDR intensity for bloom
///     ..Default::default()
/// };
///
/// // Create a subtle blue glow
/// let subtle_glow = EmissiveMaterial {
///     emissive_color: LinearRgba::new(0.3, 0.5, 1.0, 1.0),
///     intensity: 0.5,
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug)]
pub struct EmissiveMaterial {
    /// The emissive color of the material.
    ///
    /// This color is directly output to the framebuffer without any lighting calculations.
    /// The RGB values represent the color of the emitted light.
    pub emissive_color: LinearRgba,

    /// Optional texture for emissive color.
    ///
    /// If present, the texture's RGB values are multiplied with `emissive_color`.
    /// The alpha channel can be used for transparency when combined with appropriate `alpha_mode`.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub emissive_texture: Option<AssetHandle<TextureId>>,

    /// Intensity multiplier for the emissive color.
    ///
    /// Values greater than 1.0 enable HDR effects and are particularly useful for
    /// bloom post-processing. A value of 1.0 produces standard emissive output.
    ///
    /// **Recommended ranges:**
    /// - 0.0-1.0: Subtle emission
    /// - 1.0-3.0: Strong emission with bloom
    /// - 3.0+: Very bright, suitable for light sources
    pub intensity: f32,

    /// The alpha blending mode for this material.
    ///
    /// Determines how transparency is handled. See [`AlphaMode`] for details.
    pub alpha_mode: AlphaMode,
}

impl Default for EmissiveMaterial {
    fn default() -> Self {
        Self {
            emissive_color: LinearRgba::new(1.0, 1.0, 1.0, 1.0), // White
            // emissive_texture: None,
            intensity: 1.0,
            alpha_mode: AlphaMode::Opaque,
        }
    }
}

impl Asset for EmissiveMaterial {}
impl Material for EmissiveMaterial {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emissive_material_default() {
        let material = EmissiveMaterial::default();

        assert_eq!(material.emissive_color, LinearRgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(material.intensity, 1.0);
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
        // assert!(material.emissive_texture.is_none());
    }

    #[test]
    fn test_emissive_material_custom_color() {
        let material = EmissiveMaterial {
            emissive_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };

        assert_eq!(material.emissive_color, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn test_emissive_material_hdr_intensity() {
        // Test various intensity ranges
        let subtle = EmissiveMaterial {
            intensity: 0.5,
            ..Default::default()
        };
        assert_eq!(subtle.intensity, 0.5);

        let normal = EmissiveMaterial {
            intensity: 1.0,
            ..Default::default()
        };
        assert_eq!(normal.intensity, 1.0);

        let bright = EmissiveMaterial {
            intensity: 3.0,
            ..Default::default()
        };
        assert_eq!(bright.intensity, 3.0);

        let very_bright = EmissiveMaterial {
            intensity: 10.0,
            ..Default::default()
        };
        assert_eq!(very_bright.intensity, 10.0);
    }

    #[test]
    fn test_emissive_material_alpha_modes() {
        let opaque = EmissiveMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..Default::default()
        };
        assert_eq!(opaque.alpha_mode, AlphaMode::Opaque);

        let masked = EmissiveMaterial {
            alpha_mode: AlphaMode::Mask(0.5),
            ..Default::default()
        };
        assert_eq!(masked.alpha_mode, AlphaMode::Mask(0.5));

        let blend = EmissiveMaterial {
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        };
        assert_eq!(blend.alpha_mode, AlphaMode::Blend);
    }

    #[test]
    fn test_emissive_material_clone() {
        let original = EmissiveMaterial {
            emissive_color: LinearRgba::new(0.5, 0.7, 1.0, 1.0),
            intensity: 2.5,
            ..Default::default()
        };

        let cloned = original.clone();
        assert_eq!(cloned.emissive_color, original.emissive_color);
        assert_eq!(cloned.intensity, original.intensity);
    }

    #[test]
    fn test_emissive_material_neon_sign_example() {
        // Realistic example: blue neon sign
        let neon = EmissiveMaterial {
            emissive_color: LinearRgba::new(0.2, 0.5, 1.0, 1.0),
            intensity: 2.0,
            alpha_mode: AlphaMode::Opaque,
            // emissive_texture: None,
        };

        assert_eq!(neon.intensity, 2.0);
        assert_eq!(neon.alpha_mode, AlphaMode::Opaque);
    }
}
