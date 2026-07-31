// Diffuse irradiance convolution — bakes one face of the small irradiance
// cube from the environment cube.
//
// For each output texel the fragment reconstructs its world direction N (via
// the per-face basis, same convention as the sky bake), then integrates the
// cosine-weighted incoming radiance over the hemisphere around N by sampling
// the environment cube. The result is the diffuse irradiance arriving at a
// surface whose normal is N. Run once at startup, six times (one per face).

const PI: f32 = 3.14159265359;

struct FaceBasis {
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(face.forward.xyz + in.ndc.x * face.right.xyz + in.ndc.y * face.up.xyz);

    // Tangent basis around N for hemisphere sampling.
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.999) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(cross(up, n));
    let tangent_up = normalize(cross(n, right));

    var irradiance = vec3<f32>(0.0);
    var samples = 0.0;
    let delta = 0.025;
    var phi = 0.0;
    loop {
        if (phi >= 2.0 * PI) { break; }
        var theta = 0.0;
        loop {
            if (theta >= 0.5 * PI) { break; }
            // Tangent-space sample → world.
            let ts = vec3<f32>(sin(theta) * cos(phi), sin(theta) * sin(phi), cos(theta));
            let dir = ts.x * right + ts.y * tangent_up + ts.z * n;
            let radiance = textureSampleLevel(env_cube, env_sampler, dir, 0.0).rgb;
            // Cosine-weighted (cos θ) with the solid-angle sin θ factor.
            irradiance += radiance * cos(theta) * sin(theta);
            samples += 1.0;
            theta += delta;
        }
        phi += delta;
    }
    // Lambertian normalisation: π · average of the weighted samples.
    irradiance = PI * irradiance / max(samples, 1.0);
    return vec4<f32>(irradiance, 1.0);
}
