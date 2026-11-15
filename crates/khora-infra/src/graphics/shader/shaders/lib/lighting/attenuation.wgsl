#define_import_path khora::lighting::attenuation

// Distance-and-cone attenuation helpers shared by every shading model.

/// Smooth distance attenuation that reaches zero at `range`.
///
/// `range` is a finite positive distance: a light whose range is not never
/// reaches the shader (`shining_lights` on the CPU keeps it out).
fn calculate_attenuation(distance: f32, range: f32) -> f32 {
    let normalized_distance = distance / range;
    let attenuation = saturate(1.0 - normalized_distance * normalized_distance);
    return attenuation * attenuation;
}

/// Spotlight cone attenuation: full light inside the inner cone, none past the
/// outer one, a smooth fade between.
///
/// The fade is `smoothstep`'s, written out: WGSL leaves `smoothstep` undefined
/// when `low >= high`, and equal cones are an author's hard-edged spot (both
/// cosines also round to 1 under about 3e-4 rad). The angles are never changed.
fn calculate_spot_attenuation(
    light_dir: vec3<f32>,
    spot_direction: vec3<f32>,
    inner_cone_cos: f32,
    outer_cone_cos: f32,
) -> f32 {
    let cos_angle = dot(-light_dir, spot_direction);
    let span = inner_cone_cos - outer_cone_cos;
    if (span <= 0.0) {
        // On the edge counts as inside: a cone so thin its cosines round to 1
        // still lights its own axis.
        return select(0.0, 1.0, cos_angle >= outer_cone_cos);
    }
    let t = saturate((cos_angle - outer_cone_cos) / span);
    return t * t * (3.0 - 2.0 * t);
}
