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

//! Bind-group contract for image-based lighting: the irradiance and
//! prefiltered cubes, the split-sum BRDF LUT and their sampler.
//!
//! The block sits in group 3 at a base index each lit lane picks, because the
//! lanes' group-3 layouts differ; the layout entries, the bind group and the
//! lane's WGSL must all use that same base.

use crate::renderer::api::command::{
    BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType, SamplerBindingType,
    TextureSampleType, TextureViewDimension,
};
use crate::renderer::api::resource::{SamplerId, TextureViewId};
use crate::renderer::api::util::ShaderStageFlags;

/// Number of bind-group slots the IBL block occupies (irradiance cube,
/// prefiltered cube, BRDF LUT, sampler).
pub const IBL_BINDING_COUNT: u32 = 4;

/// Offsets of each IBL resource relative to the block's `base` binding.
pub mod offset {
    /// Diffuse irradiance cube (relative to the IBL base binding).
    pub const IRRADIANCE_CUBE: u32 = 0;
    /// Prefiltered specular environment cube (mipmapped by roughness).
    pub const PREFILTERED_CUBE: u32 = 1;
    /// Split-sum BRDF integration LUT (2D).
    pub const BRDF_LUT: u32 = 2;
    /// Filtering sampler shared by the cubes and the LUT.
    pub const SAMPLER: u32 = 3;
}

/// Per-scene GPU resources the IBL bake publishes for lit lanes.
///
/// Lit lanes pass it to [`fill_ibl_bind_group_entries`] without reading it.
/// The bake is static after startup, so the same ids are bound every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IblGpuBindings {
    /// Full-resolution linear-HDR environment cube the bake derives from.
    ///
    /// Outside the lit lanes' IBL block: only the skybox pass samples it.
    pub env_cube: TextureViewId,
    /// Diffuse irradiance cube view (cosine-convolved environment).
    pub irradiance_cube: TextureViewId,
    /// Prefiltered specular environment cube view (roughness per mip).
    pub prefiltered_cube: TextureViewId,
    /// Split-sum BRDF integration LUT view (2D, `Rg16Float`).
    pub brdf_lut: TextureViewId,
    /// Filtering sampler (trilinear, mip-clamped) shared by all three.
    pub sampler: SamplerId,
}

/// Returns the four [`BindGroupLayoutEntry`] values the IBL block contributes
/// to a lit lane's group-3 bind-group layout, starting at `base`.
pub fn ibl_bind_group_layout_entries(base: u32) -> [BindGroupLayoutEntry; 4] {
    let cube = |binding: u32| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::Cube,
            multisampled: false,
        },
    };
    [
        cube(base + offset::IRRADIANCE_CUBE),
        cube(base + offset::PREFILTERED_CUBE),
        BindGroupLayoutEntry {
            binding: base + offset::BRDF_LUT,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: base + offset::SAMPLER,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
    ]
}

/// Pushes the four IBL [`BindGroupEntry`] values into `entries`, starting at
/// `base`.
///
/// `base` must match the one given to [`ibl_bind_group_layout_entries`] for
/// the same lane.
pub fn fill_ibl_bind_group_entries<'a>(
    bindings: &IblGpuBindings,
    base: u32,
    entries: &mut Vec<BindGroupEntry<'a>>,
) {
    let texture = |binding: u32, view: TextureViewId| BindGroupEntry {
        binding,
        resource: BindingResource::TextureView(view),
        _phantom: std::marker::PhantomData,
    };
    entries.push(texture(
        base + offset::IRRADIANCE_CUBE,
        bindings.irradiance_cube,
    ));
    entries.push(texture(
        base + offset::PREFILTERED_CUBE,
        bindings.prefiltered_cube,
    ));
    entries.push(texture(base + offset::BRDF_LUT, bindings.brdf_lut));
    entries.push(BindGroupEntry {
        binding: base + offset::SAMPLER,
        resource: BindingResource::Sampler(bindings.sampler),
        _phantom: std::marker::PhantomData,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_entries_are_contiguous_from_base() {
        let base = 4;
        let entries = ibl_bind_group_layout_entries(base);
        assert_eq!(entries.len() as u32, IBL_BINDING_COUNT);
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.binding, base + i as u32);
        }
    }

    #[test]
    fn fill_entries_match_layout_for_any_base() {
        for base in [4u32, 8] {
            let layout = ibl_bind_group_layout_entries(base);
            let bindings = IblGpuBindings {
                env_cube: TextureViewId(5),
                irradiance_cube: TextureViewId(1),
                prefiltered_cube: TextureViewId(2),
                brdf_lut: TextureViewId(3),
                sampler: SamplerId(4),
            };
            let mut entries: Vec<BindGroupEntry> = Vec::new();
            fill_ibl_bind_group_entries(&bindings, base, &mut entries);
            assert_eq!(entries.len(), layout.len());
            for (entry, layout_entry) in entries.iter().zip(layout.iter()) {
                assert_eq!(entry.binding, layout_entry.binding);
            }
        }
    }
}
