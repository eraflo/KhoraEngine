// Test probe, not an engine pipeline: evaluates the shared spot-cone falloff
// once per input sample so `tests/it/spot_attenuation_gpu.rs` can read every
// value back. It is composed through the `PipelineSystem` like a pipeline, so
// the function it runs is the one every lit pipeline imports.
//
// It has no `#define_import_path`, so a hot reload of this file recomposes
// nothing.

#import khora::lighting::attenuation::calculate_spot_attenuation

struct SpotSample {
    // xyz: from the shaded point towards the light (`light_dir`); w unused.
    light_dir: vec4<f32>,
    // xyz: the spot's axis, unit length; w unused.
    spot_direction: vec4<f32>,
    // x: cosine of the inner cone angle; y: cosine of the outer cone angle.
    cones: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> samples: array<SpotSample>;
@group(0) @binding(1) var<storage, read_write> falloffs: array<f32>;

@compute @workgroup_size(64)
fn evaluate(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= arrayLength(&samples)) {
        return;
    }
    let s = samples[i];
    falloffs[i] = calculate_spot_attenuation(
        s.light_dir.xyz,
        s.spot_direction.xyz,
        s.cones.x,
        s.cones.y,
    );
}
