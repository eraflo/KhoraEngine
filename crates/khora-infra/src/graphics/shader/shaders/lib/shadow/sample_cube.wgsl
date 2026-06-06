#define_import_path khora::shadow::sample_cube

#import khora::lighting::structs::PointLight
#import khora::shadow::bindings::{shadow_cube_atlas, shadow_sampler}

/// Core cube-atlas sampling with explicit per-light parameters.
///
/// Reproduces the same non-linear depth value the shadow pass wrote:
///   depth(d) = far/(far - near) - (near*far)/((far - near)*d)
/// where `near = SHADOW_CUBE_NEAR` (injected by ShaderRegistry, must
/// match `Mat4::cube_face_view_proj`'s NEAR constant) and `far` is the
/// per-light far plane. `textureSampleCompareLevel` does cube-face
/// selection in hardware, so the 4-tap PCF around `dir` falls onto the
/// right neighbour without seams.
///
/// Consumers that do not have a `PointLight` struct (e.g. Forward+'s
/// unified `GpuLight`) call this directly with the individual fields.
fn sample_point_shadow_params(
    light_pos: vec3<f32>,
    cube_index: i32,
    bias: f32,
    normal_bias: f32,
    far_plane: f32,
    world_pos: vec3<f32>,
    N: vec3<f32>,
) -> f32 {
    if (cube_index < 0) {
        return 1.0;
    }
    let near_plane = SHADOW_CUBE_NEAR;

    if (far_plane <= near_plane) {
        return 1.0;
    }

    let to_frag = (world_pos + N * normal_bias) - light_pos;
    let dist = length(to_frag);
    if (dist < 1e-4 || dist > far_plane) {
        return 1.0;
    }
    let dir = to_frag / dist;

    let inv_range = 1.0 / (far_plane - near_plane);
    let depth = far_plane * inv_range - (near_plane * far_plane * inv_range) / dist;
    let biased_depth = depth - bias;

    var tangent: vec3<f32>;
    if (abs(dir.y) < 0.95) {
        tangent = normalize(cross(dir, vec3<f32>(0.0, 1.0, 0.0)));
    } else {
        tangent = normalize(cross(dir, vec3<f32>(1.0, 0.0, 0.0)));
    }
    let bitangent = cross(dir, tangent);
    let offset = 0.003;

    let s0 = textureSampleCompareLevel(
        shadow_cube_atlas, shadow_sampler, dir, cube_index, biased_depth);
    let s1 = textureSampleCompareLevel(
        shadow_cube_atlas, shadow_sampler,
        normalize(dir + tangent * offset), cube_index, biased_depth);
    let s2 = textureSampleCompareLevel(
        shadow_cube_atlas, shadow_sampler,
        normalize(dir - tangent * offset), cube_index, biased_depth);
    let s3 = textureSampleCompareLevel(
        shadow_cube_atlas, shadow_sampler,
        normalize(dir + bitangent * offset), cube_index, biased_depth);
    let s4 = textureSampleCompareLevel(
        shadow_cube_atlas, shadow_sampler,
        normalize(dir - bitangent * offset), cube_index, biased_depth);

    return (s0 + s1 + s2 + s3 + s4) / 5.0;
}

/// Convenience wrapper for the lit lane's `PointLight` struct — forwards
/// to the parameter-explicit variant above.
fn sample_point_shadow(
    light: PointLight,
    world_pos: vec3<f32>,
    N: vec3<f32>,
) -> f32 {
    return sample_point_shadow_params(
        light.position.xyz,
        i32(light.shadow_params.x),
        light.shadow_params.y,
        light.shadow_params.z,
        light.shadow_params.w,
        world_pos,
        N,
    );
}
