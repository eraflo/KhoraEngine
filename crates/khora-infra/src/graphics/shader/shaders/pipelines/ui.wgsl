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

//! Unified UI shader supporting rounded rectangles, borders, and textures.

struct UiUniforms {
    view_proj: mat4x4<f32>,
};

struct UiInstance {
    pos: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
    // x = border_radius, y = border_width, z = has_texture (0.0 or 1.0), w = unused
    params: vec4<f32>,
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
};

@group(0) @binding(0) var<uniform> global: UiUniforms;
@group(1) @binding(0) var<storage, read> instances: array<UiInstance>;

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
    let instance = instances[instance_index];
    
    // Quad vertices: (0,0), (1,0), (1,1), (0,1)
    var pos = vec2<f32>(0.0, 0.0);
    if (vertex_index == 1u || vertex_index == 2u) { pos.x = 1.0; }
    if (vertex_index == 2u || vertex_index == 3u) { pos.y = 1.0; }
    
    // Remap UV to [uv_min, uv_max]
    let uv = mix(instance.uv_min, instance.uv_max, pos);
    
    // Transform to world space
    let world_pos = instance.pos + pos * instance.size;
    
    var out: VertexOutput;
    out.clip_position = global.view_proj * vec4<f32>(world_pos, 0.0, 1.0);
    out.uv = uv;
    out.instance_index = instance_index;
    return out;
}

@group(2) @binding(0) var t_diffuse: texture_2d<f32>;
@group(2) @binding(1) var s_diffuse: sampler;

/// Signed Distance Function for a rounded rectangle.
fn sdf_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let instance = instances[in.instance_index];
    let half_size = instance.size * 0.5;
    
    // Convert UV [0, 1] to centered coordinates [-half_size, half_size]
    let p = (in.uv - 0.5) * instance.size;
    
    // Calculate SDF
    let dist = sdf_rounded_rect(p, half_size, instance.params.x);
    
    // Antialiasing using fwidth
    let edge_softness = fwidth(dist);
    let alpha = 1.0 - smoothstep(-edge_softness, edge_softness, dist);
    
    if (alpha <= 0.0) {
        discard;
    }

    // Border handling
    let border_width = instance.params.y;
    var final_color = instance.color;
    var final_alpha = alpha;
    
    if (border_width > 0.0) {
        let border_dist = dist + border_width;
        let border_edge = smoothstep(-edge_softness, edge_softness, border_dist);
        let inner_edge = smoothstep(-edge_softness, edge_softness, dist);
        let border_alpha = inner_edge - border_edge;
        
        // Mix border color with the main color based on border alpha
        let border_color = vec4<f32>(instance.color.rgb * 0.5, 1.0);
        final_color = mix(border_color, instance.color, border_edge);
        final_alpha = max(alpha, border_alpha);
    }

    // Texture support
    if (instance.params.z > 0.5) {
        let tex_color = textureSample(t_diffuse, s_diffuse, in.uv);
        final_color = final_color * tex_color;
    }
    
    return vec4<f32>(final_color.rgb, final_color.a * final_alpha);
}
