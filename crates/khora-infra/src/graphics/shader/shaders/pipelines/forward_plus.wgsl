// Forward+ pipeline — tile-based light lookup with the shared Cook-Torrance
// PBR BRDF and full shadow sampling: 2D atlas for directional / spot, cube
// atlas for point lights.
//
// The BRDF (`khora::lighting::pbr`) and the output tone-map are shared with
// `standard_pbr` and `lit_forward`, so the three lit lanes render the same
// image — this lane differs only in iterating just the lights its tile's
// culling list selected.
//
// Bind-group layout — the engine's canonical 4-group render convention
// (see .agent/conventions.md). Group 3 is the *lighting domain*: it
// holds every lighting input — the light list, the per-tile culling
// results, the shadow atlases. Forward+ packs its data around the
// shadow bindings (1/2/3) imposed by `khora::shadow::bindings`:
//   group(3) binding 0 — lights (storage)
//   group(3) binding 1 — shadow atlas 2D       ─┐ khora::shadow::bindings
//   group(3) binding 2 — shadow sampler         ├ (shared with LitForward)
//   group(3) binding 3 — shadow cube atlas     ─┘
//   group(3) binding 4 — light_indices (storage)
//   group(3) binding 5 — light_grid (storage)
//   group(3) binding 6 — tile_info (uniform)
//   group(3) binding 7 — light_shadow_view_projs (storage)
//
// Point-light cube shadows use `sample_point_shadow_params` directly,
// since `GpuLight` is the unified struct (no per-face matrices needed —
// the cube sampler picks the face from the world-space direction in
// hardware, and depth is reconstructed from distance + far plane).

#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::material::material
#import khora::std::material_textures::{sample_albedo, sample_metallic_roughness, sample_emissive, sample_occlusion}
#ifdef HAS_NORMAL_MAP
#import khora::std::material_textures::apply_normal_map
#endif
#import khora::std::vertex::{VertexInput, VertexOutput}
#import khora::lighting::attenuation::{calculate_attenuation, calculate_spot_attenuation}
#import khora::lighting::pbr::{cook_torrance, tonemap_reinhard, hemisphere_ambient}
#import khora::shadow::sample_2d::sample_shadow_pcf
#import khora::shadow::sample_cube::sample_point_shadow_params

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

// --- Forward+ Light Data ---

// Unified light structure (matches GpuLight in Rust, 72 bytes).
struct GpuLight {
    position: vec3<f32>,
    range: f32,
    color: vec3<f32>,
    intensity: f32,
    direction: vec3<f32>,
    light_type: u32,        // 0 = directional, 1 = point, 2 = spot
    inner_cone_cos: f32,
    outer_cone_cos: f32,
    shadow_map_index: i32,  // -1 = no shadow, else atlas layer (2D) or cube index
    shadow_bias: f32,
    shadow_normal_bias: f32,
    shadow_far_plane: f32,  // point-light cube projection far plane (0 for dir/spot)
};

struct TileInfo {
    tile_count: vec2<u32>,
    tile_size: u32,
    max_lights_per_tile: u32,
};

// Group 3 is the lighting domain. Bindings 1/2/3 are the shadow atlases
// (declared by the imported `khora::shadow::bindings`); Forward+ slots
// its own buffers into 0, 4, 5, 6, 7.
@group(3) @binding(0)
var<storage, read> lights: array<GpuLight>;

@group(3) @binding(4)
var<storage, read> light_indices: array<u32>;

@group(3) @binding(5)
var<storage, read> light_grid: array<u32>;

@group(3) @binding(6)
var<uniform> tile_info: TileInfo;

// Parallel array indexed identically to `lights`. Entry `i` is the
// view-projection matrix the shadow lane used for that light's 2D
// atlas slice. Unused / point-light entries hold the identity matrix —
// they are bypassed via the `shadow_map_index < 0` early-out inside
// `sample_shadow_pcf`.
@group(3) @binding(7)
var<storage, read> light_shadow_view_projs: array<mat4x4<f32>>;

fn calculate_light_contribution(
    light: GpuLight,
    world_position: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
) -> vec3<f32> {
    if (light.light_type == 0u) {
        let L = -normalize(light.direction);
        let radiance = light.color * light.intensity;
        return cook_torrance(N, V, L, albedo, metallic, roughness, radiance);
    }

    let light_vec = light.position - world_position;
    let distance = length(light_vec);
    if (distance > light.range) {
        return vec3<f32>(0.0);
    }
    let L = normalize(light_vec);
    var attenuation = calculate_attenuation(distance, light.range);

    if (light.light_type == 2u) {
        let spot_attenuation = calculate_spot_attenuation(
            L,
            normalize(light.direction),
            light.inner_cone_cos,
            light.outer_cone_cos,
        );
        attenuation *= spot_attenuation;
    }

    if (attenuation <= 0.0) {
        return vec3<f32>(0.0);
    }

    let radiance = light.color * light.intensity * attenuation;
    return cook_torrance(N, V, L, albedo, metallic, roughness, radiance);
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
    let geometric_normal = normalize(input.normal);
#ifdef HAS_NORMAL_MAP
    let N = apply_normal_map(geometric_normal, input.world_position, input.uv);
#else
    let N = geometric_normal;
#endif
    let V = normalize(camera.camera_position.xyz - input.world_position);

    let tile_x = u32(input.clip_position.x) / tile_info.tile_size;
    let tile_y = u32(input.clip_position.y) / tile_info.tile_size;
    let tile_index = tile_y * tile_info.tile_count.x + tile_x;

    let light_offset = light_grid[tile_index * 2u];
    let light_count = light_grid[tile_index * 2u + 1u];

    // AO attenuates the indirect (ambient) term only — never direct light.
    let ao = sample_occlusion(input.uv);
    var final_color = hemisphere_ambient(N) * albedo * ao;
    for (var i = 0u; i < light_count; i++) {
        let light_index = light_indices[light_offset + i];
        let light = lights[light_index];

        // Shadow factor — branch on light type:
        //   - directional (0) / spot (2): 2D PCF against `shadow_atlas`
        //     using the per-light view-projection from
        //     `light_shadow_view_projs`.
        //   - point (1): cube PCF against `shadow_cube_atlas`, depth
        //     reconstructed from the light-to-fragment distance.
        var shadow_factor = 1.0;
        if (light.light_type == 1u) {
            shadow_factor = sample_point_shadow_params(
                light.position,
                light.shadow_map_index,
                light.shadow_bias,
                light.shadow_normal_bias,
                light.shadow_far_plane,
                input.world_position,
                N,
            );
        } else {
            shadow_factor = sample_shadow_pcf(
                light_shadow_view_projs[light_index],
                input.world_position,
                N,
                light.shadow_map_index,
                light.shadow_bias,
                light.shadow_normal_bias,
            );
        }

        final_color += calculate_light_contribution(
            light,
            input.world_position,
            N, V,
            albedo,
            metallic,
            roughness,
        ) * shadow_factor;
    }

    final_color += material.emissive * sample_emissive(input.uv);
    final_color = tonemap_reinhard(final_color);

    return vec4<f32>(final_color, out_alpha);
}
