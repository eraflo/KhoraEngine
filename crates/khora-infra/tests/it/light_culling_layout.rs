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

//! The light-culling uniforms the renderer uploads have the layout of the
//! `LightCullingUniforms` struct `light_culling.wgsl` declares, as naga lays it
//! out for a uniform buffer: same size, same fields at the same offsets.

use std::mem::{offset_of, size_of};

use khora_core::renderer::api::scene::CullingUniformsData;

const LIGHT_CULLING_WGSL: &str =
    include_str!("../../src/graphics/shader/shaders/pipelines/light_culling.wgsl");

/// `(name, offset)` of each member of the struct bound to the WGSL global
/// `uniforms`, in declaration order, and the struct's size.
fn wgsl_uniform_layout() -> (Vec<(String, u32)>, u32) {
    let module = naga::front::wgsl::parse_str(LIGHT_CULLING_WGSL)
        .unwrap_or_else(|e| panic!("light_culling.wgsl does not parse: {e}"));
    let (_, global) = module
        .global_variables
        .iter()
        .find(|(_, var)| var.name.as_deref() == Some("uniforms"))
        .expect("light_culling.wgsl declares `uniforms`");
    assert_eq!(global.space, naga::AddressSpace::Uniform);
    match &module.types[global.ty].inner {
        naga::TypeInner::Struct { members, span } => (
            members
                .iter()
                .map(|m| (m.name.clone().unwrap_or_default(), m.offset))
                .collect(),
            *span,
        ),
        other => panic!("`uniforms` is not a struct: {other:?}"),
    }
}

#[test]
fn culling_uniforms_match_the_wgsl_struct_layout() {
    let (members, span) = wgsl_uniform_layout();

    let rust: Vec<(String, u32)> = [
        (
            "view_projection",
            offset_of!(CullingUniformsData, view_projection),
        ),
        (
            "inverse_projection",
            offset_of!(CullingUniformsData, inverse_projection),
        ),
        (
            "screen_dimensions",
            offset_of!(CullingUniformsData, screen_dimensions),
        ),
        ("tile_count", offset_of!(CullingUniformsData, tile_count)),
        ("num_lights", offset_of!(CullingUniformsData, num_lights)),
        ("tile_size", offset_of!(CullingUniformsData, tile_size)),
        ("_padding", offset_of!(CullingUniformsData, _padding)),
    ]
    .into_iter()
    .map(|(name, offset)| (name.to_owned(), offset as u32))
    .collect();

    assert_eq!(rust, members);
    assert_eq!(size_of::<CullingUniformsData>() as u32, span);
}

/// A uniform buffer binding is sized in multiples of 16 bytes.
#[test]
fn culling_uniforms_fill_whole_16_byte_rows() {
    assert_eq!(size_of::<CullingUniformsData>() % 16, 0);
}
