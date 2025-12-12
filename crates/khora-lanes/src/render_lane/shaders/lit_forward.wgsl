// Lit Forward Shader
// Multi-light forward rendering with Blinn-Phong lighting
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
    @location(3) color: vec3<f32>,
};

// Output from vertex shader to fragment shader
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
};

// Model transform uniform
struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,  // For transforming normals
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
    
    // Pass through UV and color
    out.uv = input.uv;
    out.color = input.color;
    
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

// --- Light Structures ---

struct DirectionalLight {
    direction: vec3<f32>,
    _padding1: f32,
    color: vec3<f32>,
    intensity: f32,
};

struct PointLight {
    position: vec3<f32>,
    range: f32,
    color: vec3<f32>,
    intensity: f32,
};

struct SpotLight {
    position: vec3<f32>,
    range: f32,
    direction: vec3<f32>,
    inner_cone_cos: f32,
    color: vec3<f32>,
    outer_cone_cos: f32,
    intensity: f32,
    _padding: vec3<f32>,
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
    let specular_strength = pow(NdotH, specular_power);
    let specular = vec3<f32>(specular_strength);
    
    return (diffuse + specular) * light_color * light_intensity;
}

/// Calculate contribution from all directional lights
fn calculate_directional_lights(
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    
    for (var i = 0u; i < lights.num_directional_lights && i < MAX_DIRECTIONAL_LIGHTS; i++) {
        let light = lights.directional_lights[i];
        let L = -normalize(light.direction);  // Reverse direction (toward light)
        
        result += blinn_phong(
            N, V, L,
            light.color,
            light.intensity,
            diffuse_color,
            specular_power
        );
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
        let light_vec = light.position - world_position;
        let distance = length(light_vec);
        
        // Skip if outside range
        if (distance > light.range) {
            continue;
        }
        
        let L = normalize(light_vec);
        let attenuation = calculate_attenuation(distance, light.range);
        
        result += blinn_phong(
            N, V, L,
            light.color,
            light.intensity * attenuation,
            diffuse_color,
            specular_power
        );
    }
    
    return result;
}

/// Calculate contribution from all spot lights
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
        let light_vec = light.position - world_position;
        let distance = length(light_vec);
        
        // Skip if outside range
        if (distance > light.range) {
            continue;
        }
        
        let L = normalize(light_vec);
        let distance_attenuation = calculate_attenuation(distance, light.range);
        let spot_attenuation = calculate_spot_attenuation(
            L,
            normalize(light.direction),
            light.inner_cone_cos,
            light.outer_cone_cos
        );
        let total_attenuation = distance_attenuation * spot_attenuation;
        
        // Skip if outside cone
        if (total_attenuation <= 0.0) {
            continue;
        }
        
        result += blinn_phong(
            N, V, L,
            light.color,
            light.intensity * total_attenuation,
            diffuse_color,
            specular_power
        );
    }
    
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Prepare surface data
    let N = normalize(input.normal);
    let V = normalize(camera.camera_position.xyz - input.world_position);
    
    // Calculate base diffuse color (material * vertex color)
    let diffuse_color = material.base_color.rgb * input.color;
    
    // Start with ambient lighting
    var final_color = material.ambient * diffuse_color;
    
    // Add contributions from all light types
    final_color += calculate_directional_lights(N, V, diffuse_color, material.specular_power);
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
