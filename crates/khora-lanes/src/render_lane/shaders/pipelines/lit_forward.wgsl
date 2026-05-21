// Lit Forward pipeline — multi-light forward rendering with
// Blinn-Phong + Shadow Mapping.
//
// All struct definitions, bindings, and helper functions come from
// the `lib/` modules below. Bumping `MAX_*_LIGHTS` in
// `khora_core::renderer::api::ShaderDefs` automatically reshapes the
// `LightingUniforms` arrays here.

#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::vertex::{VertexInput, VertexOutput}
#import khora::std::material::material
#import khora::lighting::structs::{DirectionalLight, PointLight, SpotLight}
#import khora::lighting::uniforms::lights
#import khora::lighting::attenuation::{calculate_attenuation, calculate_spot_attenuation}
#import khora::lighting::blinn_phong::blinn_phong
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
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_directional_lights && i < #{MAX_DIRECTIONAL_LIGHTS}u; i++) {
        let light = lights.directional_lights[i];
        let L = -normalize(light.direction.xyz);
        let shadow = sample_shadow_pcf(
            light.shadow_view_proj,
            world_position,
            N,
            i32(light.shadow_params.x),
            light.shadow_params.y,
            light.shadow_params.z,
        );
        result += blinn_phong(
            N, V, L,
            light.color.rgb,
            light.color.a,
            diffuse_color,
            specular_power,
        ) * shadow;
    }
    return result;
}

fn calculate_point_lights(
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_point_lights && i < #{MAX_POINT_LIGHTS}u; i++) {
        let light = lights.point_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        if (distance > light.position.w) {
            continue;
        }
        let L = normalize(light_vec);
        let attenuation = calculate_attenuation(distance, light.position.w);
        let shadow = sample_point_shadow(light, world_position, N);
        result += blinn_phong(
            N, V, L,
            light.color.rgb,
            light.color.a * attenuation,
            diffuse_color,
            specular_power,
        ) * shadow;
    }
    return result;
}

fn calculate_spot_lights(
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32,
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var i = 0u; i < lights.num_spot_lights && i < #{MAX_SPOT_LIGHTS}u; i++) {
        let light = lights.spot_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        if (distance > light.position.w) {
            continue;
        }
        let L = normalize(light_vec);
        let distance_attenuation = calculate_attenuation(distance, light.position.w);
        let spot_attenuation = calculate_spot_attenuation(
            L,
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
            N,
            i32(light.shadow_params.x),
            light.shadow_params.y,
            light.shadow_params.z,
        );
        result += blinn_phong(
            N, V, L,
            light.color.rgb,
            light.color.a * total_attenuation,
            diffuse_color,
            specular_power,
        ) * shadow;
    }
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let N = normalize(input.normal);
    let V = normalize(camera.camera_position.xyz - input.world_position);
    let diffuse_color = material.base_color.rgb;

    var final_color = material.ambient * diffuse_color;
    final_color += calculate_directional_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    final_color += calculate_point_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    final_color += calculate_spot_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    final_color += material.emissive;

    final_color = final_color / (final_color + vec3<f32>(1.0));
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(final_color, material.base_color.a);
}
