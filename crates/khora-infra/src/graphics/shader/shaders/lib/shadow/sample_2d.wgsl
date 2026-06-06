#define_import_path khora::shadow::sample_2d

#import khora::shadow::bindings::{shadow_atlas, shadow_sampler}

/// Samples the 2D shadow atlas with 3×3 PCF.
/// Returns 1.0 = fully lit, 0.0 = fully in shadow.
fn sample_shadow_pcf(
    shadow_vp: mat4x4<f32>,
    world_pos: vec3<f32>,
    N: vec3<f32>,
    atlas_index: i32,
    bias: f32,
    normal_bias: f32,
) -> f32 {
    if (atlas_index < 0) {
        return 1.0;
    }

    let biased_pos = world_pos + N * normal_bias;
    let light_clip = shadow_vp * vec4<f32>(biased_pos, 1.0);
    let light_ndc = light_clip.xyz / light_clip.w;

    let shadow_uv = vec2<f32>(
        light_ndc.x * 0.5 + 0.5,
        1.0 - (light_ndc.y * 0.5 + 0.5),
    );

    if (shadow_uv.x < 0.0 || shadow_uv.x > 1.0 || shadow_uv.y < 0.0 || shadow_uv.y > 1.0) {
        return 1.0;
    }

    let depth = light_ndc.z - bias;

    // PCF texel size is hardcoded against the StandardShadowsLane atlas
    // size (2048²). LowResShadowsLane uses a different atlas size but
    // PCF stays in normalized UV space, so a slightly larger relative
    // step on a smaller atlas is acceptable. A future iteration can
    // inject this via `#define` based on the active strategy.
    let texel_size = 1.0 / 2048.0;
    var shadow = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel_size;
            shadow += textureSampleCompareLevel(
                shadow_atlas,
                shadow_sampler,
                shadow_uv + offset,
                atlas_index,
                depth,
            );
        }
    }
    return shadow / 9.0;
}
