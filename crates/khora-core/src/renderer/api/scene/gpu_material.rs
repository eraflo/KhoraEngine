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
use crate::renderer::api::resource::{BufferId, SamplerId, TextureViewId};

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
    /// Filtering sampler shared by all maps.
    pub sampler: SamplerId,
    /// Prebuilt group-2 bind group bound by lit lanes.
    pub bind_group: BindGroupId,
    /// Shader variant (`HAS_*` texture flags) this material was built for.
    /// The lit lane requests the matching pipeline and groups draws by it.
    pub variant: ShaderVariantKey,
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
            sampler: self.sampler,
        }
    }
}

impl Asset for GpuMaterial {}
