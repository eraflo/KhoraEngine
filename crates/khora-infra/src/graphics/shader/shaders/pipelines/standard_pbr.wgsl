// Standard PBR pipeline — multi-light forward rendering with
// Cook-Torrance BRDF + shadow mapping.
//
// Shares the lit lane's uniform layouts and lib modules (camera /
// model / material / vertex / lighting + shadows), so it slots into
// the same `Group 3 = lighting + shadows` contract as `LitForward`.
// The difference vs LitForward is the BRDF in the fragment shader —
// PBR (GGX + Smith + Schlick) instead of Blinn-Phong.
//
// Bump `MAX_*_LIGHTS` in `khora_core::renderer::api::ShaderDefs` to
// reshape the `LightingUniforms` arrays automatically.

#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::vertex::{VertexInput, VertexOutput}
#import khora::std::material::material
#import khora::std::material_textures::{sample_albedo, sample_metallic_roughness, sample_emissive}
#ifdef HAS_NORMAL_MAP
#import khora::std::material_textures::apply_normal_map
#endif
#import khora::lighting::structs::{DirectionalLight, PointLight, SpotLight}
#import khora::lighting::uniforms::lights
#import khora::lighting::attenuation::{calculate_attenuation, calculate_spot_attenuation}
#import khora::shadow::sample_2d::sample_shadow_pcf
#import khora::shadow::sample_cube::sample_point_shadow

const PI: f32 = 3.14159265359;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    out.world_position = world_pos.xyz;
    out.clip_position = camera.view_projection * world_pos;
    out.normal = normalize((model.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    out.uv = input.uv;
    return out;
}

// Schlick approximation of the Fresnel term.
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - cos_theta, 5.0);
}

// GGX / Trowbridge-Reitz normal distribution.
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom_inner = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom_inner * denom_inner);
}

fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_l = max(dot(n, l), 0.0);
    return geometry_schlick_ggx(n_dot_v, roughness)
        * geometry_schlick_ggx(n_dot_l, roughness);
}

// Cook-Torrance BRDF contribution for a single light, multiplied by
// `n_dot_l * radiance`. `albedo` is the base color, `metallic` and
// `roughness` come from the material. `radiance` is `light.color *
// light.intensity * attenuation`.
fn cook_torrance(
    n: vec3<f32>,
    v: vec3<f32>,
    l: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }

    // F0 = 0.04 for dielectrics, lerps to albedo for metals.
    var f0 = vec3<f32>(0.04);
    f0 = mix(f0, albedo, metallic);

    let ndf = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);

    let numerator = ndf * g * f;
    let denominator = 4.0 * max(dot(n, v), 0.0) * n_dot_l + 0.001;
    let specular = numerator / denominator;

    // Energy conservation: diffuse is the complement of the reflected
    // fraction, scaled by `1 - metallic` because metals have no diffuse.
    let k_s = f;
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    return (diffuse + specular) * radiance * n_dot_l;
}

fn calculate_directional_lights(
    world_position: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_directional_lights && i < #{MAX_DIRECTIONAL_LIGHTS}u; i++) {
        let light = lights.directional_lights[i];
        let l = -normalize(light.direction.xyz);
        let shadow = sample_shadow_pcf(
            light.shadow_view_proj,
            world_position,
            n,
            i32(light.shadow_params.x),
            light.shadow_params.y,
            light.shadow_params.z,
        );
        let radiance = light.color.rgb * light.color.a;
        result += cook_torrance(n, v, l, albedo, metallic, roughness, radiance) * shadow;
    }
    return result;
}

fn calculate_point_lights(
    world_position: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_point_lights && i < #{MAX_POINT_LIGHTS}u; i++) {
        let light = lights.point_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        if (distance > light.position.w) {
            continue;
        }
        let l = normalize(light_vec);
        let attenuation = calculate_attenuation(distance, light.position.w);
        let shadow = sample_point_shadow(light, world_position, n);
        let radiance = light.color.rgb * light.color.a * attenuation;
        result += cook_torrance(n, v, l, albedo, metallic, roughness, radiance) * shadow;
    }
    return result;
}

fn calculate_spot_lights(
    world_position: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_spot_lights && i < #{MAX_SPOT_LIGHTS}u; i++) {
        let light = lights.spot_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        if (distance > light.position.w) {
            continue;
        }
        let l = normalize(light_vec);
        let distance_attenuation = calculate_attenuation(distance, light.position.w);
        let spot_attenuation = calculate_spot_attenuation(
            l,
            normalize(light.direction.xyz),
            light.direction.w,
            light.params.x,
        );
        let total_attenuation = distance_attenuation * spot_attenuation;
        if (total_attenuation <= 0.0) {
            continue;
        }
        let shadow = sample_shadow_pcf(
            light.shadow_view_proj,
            world_position,
            n,
            i32(light.shadow_params.x),
            light.shadow_params.y,
            light.shadow_params.z,
        );
        let radiance = light.color.rgb * light.color.a * total_attenuation;
        result += cook_torrance(n, v, l, albedo, metallic, roughness, radiance) * shadow;
    }
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let base = sample_albedo(input.uv);
    let albedo = material.base_color.rgb * base.rgb;
    // glTF metallic-roughness texture × scalar factors (white fallback ⇒
    // factors pass through).
    let mr = sample_metallic_roughness(input.uv);
    let metallic = clamp(material.pbr_factors.x * mr.x, 0.0, 1.0);
    let roughness = clamp(material.pbr_factors.y * mr.y, 0.05, 1.0);
    // Geometric normal, perturbed by the tangent-space normal map only for
    // materials whose variant declares one (`HAS_NORMAL_MAP`).
    let geometric_normal = normalize(input.normal);
#ifdef HAS_NORMAL_MAP
    let n = apply_normal_map(geometric_normal, input.world_position, input.uv);
#else
    let n = geometric_normal;
#endif
    let v = normalize(camera.camera_position.xyz - input.world_position);

    var color = material.ambient * albedo;
    color += calculate_directional_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += calculate_point_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += calculate_spot_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += material.emissive * sample_emissive(input.uv);

    // Reinhard tone-map + gamma.
    color = color / (color + vec3<f32>(1.0));
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, material.base_color.a * base.a);
}
