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

//! UI glyphs: one instanced quad per glyph, sampling the glyph atlases.
//!
//! A mask glyph is its coverage in the mask atlas, painted in the text's
//! colour; a colour glyph (an emoji) is its own colours from the colour
//! atlas, faded by the text's alpha. Each quad is placed at whole pixels and
//! as large as its image, so a texel lands on a pixel.

struct UiUniforms {
    view_proj: mat4x4<f32>,
};

/// One glyph — the layout of the quads' `UiInstance`.
struct GlyphInstance {
    /// Top-left corner, in UI pixels.
    pos: vec2<f32>,
    /// Size, in pixels: the glyph image's.
    size: vec2<f32>,
    /// The text's colour, straight alpha.
    color: vec4<f32>,
    // x = 1.0 for a colour glyph, 0.0 for a mask; y, z, w unused.
    params: vec4<f32>,
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
};

@group(0) @binding(0) var<uniform> global: UiUniforms;
@group(1) @binding(0) var<storage, read> glyphs: array<GlyphInstance>;
@group(2) @binding(0) var t_mask: texture_2d<f32>;
@group(2) @binding(1) var t_color: texture_2d<f32>;
@group(2) @binding(2) var s_glyph: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) instance_index: u32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let glyph = glyphs[instance_index];

    // Quad corners in triangle-strip order: (0,0), (1,0), (0,1), (1,1).
    var corner = vec2<f32>(0.0, 0.0);
    if (vertex_index == 1u || vertex_index == 3u) { corner.x = 1.0; }
    if (vertex_index == 2u || vertex_index == 3u) { corner.y = 1.0; }

    var out: VertexOutput;
    out.clip_position = global.view_proj * vec4<f32>(glyph.pos + corner * glyph.size, 0.0, 1.0);
    out.uv = mix(glyph.uv_min, glyph.uv_max, corner);
    out.instance_index = instance_index;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let glyph = glyphs[in.instance_index];
    // Both atlases are sampled in uniform control flow; the glyph's kind
    // picks the one it is in.
    let coverage = textureSample(t_mask, s_glyph, in.uv).r;
    let colour = textureSample(t_color, s_glyph, in.uv);
    let mask = vec4<f32>(glyph.color.rgb, glyph.color.a * coverage);
    let emoji = vec4<f32>(colour.rgb, colour.a * glyph.color.a);
    let out = select(mask, emoji, glyph.params.x > 0.5);
    if (out.a <= 0.0) {
        discard;
    }
    return out;
}
