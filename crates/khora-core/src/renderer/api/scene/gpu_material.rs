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
use crate::renderer::api::pipeline::ShaderVariantKey;
use crate::renderer::api::resource::{BufferId, SamplerId, TextureId, TextureViewId};

/// A GPU-ready representation of a material: its uniform buffer, the PBR
/// texture views it declares, the shared sampler, and a ready-to-bind
/// group-2 bind group.
///
/// Built once per material UUID by the data-layer projection (mirroring
/// [`GpuMesh`](super::GpuMesh)), cached, and consumed by every lit lane —
/// which selects the pipeline for `variant` and binds `bind_group` at
/// group 2. Each texture view is `Some` only for a map the material
/// declares (no fallback textures); the [`variant`](Self::variant) records
/// exactly that set of `HAS_*` flags, so the bind group, the group-2 layout
/// it was built against, and the lit pipeline's group-2 layout all resolve
/// from the same `(LayoutKey::Material, variant)`.
///
/// Each declared map is uploaded as a texture private to this material (the
/// projection does not share GPU textures across materials), so the material
/// **exclusively owns** its `uniform_buffer`, its four `*_texture` /
/// `*_view` handles, and its `bind_group`. The `sampler` is the opposite:
/// it is the engine-shared filtering sampler. This ownership split is what
/// asset eviction relies on to free the resources — it destroys everything
/// but the shared sampler when the material is reclaimed. The `*_texture`
/// and matching `*_view` fields are always `Some`/`None` together.
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
    /// Filtering sampler shared by all maps (engine-owned; NOT freed on eviction).
    pub sampler: SamplerId,
    /// Prebuilt group-2 bind group bound by lit lanes.
    pub bind_group: BindGroupId,
    /// Shader variant (`HAS_*` texture flags) this material was built for.
    /// The lit lane requests the matching pipeline and groups draws by it.
    pub variant: ShaderVariantKey,
    /// Whether the material renders double-sided (back faces not culled). The
    /// lit lane selects a no-cull pipeline for it; single-sided materials cull
    /// back faces. Part of the pipeline cache key via [`PipelineKey`].
    pub double_sided: bool,
    /// Whether the material is alpha-*blended* (`AlphaMode::Blend`).
    ///
    /// Blended materials need a different pipeline state (alpha blending on,
    /// depth writes off) **and** a different draw order: the lit lane defers
    /// them to a second, back-to-front sorted batch after the opaque draws,
    /// because blending is order-dependent. `AlphaMode::Mask` is *not* blended
    /// — it discards in the shader and stays in the opaque batch.
    pub blend: bool,
}

impl GpuMaterial {
    /// Returns the [`MaterialGpuBindings`] view of this material's
    /// resources, suitable for
    /// [`fill_material_bind_group_entries`](crate::renderer::api::material::fill_material_bind_group_entries).
    pub fn bindings(&self) -> MaterialGpuBindings {
        MaterialGpuBindings {
            uniform_buffer: self.uniform_buffer,
            base_color: self.base_color_view,
            metallic_roughness: self.metallic_roughness_view,
            normal: self.normal_view,
            emissive: self.emissive_view,
            occlusion: self.occlusion_view,
            sampler: self.sampler,
        }
    }
}

impl Asset for GpuMaterial {}
