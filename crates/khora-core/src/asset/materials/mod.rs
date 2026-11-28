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

//! Defines the core traits and material types for the rendering system.

mod alpha_mode;
mod emissive;
mod inline_material;
mod standard;
mod unlit;
mod wireframe;

pub use alpha_mode::*;
pub use emissive::*;
pub use inline_material::InlineMaterial;
pub use standard::*;
pub use unlit::*;
pub use wireframe::*;

use std::any::Any;

use super::Stored;

/// Helper trait to allow downcasting `dyn Material` trait objects to their concrete types.
pub trait AsAny {
    /// Returns a reference to the inner value as `&dyn Any`.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Supertrait that gives every material a `clone_box`, keeping `Material`
/// object-safe.
///
/// A blanket impl covers every `T: Material + Clone`.
pub trait MaterialClone {
    /// Clones `self` into a new boxed `dyn Material`.
    fn clone_box(&self) -> Box<dyn Material>;
}

impl<T: Material + Clone + 'static> MaterialClone for T {
    fn clone_box(&self) -> Box<dyn Material> {
        Box::new(self.clone())
    }
}

/// A trait for types that can be used as a material.
///
/// A material defines the surface properties of an object being rendered,
/// influencing how it interacts with light and determining which shader
/// (render pipeline) is used to draw it.
pub trait Material: Stored + AsAny + MaterialClone {
    /// Feeds the material's content to `hasher`: its type's stable name, then every field.
    ///
    /// Floats are fed by their bits, textures by their uuid. Two materials that
    /// feed equal content are one material: an [`InlineMaterial`] is named by
    /// this hash, so equal inline materials share one copy in the store and on
    /// a device, and a different type or field gives another.
    fn hash_content(&self, hasher: &mut crate::asset::ContentHasher);

    /// Returns the base color (albedo or diffuse) of the material.
    /// Default implementation is White.
    fn base_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::WHITE
    }

    /// Returns the emissive color of the material.
    /// Default implementation is Black.
    fn emissive_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::BLACK
    }

    /// Returns the specular power or roughness conversion for the material.
    /// Default implementation is 32.0.
    fn specular_power(&self) -> f32 {
        32.0
    }

    /// Returns the ambient color modifier for the material.
    /// Default implementation is (0.1, 0.1, 0.1, 0.0).
    fn ambient_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::new(0.1, 0.1, 0.1, 0.0)
    }

    /// Returns the metallic factor (0.0 = dielectric, 1.0 = metal).
    /// Default implementation is 0.0.
    fn metallic(&self) -> f32 {
        0.0
    }

    /// Returns the roughness factor (0.0 = smooth, 1.0 = rough).
    /// Default implementation is 1.0.
    fn roughness(&self) -> f32 {
        1.0
    }

    /// Returns the UUID of the base-color (albedo) texture, if any.
    /// Default implementation is `None` (untextured).
    fn base_color_texture(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of the metallic-roughness texture, if any.
    ///
    /// glTF convention: B = metallic, G = roughness. Default implementation
    /// is `None`.
    fn metallic_roughness_texture(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of the tangent-space normal map, if any.
    /// Default implementation is `None`.
    fn normal_map(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of the emissive texture, if any.
    /// Default implementation is `None`.
    fn emissive_texture(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of the ambient-occlusion map (red channel), if any.
    ///
    /// It attenuates the indirect (ambient/IBL) term only, never direct light.
    /// Default implementation is `None`.
    fn occlusion_map(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the specular strength (glTF `KHR_materials_specular`
    /// `specularFactor`). Default implementation is `1.0`.
    fn specular(&self) -> f32 {
        1.0
    }

    /// Returns the specular colour (glTF `KHR_materials_specular`
    /// `specularColorFactor`). Default implementation is white.
    fn specular_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::WHITE
    }

    /// Returns the UUID of the specular-strength texture (alpha channel), if
    /// any. Default implementation is `None`.
    fn specular_texture(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of the specular-colour texture (rgb), if any.
    /// Default implementation is `None`.
    fn specular_color_texture(&self) -> Option<crate::asset::AssetUUID> {
        None
    }

    /// Returns the UUID of every texture slot that is set, in slot order.
    ///
    /// The order: base colour, metallic-roughness, normal, emissive,
    /// occlusion, specular, specular colour. This is the single list of a
    /// material's texture slots.
    fn textures(&self) -> Vec<crate::asset::AssetUUID> {
        [
            self.base_color_texture(),
            self.metallic_roughness_texture(),
            self.normal_map(),
            self.emissive_texture(),
            self.occlusion_map(),
            self.specular_texture(),
            self.specular_color_texture(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Returns the material's alpha (transparency) mode.
    ///
    /// For [`AlphaMode::Mask`] the contained value is the alpha-cutoff
    /// threshold: fragments below it are discarded. Default implementation is
    /// [`AlphaMode::Opaque`].
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    /// Returns whether the material is rendered double-sided (back faces not culled).
    /// `false` (the default) culls back faces for correctly-wound meshes.
    fn double_sided(&self) -> bool {
        false
    }
}

/// A boxed material is an asset, so an `AssetHandle` can hold a material of
/// any concrete type.
impl Stored for Box<dyn Material> {}

impl crate::asset::Asset for Box<dyn Material> {
    const KIND: crate::asset::AssetKind = crate::asset::kinds::MATERIAL;
    type LocalSource = InlineMaterial;

    fn dependencies(&self, out: &mut Vec<crate::asset::NamedAsset>) {
        out.extend(
            self.textures()
                .into_iter()
                .map(|uuid| crate::asset::NamedAsset {
                    uuid,
                    kind: crate::asset::kinds::TEXTURE,
                }),
        );
    }
}

impl Clone for Box<dyn Material> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
