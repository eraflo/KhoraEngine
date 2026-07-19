// Procedural sky baked into each face of the IBL environment cubemap.
//
// A fullscreen triangle covers one cube face; the fragment reconstructs the
// world-space direction for its texel from the per-face basis (forward /
// right / up chosen to match wgpu's cube-sampling convention) and evaluates a
// simple gradient sky. Output is linear HDR into the `Rgba16Float` env cube.
// Run once at startup by the IBL bake, six times (one pass per face).

struct FaceBasis {
    // xyz = the face's forward / right / up in world space (w unused). For a
    // fragment at normalized device coords `ndc` in [-1,1]^2, the sampled
    // direction is `normalize(forward + ndc.x*right + ndc.y*up)`.
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> face: FaceBasis;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Fullscreen triangle: uv in {(0,0),(2,0),(0,2)} → clip in [-1,3].
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    var out: VsOut;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.ndc = uv * 2.0 - 1.0;
    return out;
}

// Simple gradient sky in linear space: a blue zenith, a bright hazy horizon,
// and a dim warm ground below. Placeholder environment until an authored HDR
// map replaces it.
fn procedural_sky(dir: vec3<f32>) -> vec3<f32> {
    // Linear values — a moderately bright sky so the indirect (ambient) term
    // reads clearly on shaded surfaces without washing them out.
    let zenith = vec3<f32>(0.25, 0.40, 0.70);
    let horizon = vec3<f32>(0.60, 0.65, 0.72);
    let ground = vec3<f32>(0.18, 0.16, 0.13);
    let t = dir.y;
    if (t >= 0.0) {
        return mix(horizon, zenith, pow(clamp(t, 0.0, 1.0), 0.5));
    }
    return mix(horizon, ground, clamp(-t, 0.0, 1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(face.forward.xyz + in.ndc.x * face.right.xyz + in.ndc.y * face.up.xyz);
    return vec4<f32>(procedural_sky(dir), 1.0);
}
