// Equirectangular → cubemap projection — bakes one face of the environment
// cube from an authored equirectangular (lat-long) environment map.
//
// This is the authored-asset counterpart of `ibl_sky`: same output (one face of
// the linear-HDR `Rgba16Float` env cube), but the radiance comes from a loaded
// texture instead of a procedural gradient. Everything downstream — irradiance
// convolution, specular prefilter, skybox — is unchanged, since they all read
// the env cube and never know how it was produced.
//
// For each output texel the fragment reconstructs its world direction from the
// per-face basis (same convention as the sky bake), converts that direction to
// lat-long UV, and samples the source map. Run once at startup, six times.

const PI: f32 = 3.14159265359;
const INV_TWO_PI: f32 = 0.15915494309;
const INV_PI: f32 = 0.31830988618;

struct FaceBasis {
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var equirect: texture_2d<f32>;
@group(0) @binding(1) var equirect_sampler: sampler;
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

// Maps a unit direction to equirectangular UV: longitude from atan2 wrapped
// into [0,1), latitude from the polar angle so v = 0 is straight up (+Y), which
// is how lat-long environment maps are authored.
fn direction_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let u = atan2(dir.z, dir.x) * INV_TWO_PI + 0.5;
    let v = acos(clamp(dir.y, -1.0, 1.0)) * INV_PI;
    return vec2<f32>(u, v);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(face.forward.xyz + in.ndc.x * face.right.xyz + in.ndc.y * face.up.xyz);
    let uv = direction_to_equirect_uv(dir);
    // Explicit LOD: the fragment's neighbours can straddle the u wrap-around
    // seam, which would make implicit derivatives select a garbage mip.
    let radiance = textureSampleLevel(equirect, equirect_sampler, uv, 0.0).rgb;
    return vec4<f32>(radiance, 1.0);
}
