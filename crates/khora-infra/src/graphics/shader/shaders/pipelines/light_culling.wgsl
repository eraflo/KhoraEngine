// Light Culling Compute Shader for Forward+ Rendering
//
// One workgroup per screen tile. Each tile is the part of the camera's view
// volume that projects into its pixels: four side planes through the tile's
// edges, closed by the camera's near and far planes. Every light whose sphere
// of influence reaches inside all six planes is listed for the tile; the
// Forward+ fragment shader then shades a pixel with its tile's list only.
//
// The planes are extracted from the view-projection matrix (Gribb & Hartmann):
// with `clip = M * p`, a world point is right of the NDC line `x = x0` iff
// `dot(row0 - x0 * row3, p) >= 0`. They are therefore world-space planes, exact
// for perspective and orthographic cameras alike, and the lights are tested
// at their world positions.
//
// Tiles follow the framebuffer convention the fragment shader reads them
// with: tile row 0 is the TOP of the image (pixel y grows downward, NDC y
// grows upward).
//
// Workgroup size: 16x16 = 256 threads sharing the tile's light loop; the
// tile's size in pixels comes from the uniforms and is independent of it.

// --- Uniforms and Buffers ---

struct LightCullingUniforms {
    view_projection: mat4x4<f32>,
    screen_dimensions: vec2<f32>,
    tile_count: vec2<u32>,
    num_lights: u32,
    tile_size: u32,
    max_lights_per_tile: u32,
    _padding: u32,
};

@group(0) @binding(0)
var<uniform> uniforms: LightCullingUniforms;

// The unified light structure — the layout of `GpuLight` in Rust and in
// `forward_plus.wgsl`. Culling reads the position, range and type only.
struct GpuLight {
    position: vec3<f32>,
    range: f32,
    color: vec3<f32>,
    intensity: f32,
    direction: vec3<f32>,
    light_type: u32,      // 0 = directional, 1 = point, 2 = spot
    inner_cone_cos: f32,
    outer_cone_cos: f32,
    shadow_map_index: i32,
    shadow_bias: f32,
    shadow_normal_bias: f32,
    shadow_far_plane: f32,
};

@group(0) @binding(1)
var<storage, read> lights: array<GpuLight>;

// Output: per-tile light indices.
// Tile `t` owns slots `[t * MAX_LIGHTS_PER_TILE, (t + 1) * MAX_LIGHTS_PER_TILE)`.
@group(0) @binding(2)
var<storage, read_write> light_index_list: array<u32>;

// Output: per-tile (offset, count) pairs.
// light_grid[t * 2] = offset, light_grid[t * 2 + 1] = count
@group(0) @binding(3)
var<storage, read_write> light_grid: array<atomic<u32>>;

// --- Workgroup Shared Memory ---

// Slots per tile in `light_index_list`: the most lights a tile can list.
// Mirrored in Rust as `CULLING_SHADER_LIGHTS_PER_TILE`.
const MAX_LIGHTS_PER_TILE: u32 = 128u;

var<workgroup> shared_light_count: atomic<u32>;
var<workgroup> shared_light_indices: array<u32, 128u>;
// Left, right, bottom, top, near, far — normalised, normals pointing inward.
var<workgroup> tile_planes: array<vec4<f32>, 6>;

// --- Helper Functions ---

// Row `i` of a column-major matrix.
fn matrix_row(m: mat4x4<f32>, i: u32) -> vec4<f32> {
    return vec4<f32>(m[0][i], m[1][i], m[2][i], m[3][i]);
}

// Scales a plane so its normal is a unit vector, making `dot(n, p) + d` a
// signed distance. A degenerate plane (no normal) is returned as one every
// point is inside of, so it never culls and never yields NaN.
fn normalize_plane(plane: vec4<f32>) -> vec4<f32> {
    let len = length(plane.xyz);
    if (len < 1e-12) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return plane / len;
}

// The six planes of tile (tile_x, tile_y), in world space.
fn tile_plane(index: u32, tile_x: u32, tile_y: u32) -> vec4<f32> {
    let size = f32(uniforms.tile_size);
    let dims = uniforms.screen_dimensions;
    // Pixel edges of the tile, then NDC: x right, y up (row 0 is the top).
    let x0 = f32(tile_x) * size / dims.x * 2.0 - 1.0;
    let x1 = f32(tile_x + 1u) * size / dims.x * 2.0 - 1.0;
    let y_top = 1.0 - f32(tile_y) * size / dims.y * 2.0;
    let y_bottom = 1.0 - f32(tile_y + 1u) * size / dims.y * 2.0;

    let m = uniforms.view_projection;
    let r0 = matrix_row(m, 0u);
    let r1 = matrix_row(m, 1u);
    let r2 = matrix_row(m, 2u);
    let r3 = matrix_row(m, 3u);

    var plane: vec4<f32>;
    switch (index) {
        case 0u: { plane = r0 - x0 * r3; }        // left:   x >= x0 * w
        case 1u: { plane = x1 * r3 - r0; }        // right:  x <= x1 * w
        case 2u: { plane = r1 - y_bottom * r3; }  // bottom: y >= y_bottom * w
        case 3u: { plane = y_top * r3 - r1; }     // top:    y <= y_top * w
        case 4u: { plane = r2; }                  // near:   z >= 0 (depth 0..1)
        default: { plane = r3 - r2; }             // far:    z <= w
    }
    return normalize_plane(plane);
}

// Whether a sphere reaches inside every plane of the tile.
fn sphere_in_tile(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let plane = tile_planes[i];
        if (dot(plane.xyz, center) + plane.w < -radius) {
            return false;
        }
    }
    return true;
}

// --- Main Compute Shader ---

@compute @workgroup_size(16, 16, 1)
fn cs_main(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let tile_x = workgroup_id.x;
    let tile_y = workgroup_id.y;
    let tile_index = tile_y * uniforms.tile_count.x + tile_x;

    if (local_index == 0u) {
        atomicStore(&shared_light_count, 0u);
    }
    if (local_index < 6u) {
        tile_planes[local_index] = tile_plane(local_index, tile_x, tile_y);
    }

    workgroupBarrier();

    // The configured budget, never past the slots a tile owns.
    let cap = min(uniforms.max_lights_per_tile, MAX_LIGHTS_PER_TILE);

    // --- Light Culling ---
    // Each thread tests every 256th light — of those the buffer really holds:
    // past its end a read is zeros, which would pass for a directional light.
    let num_lights = min(uniforms.num_lights, arrayLength(&lights));
    for (var light_index = local_index; light_index < num_lights; light_index += 256u) {
        let light = lights[light_index];

        // Directional lights reach everywhere; point and spot lights are
        // bounded by their range (a spot's cone lies inside that sphere).
        var affects_tile = light.light_type == 0u;
        if (!affects_tile) {
            affects_tile = sphere_in_tile(light.position, light.range);
        }

        if (affects_tile) {
            let slot = atomicAdd(&shared_light_count, 1u);
            if (slot < cap) {
                shared_light_indices[slot] = light_index;
            }
        }
    }

    workgroupBarrier();

    // --- Write Results ---
    let count = min(atomicLoad(&shared_light_count), cap);
    let base_offset = tile_index * MAX_LIGHTS_PER_TILE;

    if (local_index == 0u) {
        atomicStore(&light_grid[tile_index * 2u], base_offset);
        atomicStore(&light_grid[tile_index * 2u + 1u], count);
    }

    for (var i = local_index; i < count; i += 256u) {
        light_index_list[base_offset + i] = shared_light_indices[i];
    }
}
