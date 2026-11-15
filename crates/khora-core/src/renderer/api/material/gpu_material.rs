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

//! GPU-side material projection.

use crate::asset::Asset;
use crate::renderer::api::command::BindGroupId;
use crate::renderer::api::material::MaterialGpuBindings;
use crate::renderer::api::resource::{BufferId, SamplerId, TextureId, TextureViewId};
use crate::renderer::api::shader::ShaderVariantKey;

/// GPU-ready material: its uniform buffer, declared texture maps, shared
/// sampler and ready-to-bind group-2 bind group.
///
/// A view is `Some` only for a map the material declares, and
/// [`variant`](Self::variant) records exactly that set of `HAS_*` flags. The
/// material owns its buffer, textures, views and bind group, freed on
/// eviction; the sampler is the engine's, never freed with it. Each
/// `*_texture` is `Some` exactly when its `*_view` is.
#[derive(Debug, Clone)]
pub struct GpuMaterial {
    /// `MaterialUniforms` uniform buffer (base color, factors, …).
    pub uniform_buffer: BufferId,
    /// Base-color (albedo) texture view, if declared.
    pub base_color_view: Option<TextureViewId>,
    /// Metallic-roughness texture view (B=metallic, G=roughness), if declared.
    pub metallic_roughness_view: Option<TextureViewId>,
    /// Tangent-space normal texture view, if declared.
    pub normal_view: Option<TextureViewId>,
    /// Emissive texture view, if declared.
    pub emissive_view: Option<TextureViewId>,
    /// Ambient-occlusion texture view, if declared.
    pub occlusion_view: Option<TextureViewId>,
    /// Base-color texture backing [`base_color_view`](Self::base_color_view),
    /// owned by this material and freed on eviction.
    pub base_color_texture: Option<TextureId>,
    /// Metallic-roughness texture backing
    /// [`metallic_roughness_view`](Self::metallic_roughness_view).
    pub metallic_roughness_texture: Option<TextureId>,
    /// Normal texture backing [`normal_view`](Self::normal_view).
    pub normal_texture: Option<TextureId>,
    /// Emissive texture backing [`emissive_view`](Self::emissive_view).
    pub emissive_texture: Option<TextureId>,
    /// Ambient-occlusion texture backing [`occlusion_view`](Self::occlusion_view).
    pub occlusion_texture: Option<TextureId>,
    /// Specular-strength texture view (alpha channel), if declared.
    pub specular_view: Option<TextureViewId>,
    /// Specular-colour texture view (rgb), if declared.
    pub specular_color_view: Option<TextureViewId>,
    /// Specular-strength texture backing [`specular_view`](Self::specular_view).
    pub specular_texture: Option<TextureId>,
    /// Specular-colour texture backing
    /// [`specular_color_view`](Self::specular_color_view).
    pub specular_color_texture: Option<TextureId>,
    /// Filtering sampler shared by all maps (engine-owned; NOT freed on eviction).
    pub sampler: SamplerId,
    /// Prebuilt group-2 bind group bound by lit lanes.
    pub bind_group: BindGroupId,
    /// Shader variant (`HAS_*` texture flags) this material was built for.
    pub variant: ShaderVariantKey,
    /// Whether back faces are drawn too; part of the pipeline cache key via
    /// `PipelineKey`.
    pub double_sided: bool,
    /// Whether the material is alpha-blended (`AlphaMode::Blend`); a masked
    /// material is not.
    ///
    /// A blended material needs its own pipeline state and is drawn after the
    /// opaque ones, back to front. See the book: concepts/rendering.md,
    /// "Transparency, and the order it needs".
    pub blend: bool,
}

impl GpuMaterial {
    /// Returns the [`MaterialGpuBindings`] view of this material's resources.
    ///
    /// It is what [`fill_material_bind_group_entries`](crate::renderer::api::material::fill_material_bind_group_entries)
    /// takes.
    pub fn bindings(&self) -> MaterialGpuBindings {
        MaterialGpuBindings {
            uniform_buffer: self.uniform_buffer,
            base_color: self.base_color_view,
            metallic_roughness: self.metallic_roughness_view,
            normal: self.normal_view,
            emissive: self.emissive_view,
            occlusion: self.occlusion_view,
            specular: self.specular_view,
            specular_color: self.specular_color_view,
            sampler: self.sampler,
        }
    }
}

impl Asset for GpuMaterial {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_carry_the_specular_views_the_material_owns() {
        let material = GpuMaterial {
            uniform_buffer: BufferId(1),
            base_color_view: None,
            metallic_roughness_view: None,
            normal_view: None,
            emissive_view: None,
            occlusion_view: None,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            occlusion_texture: None,
            specular_view: Some(TextureViewId(7)),
            specular_color_view: Some(TextureViewId(8)),
            specular_texture: Some(TextureId(17)),
            specular_color_texture: Some(TextureId(18)),
            sampler: SamplerId(9),
            bind_group: BindGroupId(2),
            variant: ShaderVariantKey::empty(),
            double_sided: false,
            blend: false,
        };

        let bindings = material.bindings();

        assert_eq!(bindings.specular, Some(TextureViewId(7)));
        assert_eq!(bindings.specular_color, Some(TextureViewId(8)));
    }
}
