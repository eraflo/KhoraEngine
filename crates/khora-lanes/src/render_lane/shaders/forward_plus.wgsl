// Forward+ Fragment Shader
// Tile-based deferred light lookup with Blinn-Phong shading
//
// This shader uses the pre-computed light culling data to only
// iterate over lights that actually affect the current pixel's tile.

// --- Vertex Shader (same as lit_forward.wgsl) ---

struct CameraUniforms {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;

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

// --- Fragment Shader ---

struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive: vec3<f32>,
    specular_power: f32,
    ambient: vec3<f32>,
    _padding: f32,
};

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;

// --- Forward+ Light Data ---

// Unified light structure (matches GpuLight in Rust, 64 bytes)
struct GpuLight {
    position: vec3<f32>,
    range: f32,
    color: vec3<f32>,
    intensity: f32,
    direction: vec3<f32>,
    light_type: u32,      // 0 = directional, 1 = point, 2 = spot
    inner_cone_cos: f32,
    outer_cone_cos: f32,
    _padding: vec2<f32>,
};

// Tile info uniform
struct TileInfo {
    tile_count: vec2<u32>,
    tile_size: u32,
    max_lights_per_tile: u32,
};

@group(3) @binding(0)
var<storage, read> lights: array<GpuLight>;

@group(3) @binding(1)
var<storage, read> light_indices: array<u32>;

@group(3) @binding(2)
var<storage, read> light_grid: array<u32>;  // [offset, count] pairs per tile

@group(3) @binding(3)
var<uniform> tile_info: TileInfo;

// --- Lighting Functions ---

fn calculate_attenuation(distance: f32, range: f32) -> f32 {
    let normalized_distance = distance / range;
    let attenuation = saturate(1.0 - normalized_distance * normalized_distance);
    return attenuation * attenuation;
}

fn calculate_spot_attenuation(
    light_dir: vec3<f32>,
    spot_direction: vec3<f32>,
    inner_cone_cos: f32,
    outer_cone_cos: f32
) -> f32 {
    let cos_angle = dot(-light_dir, spot_direction);
    return smoothstep(outer_cone_cos, inner_cone_cos, cos_angle);
}

fn blinn_phong(
    N: vec3<f32>,
    V: vec3<f32>,
    L: vec3<f32>,
    light_color: vec3<f32>,
    light_intensity: f32,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    let NdotL = max(dot(N, L), 0.0);
    let diffuse = diffuse_color * NdotL;
    
    let H = normalize(L + V);
    let NdotH = max(dot(N, H), 0.0);
    let specular_strength = pow(NdotH, specular_power);
    let specular = vec3<f32>(specular_strength);
    
    return (diffuse + specular) * light_color * light_intensity;
}

// Calculate contribution from a single light
fn calculate_light_contribution(
    light: GpuLight,
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    diffuse_color: vec3<f32>,
    specular_power: f32
) -> vec3<f32> {
    // Directional light
    if (light.light_type == 0u) {
        let L = -normalize(light.direction);
        return blinn_phong(N, V, L, light.color, light.intensity, diffuse_color, specular_power);
    }
    
    // Point/Spot light common setup
    let light_vec = light.position - world_position;
    let distance = length(light_vec);
    
    if (distance > light.range) {
        return vec3<f32>(0.0);
    }
    
    let L = normalize(light_vec);
    var attenuation = calculate_attenuation(distance, light.range);
    
    // Spot light cone attenuation
    if (light.light_type == 2u) {
        let spot_attenuation = calculate_spot_attenuation(
            L,
            normalize(light.direction),
            light.inner_cone_cos,
            light.outer_cone_cos
        );
        attenuation *= spot_attenuation;
    }
    
    if (attenuation <= 0.0) {
        return vec3<f32>(0.0);
    }
    
    return blinn_phong(N, V, L, light.color, light.intensity * attenuation, diffuse_color, specular_power);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Prepare surface data
    let N = normalize(input.normal);
    let V = normalize(camera.camera_position.xyz - input.world_position);
    let diffuse_color = material.base_color.rgb;
    
    // Calculate tile index from clip position
    let tile_x = u32(input.clip_position.x) / tile_info.tile_size;
    let tile_y = u32(input.clip_position.y) / tile_info.tile_size;
    let tile_index = tile_y * tile_info.tile_count.x + tile_x;
    
    // Read light grid for this tile
    let light_offset = light_grid[tile_index * 2u];
    let light_count = light_grid[tile_index * 2u + 1u];
    
    // Start with ambient lighting
    var final_color = material.ambient * diffuse_color;
    
    // Iterate only over lights that affect this tile
    for (var i = 0u; i < light_count; i++) {
        let light_index = light_indices[light_offset + i];
        let light = lights[light_index];
        
        final_color += calculate_light_contribution(
            light,
            input.world_position,
            N, V,
            diffuse_color,
            material.specular_power
        );
    }
    
    // Add emissive
    final_color += material.emissive;
    
    // Reinhard tone mapping
    final_color = final_color / (final_color + vec3<f32>(1.0));
    
    // Gamma correction
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));
    
    return vec4<f32>(final_color, material.base_color.a);
}
