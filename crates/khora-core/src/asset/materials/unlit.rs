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

//! Defines unlit materials for the rendering system.

use crate::{
    asset::{Asset, Material},
    math::LinearRgba,
};

use super::AlphaMode;

/// A simple, unlit material.
///
/// This material does not react to lighting and simply renders with a solid
/// base color, optionally modulated by a texture. It's the most basic and
/// performant type of material, ideal for:
///
/// - UI elements and 2D sprites
/// - Debug visualization
/// - Performance-critical scenarios
/// - Stylized games that don't use lighting
/// - Skyboxes and distant geometry
///
/// # Performance
///
/// UnlitMaterial is the fastest material type in Khora. It requires minimal
/// shader calculations and is suitable for scenes with thousands of objects.
/// The RenderAgent may choose unlit rendering as a fallback strategy when
/// performance budgets are tight.
///
/// # Examples
///
/// ```
/// use khora_core::asset::{UnlitMaterial, AlphaMode};
/// use khora_core::math::LinearRgba;
///
/// // Create a solid red unlit material
/// let red = UnlitMaterial {
///     base_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
///     ..Default::default()
/// };
///
/// // Create an unlit material with alpha masking (e.g., foliage)
/// let foliage = UnlitMaterial {
///     base_color: LinearRgba::new(0.2, 0.8, 0.2, 1.0),
///     alpha_mode: AlphaMode::Mask(0.5),
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug)]
pub struct UnlitMaterial {
    /// The base color of the material.
    ///
    /// This color is directly output without any lighting calculations.
    /// When a texture is present, the texture color is multiplied with this value.
    pub base_color: LinearRgba,

    /// Optional texture for the base color.
    ///
    /// If present, the texture's RGB values are multiplied with `base_color`.
    /// The alpha channel can be used for transparency when combined with appropriate `alpha_mode`.
    ///
    /// **Future work**: This will be connected to the texture asset system when texture
    /// loading is fully implemented.
    // pub base_color_texture: Option<AssetHandle<TextureId>>,

    /// The alpha blending mode for this material.
    ///
    /// Determines how transparency is handled. See [`AlphaMode`] for details.
    pub alpha_mode: AlphaMode,

    /// The alpha cutoff threshold when using `AlphaMode::Mask`.
    ///
    /// Fragments with alpha values below this threshold are discarded.
    /// Typically set to 0.5. Only used when `alpha_mode` is `AlphaMode::Mask`.
    pub alpha_cutoff: f32,
}

impl Default for UnlitMaterial {
    fn default() -> Self {
        Self {
            base_color: LinearRgba::new(1.0, 1.0, 1.0, 1.0), // White
            // base_color_texture: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
        }
    }
}

// Mark `UnlitMaterial` as a valid asset.
impl Asset for UnlitMaterial {}

// Mark `UnlitMaterial` as a valid material.
impl Material for UnlitMaterial {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlit_material_default() {
        let material = UnlitMaterial::default();

        assert_eq!(material.base_color, LinearRgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
        assert_eq!(material.alpha_cutoff, 0.5);
        // assert!(material.base_color_texture.is_none());
    }

    #[test]
    fn test_unlit_material_custom_color() {
        let material = UnlitMaterial {
            base_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };

        assert_eq!(material.base_color, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn test_unlit_material_alpha_modes() {
        let opaque = UnlitMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..Default::default()
        };
        assert_eq!(opaque.alpha_mode, AlphaMode::Opaque);

        let masked = UnlitMaterial {
            alpha_mode: AlphaMode::Mask(0.5),
            alpha_cutoff: 0.5,
            ..Default::default()
        };
        assert_eq!(masked.alpha_mode, AlphaMode::Mask(0.5));
        assert_eq!(masked.alpha_cutoff, 0.5);

        let blend = UnlitMaterial {
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        };
        assert_eq!(blend.alpha_mode, AlphaMode::Blend);
    }

    #[test]
    fn test_unlit_material_clone() {
        let original = UnlitMaterial {
            base_color: LinearRgba::new(0.5, 0.7, 1.0, 1.0),
            alpha_mode: AlphaMode::Mask(0.3),
            alpha_cutoff: 0.3,
            // base_color_texture: None,
        };

        let cloned = original.clone();
        assert_eq!(cloned.base_color, original.base_color);
        assert_eq!(cloned.alpha_mode, original.alpha_mode);
        assert_eq!(cloned.alpha_cutoff, original.alpha_cutoff);
    }

    #[test]
    fn test_unlit_material_various_colors() {
        // Test common UI colors
        let red = UnlitMaterial {
            base_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };
        assert_eq!(red.base_color.r, 1.0);

        let blue = UnlitMaterial {
            base_color: LinearRgba::new(0.0, 0.0, 1.0, 1.0),
            ..Default::default()
        };
        assert_eq!(blue.base_color.b, 1.0);

        let semi_transparent = UnlitMaterial {
            base_color: LinearRgba::new(1.0, 1.0, 1.0, 0.5),
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        };
        assert_eq!(semi_transparent.base_color.a, 0.5);
        assert_eq!(semi_transparent.alpha_mode, AlphaMode::Blend);
    }
}
