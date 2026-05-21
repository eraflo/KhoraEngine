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

//! Bind-group contract for the shadow-aware lighting bind group
//! (group 3 in every lit pipeline).
//!
//! This file is the **single source of truth** mirrored by the WGSL
//! lib modules under `khora-lanes/src/render_lane/shaders/lib/shadow/`
//! (`bindings.wgsl` + sampling helpers). Bump a constant here and
//! update the WGSL counterpart in lockstep.

use crate::renderer::api::command::{
    BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType, SamplerBindingType,
    TextureSampleType, TextureViewDimension,
};
use crate::renderer::api::resource::{SamplerId, TextureViewId};
use crate::renderer::api::util::ShaderStageFlags;

/// Binding indices inside the "lit + shadow" bind group (group 3).
pub mod binding {
    /// Binding slot for the lit lane's `LightingUniforms` uniform buffer.
    pub const LIGHTING_UNIFORMS: u32 = 0;
    /// Binding slot for the 2D depth-array atlas (directional / spot).
    pub const ATLAS_2D: u32 = 1;
    /// Binding slot for the comparison sampler shared by both atlases.
    pub const SAMPLER: u32 = 2;
    /// Binding slot for the cube-array depth atlas (point lights).
    pub const ATLAS_CUBE: u32 = 3;
}

/// Per-frame GPU resources shadow strategies publish for lit lanes.
///
/// Lit lanes treat this opaquely: they pass it to
/// [`fill_shadow_bind_group_entries`] without inspecting fields. Which
/// concrete strategy filled it (`StandardShadowsLane`,
/// `LowResShadowsLane`, …) is invisible at this seam.
#[derive(Debug, Clone, Copy)]
pub struct ShadowGpuBindings {
    /// 2D depth-array atlas view (directional / spot lights).
    pub atlas_2d: TextureViewId,
    /// Cube-array depth atlas view (point lights).
    pub atlas_cube: TextureViewId,
    /// Comparison sampler used by both atlases.
    pub sampler: SamplerId,
}

/// Returns the three [`BindGroupLayoutEntry`] values shadow contributes
/// to the lit lane's group-3 bind group layout.
pub fn shadow_bind_group_layout_entries() -> [BindGroupLayoutEntry; 3] {
    [
        BindGroupLayoutEntry {
            binding: binding::ATLAS_2D,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::D2Array,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: binding::SAMPLER,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Comparison),
        },
        BindGroupLayoutEntry {
            binding: binding::ATLAS_CUBE,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::CubeArray,
                multisampled: false,
            },
        },
    ]
}

/// Pushes the three shadow-related [`BindGroupEntry`] values into
/// `entries`. Lit lanes call this immediately after pushing their own
/// `LIGHTING_UNIFORMS` entry.
pub fn fill_shadow_bind_group_entries<'a>(
    bindings: &ShadowGpuBindings,
    entries: &mut Vec<BindGroupEntry<'a>>,
) {
    entries.push(BindGroupEntry {
        binding: binding::ATLAS_2D,
        resource: BindingResource::TextureView(bindings.atlas_2d),
        _phantom: std::marker::PhantomData,
    });
    entries.push(BindGroupEntry {
        binding: binding::SAMPLER,
        resource: BindingResource::Sampler(bindings.sampler),
        _phantom: std::marker::PhantomData,
    });
    entries.push(BindGroupEntry {
        binding: binding::ATLAS_CUBE,
        resource: BindingResource::TextureView(bindings.atlas_cube),
        _phantom: std::marker::PhantomData,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_indices_are_unique_and_match_wgsl() {
        assert_eq!(binding::LIGHTING_UNIFORMS, 0);
        assert_eq!(binding::ATLAS_2D, 1);
        assert_eq!(binding::SAMPLER, 2);
        assert_eq!(binding::ATLAS_CUBE, 3);
    }

    #[test]
    fn layout_entries_cover_three_bindings_in_order() {
        let entries = shadow_bind_group_layout_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].binding, binding::ATLAS_2D);
        assert_eq!(entries[1].binding, binding::SAMPLER);
        assert_eq!(entries[2].binding, binding::ATLAS_CUBE);
    }

    #[test]
    fn fill_bind_group_entries_pushes_three() {
        let bindings = ShadowGpuBindings {
            atlas_2d: TextureViewId(1),
            atlas_cube: TextureViewId(2),
            sampler: SamplerId(3),
        };
        let mut entries: Vec<BindGroupEntry> = Vec::new();
        fill_shadow_bind_group_entries(&bindings, &mut entries);
        assert_eq!(entries.len(), 3);
    }
}
