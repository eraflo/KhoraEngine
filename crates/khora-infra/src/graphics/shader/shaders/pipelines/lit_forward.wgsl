// Lit Forward pipeline — multi-light forward rendering with the shared
// Cook-Torrance PBR BRDF + shadow mapping.
//
// All struct definitions, bindings, and helper functions come from
// the `lib/` modules below. Bumping `MAX_*_LIGHTS` in
// `khora_core::renderer::api::ShaderDefs` automatically reshapes the
// `LightingUniforms` arrays here.
//
// The BRDF (`khora::lighting::pbr`) and the output tone-map are shared
// with `standard_pbr` and `forward_plus`, so the three lit lanes render
// the same image — this lane differs only in how it iterates lights.

#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::vertex::{VertexInput, VertexOutput}
#import khora::std::material::material
#import khora::std::material_textures::{sample_albedo, sample_metallic_roughness, sample_emissive, sample_occlusion}
#ifdef HAS_NORMAL_MAP
#import khora::std::material_textures::apply_normal_map
#endif
#import khora::lighting::structs::{DirectionalLight, PointLight, SpotLight}
#import khora::lighting::uniforms::lights
#import khora::lighting::attenuation::{calculate_attenuation, calculate_spot_attenuation}
#import khora::lighting::pbr::{cook_torrance, tonemap_reinhard}
#import khora::shadow::sample_2d::sample_shadow_pcf
#import khora::shadow::sample_cube::sample_point_shadow

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
    // Alpha-mask discard: pbr_factors.z is the cutoff (0 for Opaque/Blend, so
    // the test never fires for them).
    let out_alpha = material.base_color.a * base.a;
    if (out_alpha < material.pbr_factors.z) {
        discard;
    }
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

    // AO attenuates the indirect (ambient) term only — never direct light.
    let ao = sample_occlusion(input.uv);
    var color = material.ambient * albedo * ao;
    color += calculate_directional_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += calculate_point_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += calculate_spot_lights(input.world_position, n, v, albedo, metallic, roughness);
    color += material.emissive * sample_emissive(input.uv);

    color = tonemap_reinhard(color);

    return vec4<f32>(color, out_alpha);
}
