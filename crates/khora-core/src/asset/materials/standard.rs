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

//! Defines the standard PBR material with metallic-roughness workflow.

use crate::{
    asset::{Asset, Material},
    math::LinearRgba,
};

use super::AlphaMode;

/// A physically-based rendering (PBR) material using the metallic-roughness workflow.
///
/// This is the primary material type for realistic 3D objects in Khora. It implements
/// the standard PBR metallic-roughness model, which is widely used in modern game engines
/// and 3D content creation tools (e.g., glTF 2.0 standard).
///
/// # PBR Properties
///
/// - **Base Color**: The surface's base color (albedo). For metals, this represents
///   the reflectance; for dielectrics, this is the diffuse color.
/// - **Metallic**: Controls whether the surface behaves like a metal (1.0) or a
///   dielectric/non-metal (0.0). Intermediate values create unrealistic results.
/// - **Roughness**: Controls how smooth (0.0) or rough (1.0) the surface appears.
///   This affects specular reflections.
///
/// # Texture Maps
///
/// Supports all common PBR texture maps:
/// - Base color/albedo
/// - Metallic-roughness combined (metallic in B channel, roughness in G channel)
/// - Normal map for surface detail
/// - Ambient occlusion for subtle shadows
/// - Emissive for self-illuminating areas
///
/// # Examples
///
/// ```
/// use khora_core::asset::StandardMaterial;
/// use khora_core::math::LinearRgba;
///
/// // Create a rough, non-metallic surface (e.g., concrete)
/// let concrete = StandardMaterial {
///     base_color: LinearRgba::new(0.5, 0.5, 0.5, 1.0),
///     metallic: 0.0,
///     roughness: 0.9,
///     ..Default::default()
/// };
///
/// // Create a smooth, metallic surface (e.g., polished gold)
/// let gold = StandardMaterial {
///     base_color: LinearRgba::new(1.0, 0.766, 0.336, 1.0),
///     metallic: 1.0,
///     roughness: 0.2,
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug)]
pub struct StandardMaterial {
    /// The base color (albedo) of the material.
    ///
    /// For metals, this is the reflectance color at normal incidence.
    /// For dielectrics, this is the diffuse color.
    pub base_color: LinearRgba,

    /// Optional texture for the base color.
    ///
    /// If present, this texture's RGB values are multiplied with `base_color`.
    /// The alpha channel can be used for transparency when combined with appropriate `alpha_mode`.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub base_color_texture: Option<AssetHandle<TextureId>>,

    /// The metallic factor (0.0 = dielectric, 1.0 = metal).
    ///
    /// This value should typically be either 0.0 or 1.0 for physically accurate results.
    /// Intermediate values can be used for artistic effects but are not physically based.
    pub metallic: f32,

    /// The roughness factor (0.0 = smooth, 1.0 = rough).
    ///
    /// Controls the microsurface detail of the material, affecting specular reflection.
    /// Lower values produce sharp, mirror-like reflections; higher values produce
    /// more diffuse reflections.
    pub roughness: f32,

    /// Optional texture for metallic and roughness values.
    ///
    /// **glTF 2.0 convention**: Blue channel = metallic, Green channel = roughness.
    /// If present, the texture values are multiplied with the `metallic` and `roughness` factors.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub metallic_roughness_texture: Option<AssetHandle<TextureId>>,

    /// Optional normal map for adding surface detail.
    ///
    /// Normal maps perturb the surface normal to create the illusion of fine geometric
    /// detail without adding actual geometry. Stored in tangent space.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub normal_map: Option<AssetHandle<TextureId>>,

    /// Optional ambient occlusion map.
    ///
    /// AO maps darken areas that should receive less ambient light, such as crevices
    /// and contact points. The red channel is typically used.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub occlusion_map: Option<AssetHandle<TextureId>>,

    /// The emissive color of the material.
    ///
    /// Allows the material to emit light. This color is added to the final shaded result
    /// and is not affected by lighting. Useful for self-illuminating objects like screens,
    /// neon signs, or magical effects.
    pub emissive: LinearRgba,

    /// Optional texture for emissive color.
    ///
    /// If present, this texture's RGB values are multiplied with `emissive`.
    ///
    /// **Future work**: Texture asset system integration pending.
    // pub emissive_texture: Option<AssetHandle<TextureId>>,

    /// The alpha blending mode for this material.
    ///
    /// Determines how transparency is handled. See [`AlphaMode`] for details.
    pub alpha_mode: AlphaMode,

    /// The alpha cutoff threshold when using `AlphaMode::Mask`.
    ///
    /// Fragments with alpha values below this threshold are discarded.
    /// Typically set to 0.5. Only used when `alpha_mode` is `AlphaMode::Mask`.
    pub alpha_cutoff: f32,

    /// Whether the material should be rendered double-sided.
    ///
    /// If `false`, back-facing triangles are culled for better performance.
    /// If `true`, both sides of the geometry are rendered.
    pub double_sided: bool,
}

impl Default for StandardMaterial {
    fn default() -> Self {
        Self {
            base_color: LinearRgba::new(0.8, 0.8, 0.8, 1.0), // Light gray
            // base_color_texture: None,
            metallic: 0.0,  // Non-metallic by default
            roughness: 0.5, // Medium roughness
            // metallic_roughness_texture: None,
            // normal_map: None,
            // occlusion_map: None,
            emissive: LinearRgba::new(0.0, 0.0, 0.0, 1.0), // No emission
            // emissive_texture: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        }
    }
}

impl Asset for StandardMaterial {}
impl Material for StandardMaterial {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_material_default() {
        let material = StandardMaterial::default();

        assert_eq!(material.base_color, LinearRgba::new(0.8, 0.8, 0.8, 1.0));
        assert_eq!(material.metallic, 0.0);
        assert_eq!(material.roughness, 0.5);
        assert_eq!(material.emissive, LinearRgba::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
        assert_eq!(material.alpha_cutoff, 0.5);
        assert_eq!(material.double_sided, false);
        // assert!(material.base_color_texture.is_none());
        // assert!(material.metallic_roughness_texture.is_none());
        // assert!(material.normal_map.is_none());
        // assert!(material.occlusion_map.is_none());
        // assert!(material.emissive_texture.is_none());
    }

    #[test]
    fn test_standard_material_custom_creation() {
        let material = StandardMaterial {
            base_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            metallic: 1.0,
            roughness: 0.2,
            ..Default::default()
        };

        assert_eq!(material.base_color, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(material.metallic, 1.0);
        assert_eq!(material.roughness, 0.2);
    }

    #[test]
    fn test_standard_material_metallic_range() {
        // Test common metallic values
        let dielectric = StandardMaterial {
            metallic: 0.0,
            ..Default::default()
        };
        assert_eq!(dielectric.metallic, 0.0);

        let metal = StandardMaterial {
            metallic: 1.0,
            ..Default::default()
        };
        assert_eq!(metal.metallic, 1.0);
    }

    #[test]
    fn test_standard_material_roughness_range() {
        // Test roughness extremes
        let smooth = StandardMaterial {
            roughness: 0.0,
            ..Default::default()
        };
        assert_eq!(smooth.roughness, 0.0);

        let rough = StandardMaterial {
            roughness: 1.0,
            ..Default::default()
        };
        assert_eq!(rough.roughness, 1.0);
    }

    #[test]
    fn test_standard_material_alpha_modes() {
        let opaque = StandardMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..Default::default()
        };
        assert_eq!(opaque.alpha_mode, AlphaMode::Opaque);

        let masked = StandardMaterial {
            alpha_mode: AlphaMode::Mask(0.5),
            alpha_cutoff: 0.5,
            ..Default::default()
        };
        assert_eq!(masked.alpha_mode, AlphaMode::Mask(0.5));
        assert_eq!(masked.alpha_cutoff, 0.5);

        let blend = StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        };
        assert_eq!(blend.alpha_mode, AlphaMode::Blend);
    }

    #[test]
    fn test_standard_material_double_sided() {
        let single_sided = StandardMaterial {
            double_sided: false,
            ..Default::default()
        };
        assert_eq!(single_sided.double_sided, false);

        let double_sided = StandardMaterial {
            double_sided: true,
            ..Default::default()
        };
        assert_eq!(double_sided.double_sided, true);
    }

    #[test]
    fn test_standard_material_clone() {
        let original = StandardMaterial {
            base_color: LinearRgba::new(0.5, 0.5, 0.5, 1.0),
            metallic: 0.8,
            roughness: 0.3,
            ..Default::default()
        };

        let cloned = original.clone();
        assert_eq!(cloned.base_color, original.base_color);
        assert_eq!(cloned.metallic, original.metallic);
        assert_eq!(cloned.roughness, original.roughness);
    }
}
