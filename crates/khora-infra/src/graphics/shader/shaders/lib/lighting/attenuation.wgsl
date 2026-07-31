#define_import_path khora::lighting::attenuation

// Distance-and-cone attenuation helpers shared by every shading model
// (Blinn-Phong, PBR, future variants).

/// Smooth distance attenuation that reaches zero at `range`.
fn calculate_attenuation(distance: f32, range: f32) -> f32 {
    let normalized_distance = distance / range;
    let attenuation = saturate(1.0 - normalized_distance * normalized_distance);
    return attenuation * attenuation;
}

/// Spotlight cone attenuation: smoothstep between inner and outer cone.
fn calculate_spot_attenuation(
    light_dir: vec3<f32>,
    spot_direction: vec3<f32>,
    inner_cone_cos: f32,
    outer_cone_cos: f32,
) -> f32 {
    let cos_angle = dot(-light_dir, spot_direction);
    return smoothstep(outer_cone_cos, inner_cone_cos, cos_angle);
}
