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

//! Bind-group contract for the material bind group (group 2 in every
//! lit pipeline).
//!
//! This file is the **single source of truth** mirrored by the WGSL lib
//! module `khora-infra/src/graphics/shader/shaders/lib/std/material_textures.wgsl`.
//! Bump a constant here and update the WGSL counterpart in lockstep.

use std::marker::PhantomData;

use crate::renderer::api::command::{
    BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType, BufferBinding,
    BufferBindingType, SamplerBindingType, TextureSampleType, TextureViewDimension,
};
use crate::renderer::api::resource::{BufferId, SamplerId, TextureViewId};
use crate::renderer::api::shader::ShaderVariantKey;
use crate::renderer::api::util::ShaderStageFlags;

/// Binding indices inside the material bind group (group 2).
pub mod binding {
    /// Binding slot for the `MaterialUniforms` uniform buffer.
    pub const MATERIAL_UNIFORMS: u32 = 0;
    /// Binding slot for the base-color (albedo) texture — sRGB.
    pub const BASE_COLOR_TEXTURE: u32 = 1;
    /// Binding slot for the metallic-roughness texture (B=metallic, G=roughness) — linear.
    pub const METALLIC_ROUGHNESS_TEXTURE: u32 = 2;
    /// Binding slot for the tangent-space normal texture — linear.
    pub const NORMAL_TEXTURE: u32 = 3;
    /// Binding slot for the emissive texture — sRGB.
    pub const EMISSIVE_TEXTURE: u32 = 4;
    /// Binding slot for the ambient-occlusion texture (red channel) — linear.
    pub const OCCLUSION_TEXTURE: u32 = 5;
    /// Binding slot for the specular-strength texture (alpha channel) — linear.
    pub const SPECULAR_TEXTURE: u32 = 6;
    /// Binding slot for the specular-colour texture (rgb) — sRGB.
    pub const SPECULAR_COLOR_TEXTURE: u32 = 7;
    /// Binding slot for the filtering sampler shared by all material maps.
    pub const SAMPLER: u32 = 8;
}

/// Canonical shader-variant flag names for the optional material texture
/// slots. A material's [`ShaderVariantKey`]
/// sets the flag for each texture slot it declares; the group-2 layout,
/// the WGSL `#ifdef` gates, and the cached bind group are all derived from
/// the same set, so they stay in lockstep. Single source of truth — never
/// duplicate these strings.
pub mod flag {
    /// Set when the material declares a base-color (albedo) texture.
    pub const HAS_BASE_COLOR_TEXTURE: &str = "HAS_BASE_COLOR_TEXTURE";
    /// Set when the material declares a metallic-roughness texture.
    pub const HAS_METALLIC_ROUGHNESS_TEXTURE: &str = "HAS_METALLIC_ROUGHNESS_TEXTURE";
    /// Set when the material declares a tangent-space normal map.
    pub const HAS_NORMAL_MAP: &str = "HAS_NORMAL_MAP";
    /// Set when the material declares an emissive texture.
    pub const HAS_EMISSIVE_TEXTURE: &str = "HAS_EMISSIVE_TEXTURE";
    /// Set when the material declares an ambient-occlusion map.
    pub const HAS_OCCLUSION_MAP: &str = "HAS_OCCLUSION_MAP";
    /// Set when the material declares a specular-strength texture.
    pub const HAS_SPECULAR_MAP: &str = "HAS_SPECULAR_MAP";
    /// Set when the material declares a specular-colour texture.
    pub const HAS_SPECULAR_COLOR_MAP: &str = "HAS_SPECULAR_COLOR_MAP";
}

/// Per-material GPU resources composing the group-2 bind group.
///
/// Lit lanes never build this — the data-layer material projection does,
/// once per material UUID. Lanes simply bind the resulting
/// [`GpuMaterial`](crate::renderer::api::material::GpuMaterial) bind group.
///
/// Each texture view is `Some` only when the material declares that map
/// (no fallback textures); the `None` slots are absent from both the
/// variant layout and the bind group.
#[derive(Debug, Clone, Copy)]
pub struct MaterialGpuBindings {
    /// `MaterialUniforms` uniform buffer for this material.
    pub uniform_buffer: BufferId,
    /// Base-color (albedo) texture view, if the material declares one.
    pub base_color: Option<TextureViewId>,
    /// Metallic-roughness texture view, if the material declares one.
    pub metallic_roughness: Option<TextureViewId>,
    /// Tangent-space normal texture view, if the material declares one.
    pub normal: Option<TextureViewId>,
    /// Emissive texture view, if the material declares one.
    pub emissive: Option<TextureViewId>,
    /// Ambient-occlusion texture view, if the material declares one.
    pub occlusion: Option<TextureViewId>,
    /// Specular-strength texture view, if the material declares one.
    pub specular: Option<TextureViewId>,
    /// Specular-colour texture view, if the material declares one.
    pub specular_color: Option<TextureViewId>,
    /// Filtering sampler shared by all maps.
    pub sampler: SamplerId,
}

/// Returns the nine [`BindGroupLayoutEntry`] values defining the canonical
/// group-2 material layout. Created once at bootstrap and referenced by
/// every lit pipeline and every cached material bind group.
pub fn material_bind_group_layout_entries() -> [BindGroupLayoutEntry; 9] {
    let texture = |binding: u32| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    };
    [
        BindGroupLayoutEntry {
            binding: binding::MATERIAL_UNIFORMS,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        },
        texture(binding::BASE_COLOR_TEXTURE),
        texture(binding::METALLIC_ROUGHNESS_TEXTURE),
        texture(binding::NORMAL_TEXTURE),
        texture(binding::EMISSIVE_TEXTURE),
        texture(binding::OCCLUSION_TEXTURE),
        texture(binding::SPECULAR_TEXTURE),
        texture(binding::SPECULAR_COLOR_TEXTURE),
        BindGroupLayoutEntry {
            binding: binding::SAMPLER,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
    ]
}

/// Returns the group-2 layout entries for a given shader variant: the
/// uniform at binding 0, the filtering sampler at binding 8, and **only**
/// the texture bindings whose `HAS_*` flag is set in `variant`. Binding
/// indices stay fixed (1 to 7) so the WGSL `#ifdef`-gated declarations and
/// the cached bind group line up regardless of which textures are present.
///
/// The same `(LayoutKey::Material, variant)` resolves to this layout for
/// both the lit pipeline (group 2) and the material's cached bind group,
/// keeping the two byte-for-byte identical.
pub fn material_layout_entries_for_variant(
    variant: &ShaderVariantKey,
) -> Vec<BindGroupLayoutEntry> {
    let texture = |binding: u32| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    };
    let mut entries = vec![BindGroupLayoutEntry {
        binding: binding::MATERIAL_UNIFORMS,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }];
    if variant.has_flag(flag::HAS_BASE_COLOR_TEXTURE) {
        entries.push(texture(binding::BASE_COLOR_TEXTURE));
    }
    if variant.has_flag(flag::HAS_METALLIC_ROUGHNESS_TEXTURE) {
        entries.push(texture(binding::METALLIC_ROUGHNESS_TEXTURE));
    }
    if variant.has_flag(flag::HAS_NORMAL_MAP) {
        entries.push(texture(binding::NORMAL_TEXTURE));
    }
    if variant.has_flag(flag::HAS_EMISSIVE_TEXTURE) {
        entries.push(texture(binding::EMISSIVE_TEXTURE));
    }
    if variant.has_flag(flag::HAS_OCCLUSION_MAP) {
        entries.push(texture(binding::OCCLUSION_TEXTURE));
    }
    if variant.has_flag(flag::HAS_SPECULAR_MAP) {
        entries.push(texture(binding::SPECULAR_TEXTURE));
    }
    if variant.has_flag(flag::HAS_SPECULAR_COLOR_MAP) {
        entries.push(texture(binding::SPECULAR_COLOR_TEXTURE));
    }
    entries.push(BindGroupLayoutEntry {
        binding: binding::SAMPLER,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Sampler(SamplerBindingType::Filtering),
    });
    entries
}

/// Pushes the material [`BindGroupEntry`] values into `entries`, in binding
/// order: the uniform at 0, each declared texture at its fixed slot, then
/// the sampler at 8. Absent textures (`None`) are skipped, so the entries
/// match [`material_layout_entries_for_variant`] for the same variant.
/// Called by the data-layer projection when building a material's cached
/// bind group.
pub fn fill_material_bind_group_entries<'a>(
    bindings: &MaterialGpuBindings,
    entries: &mut Vec<BindGroupEntry<'a>>,
) {
    entries.push(BindGroupEntry {
        binding: binding::MATERIAL_UNIFORMS,
        resource: BindingResource::Buffer(BufferBinding {
            buffer: bindings.uniform_buffer,
            offset: 0,
            size: None,
        }),
        _phantom: PhantomData,
    });
    let texture = |binding: u32, view: TextureViewId| BindGroupEntry {
        binding,
        resource: BindingResource::TextureView(view),
        _phantom: PhantomData,
    };
    if let Some(view) = bindings.base_color {
        entries.push(texture(binding::BASE_COLOR_TEXTURE, view));
    }
    if let Some(view) = bindings.metallic_roughness {
        entries.push(texture(binding::METALLIC_ROUGHNESS_TEXTURE, view));
    }
    if let Some(view) = bindings.normal {
        entries.push(texture(binding::NORMAL_TEXTURE, view));
    }
    if let Some(view) = bindings.emissive {
        entries.push(texture(binding::EMISSIVE_TEXTURE, view));
    }
    if let Some(view) = bindings.occlusion {
        entries.push(texture(binding::OCCLUSION_TEXTURE, view));
    }
    if let Some(view) = bindings.specular {
        entries.push(texture(binding::SPECULAR_TEXTURE, view));
    }
    if let Some(view) = bindings.specular_color {
        entries.push(texture(binding::SPECULAR_COLOR_TEXTURE, view));
    }
    entries.push(BindGroupEntry {
        binding: binding::SAMPLER,
        resource: BindingResource::Sampler(bindings.sampler),
        _phantom: PhantomData,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_indices_are_sequential_and_match_wgsl() {
        assert_eq!(binding::MATERIAL_UNIFORMS, 0);
        assert_eq!(binding::BASE_COLOR_TEXTURE, 1);
        assert_eq!(binding::METALLIC_ROUGHNESS_TEXTURE, 2);
        assert_eq!(binding::NORMAL_TEXTURE, 3);
        assert_eq!(binding::EMISSIVE_TEXTURE, 4);
        assert_eq!(binding::OCCLUSION_TEXTURE, 5);
        assert_eq!(binding::SPECULAR_TEXTURE, 6);
        assert_eq!(binding::SPECULAR_COLOR_TEXTURE, 7);
        assert_eq!(binding::SAMPLER, 8);
    }

    #[test]
    fn variant_layout_and_entries_agree_for_base_color_only() {
        let variant = ShaderVariantKey::empty().flag(flag::HAS_BASE_COLOR_TEXTURE);
        let layout = material_layout_entries_for_variant(&variant);
        // uniform@0 + base_color@1 + sampler@8 — no absent textures.
        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0].binding, binding::MATERIAL_UNIFORMS);
        assert_eq!(layout[1].binding, binding::BASE_COLOR_TEXTURE);
        assert_eq!(layout[2].binding, binding::SAMPLER);

        let bindings = MaterialGpuBindings {
            uniform_buffer: BufferId(1),
            base_color: Some(TextureViewId(2)),
            metallic_roughness: None,
            normal: None,
            emissive: None,
            occlusion: None,
            specular: None,
            specular_color: None,
            sampler: SamplerId(6),
        };
        let mut entries: Vec<BindGroupEntry> = Vec::new();
        fill_material_bind_group_entries(&bindings, &mut entries);
        assert_eq!(entries.len(), layout.len());
        for (entry, layout_entry) in entries.iter().zip(layout.iter()) {
            assert_eq!(entry.binding, layout_entry.binding);
        }
    }

    #[test]
    fn empty_variant_layout_is_uniform_plus_sampler() {
        let layout = material_layout_entries_for_variant(&ShaderVariantKey::empty());
        assert_eq!(layout.len(), 2);
        assert_eq!(layout[0].binding, binding::MATERIAL_UNIFORMS);
        assert_eq!(layout[1].binding, binding::SAMPLER);
    }

    #[test]
    fn occlusion_variant_layout_and_entries_agree() {
        let variant = ShaderVariantKey::empty().flag(flag::HAS_OCCLUSION_MAP);
        let layout = material_layout_entries_for_variant(&variant);
        // uniform@0 + occlusion@5 + sampler@8 — occlusion binds before the sampler.
        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0].binding, binding::MATERIAL_UNIFORMS);
        assert_eq!(layout[1].binding, binding::OCCLUSION_TEXTURE);
        assert_eq!(layout[2].binding, binding::SAMPLER);

        let bindings = MaterialGpuBindings {
            uniform_buffer: BufferId(1),
            base_color: None,
            metallic_roughness: None,
            normal: None,
            emissive: None,
            occlusion: Some(TextureViewId(2)),
            specular: None,
            specular_color: None,
            sampler: SamplerId(6),
        };
        let mut entries: Vec<BindGroupEntry> = Vec::new();
        fill_material_bind_group_entries(&bindings, &mut entries);
        assert_eq!(entries.len(), layout.len());
        for (entry, layout_entry) in entries.iter().zip(layout.iter()) {
            assert_eq!(entry.binding, layout_entry.binding);
        }
    }

    // --- KHR_materials_specular -------------------------------------------

    /// Every texture flag, in binding order.
    const EVERY_FLAG: [&str; 7] = [
        flag::HAS_BASE_COLOR_TEXTURE,
        flag::HAS_METALLIC_ROUGHNESS_TEXTURE,
        flag::HAS_NORMAL_MAP,
        flag::HAS_EMISSIVE_TEXTURE,
        flag::HAS_OCCLUSION_MAP,
        flag::HAS_SPECULAR_MAP,
        flag::HAS_SPECULAR_COLOR_MAP,
    ];

    fn variant_of(flags: &[&'static str]) -> ShaderVariantKey {
        flags
            .iter()
            .copied()
            .fold(ShaderVariantKey::empty(), ShaderVariantKey::flag)
    }

    fn layout_bindings(variant: &ShaderVariantKey) -> Vec<u32> {
        material_layout_entries_for_variant(variant)
            .iter()
            .map(|entry| entry.binding)
            .collect()
    }

    fn filled_bindings(bindings: &MaterialGpuBindings) -> Vec<u32> {
        let mut entries: Vec<BindGroupEntry> = Vec::new();
        fill_material_bind_group_entries(bindings, &mut entries);
        entries.iter().map(|entry| entry.binding).collect()
    }

    /// Bindings with only the uniform and the sampler; the caller sets the
    /// texture views it declares.
    fn bare_bindings() -> MaterialGpuBindings {
        MaterialGpuBindings {
            uniform_buffer: BufferId(1),
            base_color: None,
            metallic_roughness: None,
            normal: None,
            emissive: None,
            occlusion: None,
            specular: None,
            specular_color: None,
            sampler: SamplerId(99),
        }
    }

    #[test]
    fn the_specular_maps_bind_at_six_and_seven_and_the_sampler_at_eight() {
        assert_eq!(binding::SPECULAR_TEXTURE, 6);
        assert_eq!(binding::SPECULAR_COLOR_TEXTURE, 7);
        assert_eq!(binding::SAMPLER, 8);
    }

    #[test]
    fn the_full_material_layout_covers_nine_bindings_in_order() {
        let entries: Vec<BindGroupLayoutEntry> =
            material_bind_group_layout_entries().into_iter().collect();
        let bindings: Vec<u32> = entries.iter().map(|entry| entry.binding).collect();
        assert_eq!(bindings, (0..9).collect::<Vec<u32>>());
        assert!(matches!(entries[0].ty, BindingType::Buffer { .. }));
        for entry in &entries[1..8] {
            assert!(
                matches!(entry.ty, BindingType::Texture { .. }),
                "binding {} is a texture",
                entry.binding
            );
        }
        assert!(matches!(entries[8].ty, BindingType::Sampler(_)));
    }

    #[test]
    fn the_specular_maps_are_filterable_2d_textures_the_fragment_samples() {
        let variant = variant_of(&[flag::HAS_SPECULAR_MAP, flag::HAS_SPECULAR_COLOR_MAP]);
        let layout = material_layout_entries_for_variant(&variant);
        for slot in [binding::SPECULAR_TEXTURE, binding::SPECULAR_COLOR_TEXTURE] {
            let entry = layout
                .iter()
                .find(|entry| entry.binding == slot)
                .unwrap_or_else(|| panic!("binding {slot} is in the layout"));
            assert_eq!(entry.visibility, ShaderStageFlags::FRAGMENT);
            assert!(
                matches!(
                    entry.ty,
                    BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    }
                ),
                "binding {slot}: {:?}",
                entry.ty
            );
        }
    }

    #[test]
    fn filling_every_slot_pushes_nine_entries_with_the_specular_views_at_six_and_seven() {
        let bindings = MaterialGpuBindings {
            uniform_buffer: BufferId(1),
            base_color: Some(TextureViewId(2)),
            metallic_roughness: Some(TextureViewId(3)),
            normal: Some(TextureViewId(4)),
            emissive: Some(TextureViewId(5)),
            occlusion: Some(TextureViewId(6)),
            specular: Some(TextureViewId(7)),
            specular_color: Some(TextureViewId(8)),
            sampler: SamplerId(9),
        };
        let mut entries: Vec<BindGroupEntry> = Vec::new();
        fill_material_bind_group_entries(&bindings, &mut entries);

        let slots: Vec<u32> = entries.iter().map(|entry| entry.binding).collect();
        assert_eq!(slots, (0..9).collect::<Vec<u32>>());
        assert!(matches!(
            entries[6].resource,
            BindingResource::TextureView(TextureViewId(7))
        ));
        assert!(matches!(
            entries[7].resource,
            BindingResource::TextureView(TextureViewId(8))
        ));
        assert!(matches!(
            entries[8].resource,
            BindingResource::Sampler(SamplerId(9))
        ));
    }

    #[test]
    fn a_specular_map_alone_lays_out_the_uniform_the_map_and_the_sampler() {
        let variant = variant_of(&[flag::HAS_SPECULAR_MAP]);
        assert_eq!(layout_bindings(&variant), vec![0, 6, 8]);

        let bindings = MaterialGpuBindings {
            specular: Some(TextureViewId(2)),
            ..bare_bindings()
        };
        assert_eq!(filled_bindings(&bindings), layout_bindings(&variant));
    }

    #[test]
    fn a_specular_colour_map_alone_lays_out_the_uniform_the_map_and_the_sampler() {
        let variant = variant_of(&[flag::HAS_SPECULAR_COLOR_MAP]);
        assert_eq!(layout_bindings(&variant), vec![0, 7, 8]);

        let bindings = MaterialGpuBindings {
            specular_color: Some(TextureViewId(2)),
            ..bare_bindings()
        };
        assert_eq!(filled_bindings(&bindings), layout_bindings(&variant));
    }

    #[test]
    fn every_flag_lays_out_all_nine_bindings_as_the_filled_entries_do() {
        let variant = variant_of(&EVERY_FLAG);
        assert_eq!(layout_bindings(&variant), (0..9).collect::<Vec<u32>>());

        let bindings = MaterialGpuBindings {
            base_color: Some(TextureViewId(2)),
            metallic_roughness: Some(TextureViewId(3)),
            normal: Some(TextureViewId(4)),
            emissive: Some(TextureViewId(5)),
            occlusion: Some(TextureViewId(6)),
            specular: Some(TextureViewId(7)),
            specular_color: Some(TextureViewId(8)),
            ..bare_bindings()
        };
        assert_eq!(filled_bindings(&bindings), layout_bindings(&variant));
    }

    #[test]
    fn no_flag_lays_out_the_uniform_and_the_sampler_at_eight() {
        let variant = ShaderVariantKey::empty();
        assert_eq!(layout_bindings(&variant), vec![0, 8]);
        assert_eq!(filled_bindings(&bare_bindings()), vec![0, 8]);
    }
}
