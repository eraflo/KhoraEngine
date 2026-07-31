// Procedural sky baked into each face of the IBL environment cubemap.
//
// A fullscreen triangle covers one cube face; the fragment reconstructs the
// world-space direction for its texel from the per-face basis (forward /
// right / up chosen to match wgpu's cube-sampling convention) and evaluates a
// gradient sky with a sun disk. Output is linear HDR into the `Rgba16Float`
// env cube. Run once at startup by the IBL bake, six times (one pass per face).
//
// The sun direction comes from the scene's directional light (stamped into the
// per-face uniform by the bake), so the sky, the specular reflections, and the
// shadows all agree on where the light is.

struct FaceBasis {
    // xyz = the face's forward / right / up in world space (w unused). For a
    // fragment at normalized device coords `ndc` in [-1,1]^2, the sampled
    // direction is `normalize(forward + ndc.x*right + ndc.y*up)`.
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    // xyz = unit direction **toward** the sun (w unused), taken from the
    // scene's directional light so the sky agrees with what casts the shadows.
    sun: vec4<f32>,
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

// Linear-space sky colors. The zenith is a saturated blue and the horizon haze
// is a narrow bright band, so the gradient reads as sky rather than as flat
// grey. Placeholder environment until an authored HDR map replaces it.
const ZENITH: vec3<f32> = vec3<f32>(0.09, 0.22, 0.56);
const HORIZON: vec3<f32> = vec3<f32>(0.52, 0.60, 0.72);
const GROUND: vec3<f32> = vec3<f32>(0.16, 0.14, 0.12);

// Sun radiance (linear HDR — far above 1.0, which is the point of baking into
// `Rgba16Float`) and its warm tint.
const SUN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.93, 0.80);
const SUN_INTENSITY: f32 = 22.0;
// Cosines bounding the disk's soft edge — roughly a 2.6° radius. Larger than
// the real sun (0.27°), which at this cube resolution would alias to a
// flickering pixel; this reads as a clean disk and survives the prefilter.
const SUN_COS_OUTER: f32 = 0.99885;
const SUN_COS_INNER: f32 = 0.99955;

// Gradient sky with a sun disk, in linear space: a blue zenith, a bright hazy
// horizon band, a dim warm ground below, and forward-scattered glow around the
// sun. The exponent < 1 tightens the haze against the horizon so most of the
// visible sky keeps its blue.
fn procedural_sky(dir: vec3<f32>) -> vec3<f32> {
    let t = dir.y;
    var color: vec3<f32>;
    if (t >= 0.0) {
        color = mix(HORIZON, ZENITH, pow(clamp(t, 0.0, 1.0), 0.35));
    } else {
        color = mix(HORIZON, GROUND, clamp(-t * 2.5, 0.0, 1.0));
    }

    // Sun disk + halo, confined to the sky hemisphere so the glow does not
    // bleed into the ground half of the cube.
    let mu = dot(dir, normalize(face.sun.xyz));
    let sky_side = smoothstep(-0.05, 0.05, t);
    let disk = smoothstep(SUN_COS_OUTER, SUN_COS_INNER, mu);
    // Tight bloom hugging the disk, plus a broad atmospheric scattering lobe.
    let bloom = pow(max(mu, 0.0), 320.0) * 1.6;
    let scatter = pow(max(mu, 0.0), 8.0) * 0.22;
    color += SUN_COLOR * (disk * SUN_INTENSITY + bloom + scatter) * sky_side;

    return color;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(face.forward.xyz + in.ndc.x * face.right.xyz + in.ndc.y * face.up.xyz);
    return vec4<f32>(procedural_sky(dir), 1.0);
}
