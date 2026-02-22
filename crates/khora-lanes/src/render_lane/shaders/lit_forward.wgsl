// Lit Forward Shader
// Multi-light forward rendering with Blinn-Phong lighting + Shadow Mapping
// Supports: 4 directional + 16 point + 8 spot lights

// --- Vertex Shader ---

// Camera uniform block containing view and projection matrices
struct CameraUniforms {
    view_projection: mat4x4<f32>,  // Combined projection * view matrix
    camera_position: vec4<f32>,     // Camera position in world space (w is padding)
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

// Vertex input from the vertex buffer
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

// Output from vertex shader to fragment shader
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

// Model transform uniform
struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    
    // Transform vertex position to world space
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    out.world_position = world_pos.xyz;
    
    // Transform to clip space
    out.clip_position = camera.view_projection * world_pos;
    
    // Transform normal to world space (using normal matrix to handle non-uniform scales)
    out.normal = normalize((model.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    
    // Pass through UV
    out.uv = input.uv;
    
    return out;
}


// --- Fragment Shader ---

// Material properties uniform
struct MaterialUniforms {
    base_color: vec4<f32>,      // RGBA base color
    emissive: vec3<f32>,        // RGB emissive color
    specular_power: f32,        // Specular exponent (shininess)
    ambient: vec3<f32>,         // Ambient color
    _padding: f32,              // Alignment padding
};

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;

// --- Light Structures (must match Rust repr(C) layout) ---

struct DirectionalLight {
    direction: vec4<f32>,            // xyz = direction, w = padding
    color: vec4<f32>,                // rgb = color, a = intensity
    shadow_view_proj: mat4x4<f32>,   // Light's view-projection for shadow mapping
    shadow_params: vec4<f32>,        // x = atlas_index (-1 = no shadow), y = bias, z = normal_bias, w = padding
};

struct PointLight {
    position: vec4<f32>,             // xyz = position, w = range
    color: vec4<f32>,                // rgb = color, a = intensity
    shadow_params: vec4<f32>,        // x = atlas_index (-1 = no shadow), y = bias, z = normal_bias, w = padding
};

struct SpotLight {
    position: vec4<f32>,             // xyz = position, w = range
    direction: vec4<f32>,            // xyz = direction, w = inner_cone_cos
    color: vec4<f32>,                // rgb = color, a = intensity
    params: vec4<f32>,               // x = outer_cone_cos, yzw = padding
    shadow_view_proj: mat4x4<f32>,   // Light's view-projection for shadow mapping
    shadow_params: vec4<f32>,        // x = atlas_index (-1 = no shadow), y = bias, z = normal_bias, w = padding
};

// Light arrays with fixed sizes matching LitForwardLane defaults
const MAX_DIRECTIONAL_LIGHTS: u32 = 4u;
const MAX_POINT_LIGHTS: u32 = 16u;
const MAX_SPOT_LIGHTS: u32 = 8u;

struct LightingUniforms {
    directional_lights: array<DirectionalLight, 4>,
    point_lights: array<PointLight, 16>,
    spot_lights: array<SpotLight, 8>,
    num_directional_lights: u32,
    num_point_lights: u32,
    num_spot_lights: u32,
    _padding: u32,
};

@group(3) @binding(0)
var<uniform> lights: LightingUniforms;

// Shadow atlas (2D array depth texture) and comparison sampler
@group(3) @binding(1)
var shadow_atlas: texture_depth_2d_array;

@group(3) @binding(2)
var shadow_sampler: sampler_comparison;

// --- Shadow Sampling ---

/// Samples the shadow atlas with PCF (Percentage Closer Filtering).
/// Returns a shadow factor: 1.0 = fully lit, 0.0 = fully in shadow.
fn sample_shadow_pcf(
    shadow_vp: mat4x4<f32>,
    world_pos: vec3<f32>,
    N: vec3<f32>,
    atlas_index: i32,
    bias: f32,
    normal_bias: f32,
) -> f32 {
    if (atlas_index < 0) {
        return 1.0; // No shadow map for this light
    }

    // Apply normal bias to push the sample point along the surface normal
    let biased_pos = world_pos + N * normal_bias;

    // Project world position into light clip space
    let light_clip = shadow_vp * vec4<f32>(biased_pos, 1.0);
    let light_ndc = light_clip.xyz / light_clip.w;

    // Convert from NDC [-1,1] to UV [0,1] (note: Y is flipped)
    let shadow_uv = vec2<f32>(
        light_ndc.x * 0.5 + 0.5,
        1.0 - (light_ndc.y * 0.5 + 0.5),
    );

    // If outside the shadow map, treat as lit
    if (shadow_uv.x < 0.0 || shadow_uv.x > 1.0 || shadow_uv.y < 0.0 || shadow_uv.y > 1.0) {
        return 1.0;
    }

    // Depth in [0,1] range (RH zero-to-one projection)
    let depth = light_ndc.z - bias;

    // 3x3 PCF kernel for soft shadow edges
    let texel_size = 1.0 / 2048.0; // Atlas resolution
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

// --- Lighting Functions ---

/// Calculates attenuation for point/spot lights based on distance and range
fn calculate_attenuation(distance: f32, range: f32) -> f32 {
    // Smooth attenuation that reaches zero at range
    let normalized_distance = distance / range;
    let attenuation = saturate(1.0 - normalized_distance * normalized_distance);
    return attenuation * attenuation;
}

/// Calculates spotlight cone attenuation
fn calculate_spot_attenuation(
    light_dir: vec3<f32>,
    spot_direction: vec3<f32>,
    inner_cone_cos: f32,
    outer_cone_cos: f32
) -> f32 {
    let cos_angle = dot(-light_dir, spot_direction);
    return smoothstep(outer_cone_cos, inner_cone_cos, cos_angle);
}

/// Blinn-Phong BRDF calculation
fn blinn_phong(
    N: vec3<f32>,           // Surface normal (normalized)
    V: vec3<f32>,           // View direction (normalized)
    L: vec3<f32>,           // Light direction (normalized, pointing toward light)
    light_color: vec3<f32>,
    light_intensity: f32,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    // Diffuse component (Lambertian)
    let NdotL = max(dot(N, L), 0.0);
    let diffuse = diffuse_color * NdotL;
    
    // Specular component (Blinn-Phong)
    let H = normalize(L + V);  // Half vector
    let NdotH = max(dot(N, H), 0.0);
    // Only apply specular if the light is actually hitting the front of the surface
    var specular_strength = 0.0;
    if (NdotL > 0.0) {
        specular_strength = pow(NdotH, specular_power);
    }
    let specular = vec3<f32>(specular_strength);
    
    return (diffuse + specular) * light_color * light_intensity;
}

/// Calculate contribution from all directional lights (with shadows)
fn calculate_directional_lights(
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    
    for (var i = 0u; i < lights.num_directional_lights && i < MAX_DIRECTIONAL_LIGHTS; i++) {
        let light = lights.directional_lights[i];
        let L = -normalize(light.direction.xyz);  // Reverse direction (toward light)
        
        // Shadow factor
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
            specular_power
        ) * shadow;
    }
    
    return result;
}

/// Calculate contribution from all point lights
fn calculate_point_lights(
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    
    for (var i = 0u; i < lights.num_point_lights && i < MAX_POINT_LIGHTS; i++) {
        let light = lights.point_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        
        // Skip if outside range
        if (distance > light.position.w) {
            continue;
        }
        
        let L = normalize(light_vec);
        let attenuation = calculate_attenuation(distance, light.position.w);
        
        result += blinn_phong(
            N, V, L,
            light.color.rgb,
            light.color.a * attenuation,
            diffuse_color,
            specular_power
        );
    }
    
    return result;
}

/// Calculate contribution from all spot lights (with shadows)
fn calculate_spot_lights(
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    
    for (var i = 0u; i < lights.num_spot_lights && i < MAX_SPOT_LIGHTS; i++) {
        let light = lights.spot_lights[i];
        let light_vec = light.position.xyz - world_position;
        let distance = length(light_vec);
        
        // Skip if outside range
        if (distance > light.position.w) {
            continue;
        }
        
        let L = normalize(light_vec);
        let distance_attenuation = calculate_attenuation(distance, light.position.w);
        let spot_attenuation = calculate_spot_attenuation(
            L,
            normalize(light.direction.xyz),
            light.direction.w,
            light.params.x
        );
        let total_attenuation = distance_attenuation * spot_attenuation;
        
        // Skip if outside cone
        if (total_attenuation <= 0.0) {
            continue;
        }

        // Shadow factor
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
            specular_power
        ) * shadow;
    }
    
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Prepare surface data
    let N = normalize(input.normal);
    let V = normalize(camera.camera_position.xyz - input.world_position);
    
    // Calculate base diffuse color (material * vertex color)
    let diffuse_color = material.base_color.rgb;
    
    // Start with ambient lighting
    var final_color = material.ambient * diffuse_color;
    
    // Add contributions from all light types (with shadow mapping)
    final_color += calculate_directional_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    final_color += calculate_point_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    final_color += calculate_spot_lights(input.world_position, N, V, diffuse_color, material.specular_power);
    
    // Add emissive
    final_color += material.emissive;
    
    // Simple tone mapping (Reinhard)
    final_color = final_color / (final_color + vec3<f32>(1.0));
    
    // Gamma correction
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));
    
    return vec4<f32>(final_color, material.base_color.a);
}
