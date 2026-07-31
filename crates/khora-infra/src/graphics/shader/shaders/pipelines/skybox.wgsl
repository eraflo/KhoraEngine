// Skybox background — draws the IBL environment cube as the scene backdrop.
//
// A fullscreen triangle covers the framebuffer at the far plane (clip z = 1).
// For each pixel the fragment reconstructs the world-space view ray from the
// inverse view-projection and samples the environment cube in that direction,
// so the visible sky is exactly the environment the lit surfaces reflect —
// the specular reflections finally have a referent on screen.
//
// Drawn FIRST in the forward pass with depth-write disabled and a `LessEqual`
// test against the cleared depth (1.0): it fills every pixel but occludes
// nothing, so the meshes drawn afterwards (depth `Less`, depth-write on) paint
// over it wherever geometry exists and the sky shows through everywhere else.
//
// Group 0 is a skybox-private uniform (inverse view-projection + camera
// position); group 1 is the environment cube + its filtering sampler. Neither
// collides with the lit pass's own group 0 (camera) / group 3 (lighting), which
// are rebound before the mesh loop.

#import khora::lighting::pbr::tonemap_reinhard

struct SkyUniforms {
    // Inverse of `proj * view`: maps a clip-space point back to world space so
    // the fragment can recover its view direction.
    inv_view_proj: mat4x4<f32>,
    // World-space camera origin (w unused) — the ray's start point.
    camera_pos: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> sky: SkyUniforms;

@group(1) @binding(0)
var env_cube: texture_cube<f32>;
@group(1) @binding(1)
var env_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Fullscreen triangle: uv in {(0,0),(2,0),(0,2)} → clip xy in [-1,3].
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    let p = uv * 2.0 - 1.0;
    var out: VsOut;
    // z = w = 1 → depth 1.0 (the far plane) so `LessEqual` keeps the sky behind
    // every mesh.
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.ndc = p;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Reconstruct the world-space view ray: unproject the far-plane clip point,
    // then aim it from the camera origin.
    let far = sky.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let world = far.xyz / far.w;
    let dir = normalize(world - sky.camera_pos.xyz);
    let color = textureSampleLevel(env_cube, env_sampler, dir, 0.0).rgb;
    // Match the lit lanes' tone-map so the background exposure agrees with the
    // shaded surfaces in front of it.
    return vec4<f32>(tonemap_reinhard(color), 1.0);
}
