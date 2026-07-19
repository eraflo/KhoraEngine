#define_import_path khora::std::material_textures

// PBR material textures — group 2, bindings 1..6. Mirrors the Rust
// contract in `khora_core::renderer::api::material::bindings`. The
// uniform at binding 0 lives in `khora::std::material`.
//
// Each optional texture binding is gated by the matching `HAS_*` shader
// variant flag, set by the material projection for exactly the maps a
// material declares. The bind-group layout
// (`material_layout_entries_for_variant`) and the cached bind group are
// built from the same flag set, so the WGSL declarations here line up
// with the bound resources byte-for-byte — no fallback textures.
//
// When a map is absent the corresponding sampler helper returns the
// multiplicative identity, so the fragment shader's `factor * sampled`
// math reduces to the scalar/uniform factor alone.

#ifdef HAS_BASE_COLOR_TEXTURE
@group(2) @binding(1) var base_color_tex: texture_2d<f32>;
#endif
#ifdef HAS_METALLIC_ROUGHNESS_TEXTURE
@group(2) @binding(2) var metallic_roughness_tex: texture_2d<f32>;
#endif
#ifdef HAS_NORMAL_MAP
@group(2) @binding(3) var normal_tex: texture_2d<f32>;
#endif
#ifdef HAS_EMISSIVE_TEXTURE
@group(2) @binding(4) var emissive_tex: texture_2d<f32>;
#endif
#ifdef HAS_OCCLUSION_MAP
@group(2) @binding(5) var occlusion_tex: texture_2d<f32>;
#endif
@group(2) @binding(6) var material_sampler: sampler;

// Albedo (sRGB texture decoded to linear by the GPU on sample). Without a
// base-color map, returns white so the base-color factor passes through.
fn sample_albedo(uv: vec2<f32>) -> vec4<f32> {
#ifdef HAS_BASE_COLOR_TEXTURE
    return textureSample(base_color_tex, material_sampler, uv);
#else
    return vec4<f32>(1.0, 1.0, 1.0, 1.0);
#endif
}

// glTF metallic-roughness packing: B = metallic, G = roughness. Without a
// map, returns (1,1) so the scalar metallic/roughness factors pass through.
fn sample_metallic_roughness(uv: vec2<f32>) -> vec2<f32> {
#ifdef HAS_METALLIC_ROUGHNESS_TEXTURE
    let mr = textureSample(metallic_roughness_tex, material_sampler, uv);
    return vec2<f32>(mr.b, mr.g);
#else
    return vec2<f32>(1.0, 1.0);
#endif
}

// Without an emissive map, returns white so the emissive factor passes through.
fn sample_emissive(uv: vec2<f32>) -> vec3<f32> {
#ifdef HAS_EMISSIVE_TEXTURE
    return textureSample(emissive_tex, material_sampler, uv).rgb;
#else
    return vec3<f32>(1.0, 1.0, 1.0);
#endif
}

// Ambient-occlusion factor (red channel). Multiplies the indirect (ambient/
// IBL) term only. Without an AO map, returns 1.0 so the indirect term is
// unattenuated.
fn sample_occlusion(uv: vec2<f32>) -> f32 {
#ifdef HAS_OCCLUSION_MAP
    return textureSample(occlusion_tex, material_sampler, uv).r;
#else
    return 1.0;
#endif
}

#ifdef HAS_NORMAL_MAP
// Perturbs the geometric normal with the tangent-space normal map,
// building the TBN basis from screen-space derivatives (no per-vertex
// tangent attribute required). Christian Schüler's cotangent frame.
// Only compiled for materials that declare a normal map; without one the
// fragment shader uses the interpolated geometric normal directly.
fn apply_normal_map(
    geometric_normal: vec3<f32>,
    world_pos: vec3<f32>,
    uv: vec2<f32>,
) -> vec3<f32> {
    let tangent_normal = textureSample(normal_tex, material_sampler, uv).xyz * 2.0 - 1.0;

    let dp1 = dpdx(world_pos);
    let dp2 = dpdy(world_pos);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);

    let dp2perp = cross(dp2, geometric_normal);
    let dp1perp = cross(geometric_normal, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;

    let inv_max = inverseSqrt(max(dot(t, t), dot(b, b)));
    let tbn = mat3x3<f32>(t * inv_max, b * inv_max, geometric_normal);
    return normalize(tbn * tangent_normal);
}
#endif
