#define_import_path khora::lighting::blinn_phong

/// Blinn-Phong BRDF — diffuse (Lambert) + specular (half-vector
/// exponentiated to `specular_power`). Both terms are modulated by the
/// light color × intensity.
fn blinn_phong(
    N: vec3<f32>,
    V: vec3<f32>,
    L: vec3<f32>,
    light_color: vec3<f32>,
    light_intensity: f32,
    diffuse_color: vec3<f32>,
    specular_power: f32,
) -> vec3<f32> {
    let NdotL = max(dot(N, L), 0.0);
    let diffuse = diffuse_color * NdotL;

    let H = normalize(L + V);
    let NdotH = max(dot(N, H), 0.0);
    var specular_strength = 0.0;
    if (NdotL > 0.0) {
        specular_strength = pow(NdotH, specular_power);
    }
    let specular = vec3<f32>(specular_strength);

    return (diffuse + specular) * light_color * light_intensity;
}
