// Standard PBR Material Shader
// Implements metallic-roughness workflow for physically-based rendering

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
    metallic: f32,              // Metallic factor [0-1]
    roughness: f32,             // Roughness factor [0-1]
    alpha_cutoff: f32,          // For alpha masking
    _padding: vec2<f32>,        // Alignment padding
};

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;

// Simple directional light for basic lighting
struct DirectionalLight {
    direction: vec3<f32>,
    _padding1: f32,
    color: vec3<f32>,
    intensity: f32,
};

@group(3) @binding(0)
var<uniform> light: DirectionalLight;

// Constants
const PI: f32 = 3.14159265359;

// Simplified PBR calculations
// This is a basic implementation - will be enhanced with full PBR in issue #48

fn fresnel_schlick(cosTheta: f32, F0: vec3<f32>) -> vec3<f32> {
    return F0 + (vec3<f32>(1.0) - F0) * pow(1.0 - cosTheta, 5.0);
}

fn distribution_ggx(N: vec3<f32>, H: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let NdotH = max(dot(N, H), 0.0);
    let NdotH2 = NdotH * NdotH;
    
    let nom = a2;
    var denom = (NdotH2 * (a2 - 1.0) + 1.0);
    denom = PI * denom * denom;
    
    return nom / denom;
}

fn geometry_schlick_ggx(NdotV: f32, roughness: f32) -> f32 {
    let r = (roughness + 1.0);
    let k = (r * r) / 8.0;
    
    let nom = NdotV;
    let denom = NdotV * (1.0 - k) + k;
    
    return nom / denom;
}

fn geometry_smith(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, roughness: f32) -> f32 {
    let NdotV = max(dot(N, V), 0.0);
    let NdotL = max(dot(N, L), 0.0);
    let ggx2 = geometry_schlick_ggx(NdotV, roughness);
    let ggx1 = geometry_schlick_ggx(NdotL, roughness);
    
    return ggx1 * ggx2;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Base color from material and vertex color
    let albedo = material.base_color.rgb * in.color;
    
    // Normal (already in world space from vertex shader)
    let N = normalize(in.normal);
    
    // View direction
    let V = normalize(camera.camera_position.xyz - in.world_position);
    
    // Light direction (negated because we typically define light direction as "to light")
    let L = normalize(-light.direction);
    
    // Half vector
    let H = normalize(V + L);
    
    // Calculate F0 (surface reflection at zero incidence)
    // For dielectrics, F0 is around 0.04, for metals it's the albedo color
    var F0 = vec3<f32>(0.04);
    F0 = mix(F0, albedo, material.metallic);
    
    // Cook-Torrance BRDF
    let NDF = distribution_ggx(N, H, material.roughness);
    let G = geometry_smith(N, V, L, material.roughness);
    let F = fresnel_schlick(max(dot(H, V), 0.0), F0);
    
    let NdotL = max(dot(N, L), 0.0);
    
    // Specular component
    let numerator = NDF * G * F;
    let denominator = 4.0 * max(dot(N, V), 0.0) * NdotL + 0.001; // Add epsilon to prevent division by zero
    let specular = numerator / denominator;
    
    // Energy conservation - diffuse component
    let kS = F;  // Specular reflection
    var kD = vec3<f32>(1.0) - kS;  // Diffuse reflection
    kD *= 1.0 - material.metallic;  // Metals have no diffuse reflection
    
    // Lambert diffuse
    let diffuse = kD * albedo / PI;
    
    // Combine diffuse and specular
    let radiance = light.color * light.intensity;
    let Lo = (diffuse + specular) * radiance * NdotL;
    
    // Simple ambient (will be replaced with proper ambient lighting later)
    let ambient = vec3<f32>(0.03) * albedo;
    
    // Final color
    var color = ambient + Lo + material.emissive;
    
    // Simple tone mapping (Reinhard)
    color = color / (color + vec3<f32>(1.0));
    
    // Gamma correction
    color = pow(color, vec3<f32>(1.0 / 2.2));
    
    return vec4<f32>(color, material.base_color.a);
}
