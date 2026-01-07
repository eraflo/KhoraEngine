// Light Culling Compute Shader for Forward+ Rendering
// 
// This shader performs tile-based light culling by:
// 1. Computing the frustum for each screen tile
// 2. Testing each light against the tile frustum
// 3. Building a per-tile light index list
//
// Workgroup size: 16x16 threads = 256 threads per tile
// Each workgroup processes one screen tile

// --- Uniforms and Buffers ---

struct LightCullingUniforms {
    view_projection: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    screen_dimensions: vec2<f32>,
    tile_count: vec2<u32>,
    num_lights: u32,
    tile_size: u32,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: LightCullingUniforms;

// GPU-friendly light structure (64 bytes)
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

@group(0) @binding(1)
var<storage, read> lights: array<GpuLight>;

// Output: Per-tile light indices
// Format: For tile at (x, y), offset = (y * tiles_x + x) * max_lights_per_tile
@group(0) @binding(2)
var<storage, read_write> light_index_list: array<u32>;

// Output: Per-tile (offset, count) pairs
// Format: light_grid[tile_index * 2] = offset, light_grid[tile_index * 2 + 1] = count
@group(0) @binding(3)
var<storage, read_write> light_grid: array<atomic<u32>>;

// --- Workgroup Shared Memory ---

const TILE_SIZE: u32 = 16u;
const MAX_LIGHTS_PER_TILE: u32 = 128u;

var<workgroup> shared_light_count: atomic<u32>;
var<workgroup> shared_light_indices: array<u32, MAX_LIGHTS_PER_TILE>;
var<workgroup> tile_frustum_planes: array<vec4<f32>, 4>;  // Left, Right, Top, Bottom
var<workgroup> tile_min_z: atomic<u32>;
var<workgroup> tile_max_z: atomic<u32>;

// --- Helper Functions ---

// Convert clip-space NDC to view-space position
fn ndc_to_view(ndc: vec3<f32>) -> vec3<f32> {
    let clip = vec4<f32>(ndc, 1.0);
    let view = uniforms.inverse_projection * clip;
    return view.xyz / view.w;
}

// Create a frustum plane from 3 points (counter-clockwise order)
fn create_plane(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>) -> vec4<f32> {
    let v0 = p1 - p0;
    let v1 = p2 - p0;
    let normal = normalize(cross(v0, v1));
    let d = -dot(normal, p0);
    return vec4<f32>(normal, d);
}

// Test if a sphere intersects a plane (returns positive if in front)
fn sphere_plane_distance(center: vec3<f32>, plane: vec4<f32>) -> f32 {
    return dot(center, plane.xyz) + plane.w;
}

// Test if a light sphere intersects the tile frustum
fn light_intersects_tile(light_pos_view: vec3<f32>, light_range: f32) -> bool {
    // Check against all 4 frustum planes
    for (var i = 0u; i < 4u; i++) {
        let distance = sphere_plane_distance(light_pos_view, tile_frustum_planes[i]);
        if (distance < -light_range) {
            return false;  // Completely outside this plane
        }
    }
    
    // Check against depth range (z is negative in view space)
    let min_z_f = bitcast<f32>(atomicLoad(&tile_min_z));
    let max_z_f = bitcast<f32>(atomicLoad(&tile_max_z));
    
    // Light is too far (z more negative than max depth)
    if (light_pos_view.z + light_range < -max_z_f) {
        return false;
    }
    // Light is too close (z less negative than min depth)
    if (light_pos_view.z - light_range > -min_z_f) {
        return false;
    }
    
    return true;
}

// --- Main Compute Shader ---

@compute @workgroup_size(16, 16, 1)
fn cs_main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let tile_x = workgroup_id.x;
    let tile_y = workgroup_id.y;
    let tile_index = tile_y * uniforms.tile_count.x + tile_x;
    
    // Initialize shared memory (first thread only)
    if (local_index == 0u) {
        atomicStore(&shared_light_count, 0u);
        atomicStore(&tile_min_z, bitcast<u32>(1.0f32));  // Near = 1.0
        atomicStore(&tile_max_z, bitcast<u32>(0.0f32));  // Far = 0.0
    }
    
    // Construct tile frustum planes (first 4 threads)
    if (local_index < 4u) {
        // Calculate tile corners in NDC (-1 to 1)
        let tile_min_ndc = vec2<f32>(
            f32(tile_x * TILE_SIZE) / uniforms.screen_dimensions.x * 2.0 - 1.0,
            f32(tile_y * TILE_SIZE) / uniforms.screen_dimensions.y * 2.0 - 1.0
        );
        let tile_max_ndc = vec2<f32>(
            f32((tile_x + 1u) * TILE_SIZE) / uniforms.screen_dimensions.x * 2.0 - 1.0,
            f32((tile_y + 1u) * TILE_SIZE) / uniforms.screen_dimensions.y * 2.0 - 1.0
        );
        
        // Convert corners to view space (at near plane z = 1.0 in NDC)
        let tl = ndc_to_view(vec3<f32>(tile_min_ndc.x, tile_max_ndc.y, 1.0));
        let tr = ndc_to_view(vec3<f32>(tile_max_ndc.x, tile_max_ndc.y, 1.0));
        let bl = ndc_to_view(vec3<f32>(tile_min_ndc.x, tile_min_ndc.y, 1.0));
        let br = ndc_to_view(vec3<f32>(tile_max_ndc.x, tile_min_ndc.y, 1.0));
        let origin = vec3<f32>(0.0, 0.0, 0.0);
        
        // Create frustum planes (normals pointing inward)
        switch (local_index) {
            case 0u: { tile_frustum_planes[0] = create_plane(origin, tl, bl); }  // Left
            case 1u: { tile_frustum_planes[1] = create_plane(origin, br, tr); }  // Right
            case 2u: { tile_frustum_planes[2] = create_plane(origin, tr, tl); }  // Top
            case 3u: { tile_frustum_planes[3] = create_plane(origin, bl, br); }  // Bottom
            default: {}
        }
    }
    
    workgroupBarrier();
    
    // --- Light Culling ---
    // Each thread processes multiple lights in a strided pattern
    let lights_per_thread = (uniforms.num_lights + 255u) / 256u;
    
    for (var i = 0u; i < lights_per_thread; i++) {
        let light_index = local_index + i * 256u;
        
        if (light_index >= uniforms.num_lights) {
            break;
        }
        
        let light = lights[light_index];
        
        // Transform light position to view space
        let light_world_pos = vec4<f32>(light.position, 1.0);
        let light_clip_pos = uniforms.view_projection * light_world_pos;
        let light_view_pos = ndc_to_view(light_clip_pos.xyz / light_clip_pos.w);
        
        // Directional lights always affect all tiles
        var affects_tile = light.light_type == 0u;
        
        // Point and spot lights need frustum test
        if (light.light_type != 0u) {
            affects_tile = light_intersects_tile(light_view_pos, light.range);
        }
        
        if (affects_tile) {
            let slot = atomicAdd(&shared_light_count, 1u);
            if (slot < MAX_LIGHTS_PER_TILE) {
                shared_light_indices[slot] = light_index;
            }
        }
    }
    
    workgroupBarrier();
    
    // --- Write Results ---
    // First thread writes the light grid entry
    if (local_index == 0u) {
        let count = min(atomicLoad(&shared_light_count), MAX_LIGHTS_PER_TILE);
        let offset = tile_index * MAX_LIGHTS_PER_TILE;
        
        // Store offset and count in light_grid
        atomicStore(&light_grid[tile_index * 2u], offset);
        atomicStore(&light_grid[tile_index * 2u + 1u], count);
    }
    
    workgroupBarrier();
    
    // All threads write their portion of light indices
    let count = min(atomicLoad(&shared_light_count), MAX_LIGHTS_PER_TILE);
    let base_offset = tile_index * MAX_LIGHTS_PER_TILE;
    
    for (var i = local_index; i < count; i += 256u) {
        light_index_list[base_offset + i] = shared_light_indices[i];
    }
}
