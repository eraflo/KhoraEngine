// Prefiltered specular environment bake — one mip level of one cube face.
//
// For each output texel the fragment reconstructs its world direction N (=
// the reflection/view direction under the split-sum approximation), then GGX
// importance-samples the environment cube for this mip's roughness and
// accumulates the N·L-weighted radiance. Mip 0 (roughness 0) reduces to a
// mirror sample; higher mips (higher roughness) blur wider. Roughness is
// passed per mip in `face.forward.w`. Fullscreen-triangle pass, one per
// (mip, face).

const PI: f32 = 3.14159265359;
const SAMPLE_COUNT: u32 = 256u;

struct FaceBasis {
    // xyz = forward / right / up; forward.w = this mip's roughness.
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var env_cube: texture_cube<f32>;
@group(0) @binding(1) var env_sampler: sampler;
@group(0) @binding(2) var<uniform> face: FaceBasis;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    var out: VsOut;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.ndc = uv * 2.0 - 1.0;
    return out;
}

fn radical_inverse_vdc(bits_in: u32) -> f32 {
    var bits = bits_in;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse_vdc(i));
}

fn importance_sample_ggx(xi: vec2<f32>, n: vec3<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let phi = 2.0 * PI * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
    let h_tangent = vec3<f32>(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
    var up = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(n.z) >= 0.999) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);
    return normalize(tangent * h_tangent.x + bitangent * h_tangent.y + n * h_tangent.z);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(face.forward.xyz + in.ndc.x * face.right.xyz + in.ndc.y * face.up.xyz);
    let roughness = face.forward.w;
    let r = n;
    let v = n;

    var prefiltered = vec3<f32>(0.0);
    var total_weight = 0.0;
    for (var i = 0u; i < SAMPLE_COUNT; i++) {
        let xi = hammersley(i, SAMPLE_COUNT);
        let h = importance_sample_ggx(xi, n, roughness);
        let l = normalize(2.0 * dot(v, h) * h - v);
        let n_dot_l = max(dot(n, l), 0.0);
        if (n_dot_l > 0.0) {
            prefiltered += textureSampleLevel(env_cube, env_sampler, l, 0.0).rgb * n_dot_l;
            total_weight += n_dot_l;
        }
    }
    prefiltered = prefiltered / max(total_weight, 0.001);
    return vec4<f32>(prefiltered, 1.0);
}
