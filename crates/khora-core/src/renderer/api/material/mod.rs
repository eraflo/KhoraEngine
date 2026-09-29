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

//! Shared material API — the abstract contract for the material bind
//! group (group 2 of every lit pipeline).
//!
//! The material's GPU representation is a per-UUID projection
//! ([`GpuMaterial`]) built once
//! by the data layer, not rebuilt per draw by each lane. The bind-group
//! contract for group 2 lives in [`bindings`]:
//!
//! | Binding | Resource                              | sRGB? |
//! |---------|---------------------------------------|-------|
//! | 0       | `MaterialUniforms` uniform buffer     | —     |
//! | 1       | Base-color (albedo) texture           | sRGB  |
//! | 2       | Metallic-roughness texture (B=M, G=R) | linear|
//! | 3       | Normal texture (tangent space)        | linear|
//! | 4       | Emissive texture                      | sRGB  |
//! | 5       | Filtering sampler shared by all maps  | —     |
//!
//! Lit lanes call [`bindings::material_bind_group_layout_entries`] when
//! creating their pipeline layout's group-2 slot, and the data-layer
//! projection calls [`bindings::fill_material_bind_group_entries`] when
//! building each material's bind group. The matching WGSL — bindings +
//! sampling helpers — lives in `khora-infra/src/graphics/shader/shaders/
//! lib/std/material_textures.wgsl` and is composed into every lit shader
//! by the `PipelineSystem` backend.

pub mod bindings;
pub mod gpu_material;
pub mod uniforms;

pub use self::gpu_material::GpuMaterial;
pub use self::uniforms::MaterialUniforms;

pub use bindings::{
    fill_material_bind_group_entries, material_bind_group_layout_entries,
    material_layout_entries_for_variant, MaterialGpuBindings,
};
