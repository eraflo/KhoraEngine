#define_import_path khora::lighting::pbr

// Shared Cook-Torrance physically-based BRDF and the lit lanes' output
// tone-map. Every lit pipeline (StandardPbr, LitForward, Forward+) imports
// these so the three lanes are performance variants of the SAME image — the
// shading model never changes with the lane GORNA picks.

const PI: f32 = 3.14159265359;

// Schlick approximation of the Fresnel term.
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - cos_theta, 5.0);
}

// GGX / Trowbridge-Reitz normal distribution.
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom_inner = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom_inner * denom_inner);
}

fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_l = max(dot(n, l), 0.0);
    return geometry_schlick_ggx(n_dot_v, roughness)
        * geometry_schlick_ggx(n_dot_l, roughness);
}

// Cook-Torrance BRDF contribution for a single light, multiplied by
// `n_dot_l * radiance`. `albedo` is the base color, `metallic` and
// `roughness` come from the material. `radiance` is `light.color *
// light.intensity * attenuation`.
fn cook_torrance(
    n: vec3<f32>,
    v: vec3<f32>,
    l: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }

    // F0 = 0.04 for dielectrics, lerps to albedo for metals.
    var f0 = vec3<f32>(0.04);
    f0 = mix(f0, albedo, metallic);

    let ndf = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);

    let numerator = ndf * g * f;
    let denominator = 4.0 * max(dot(n, v), 0.0) * n_dot_l + 0.001;
    let specular = numerator / denominator;

    // Energy conservation: diffuse is the complement of the reflected
    // fraction, scaled by `1 - metallic` because metals have no diffuse.
    let k_s = f;
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    return (diffuse + specular) * radiance * n_dot_l;
}

// Reinhard tone-map followed by gamma correction. The single output curve
// shared by every lit lane so their final pixels match.
fn tonemap_reinhard(color: vec3<f32>) -> vec3<f32> {
    let mapped = color / (color + vec3<f32>(1.0));
    return pow(mapped, vec3<f32>(1.0 / 2.2));
}

// Hemispheric ambient: a cheap directional stand-in for indirect sky/ground
// light until full IBL lands. Interpolates a sky and a ground color by the
// surface normal's up component, so upward faces catch cool sky light and
// downward faces catch warmer bounce. The call site multiplies this by albedo
// and the AO factor. These are the scene-default environment colors; a
// configurable / image-based environment replaces them with IBL.
const AMBIENT_SKY: vec3<f32> = vec3<f32>(0.16, 0.18, 0.22);
const AMBIENT_GROUND: vec3<f32> = vec3<f32>(0.06, 0.055, 0.05);
fn hemisphere_ambient(n: vec3<f32>) -> vec3<f32> {
    let t = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(AMBIENT_GROUND, AMBIENT_SKY, t);
}

// The highest mip index of the prefiltered specular cube (PREFILTER_MIPS - 1).
// Roughness maps linearly to this LOD range.
const IBL_MAX_MIP: f32 = 4.0;

// Fresnel-Schlick with a roughness term, used for the ambient (IBL) specular
// so rough surfaces keep a sensible grazing response.
fn fresnel_schlick_roughness(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let inv_rough = vec3<f32>(1.0 - roughness);
    return f0 + (max(inv_rough, f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// Split-sum image-based ambient: diffuse irradiance + prefiltered specular,
// combined with the metallic/Fresnel energy split and attenuated by AO.
// `prefiltered` is the roughness-LOD sample of the specular cube; `brdf` is the
// (scale, bias) from the BRDF LUT.
fn ibl_ambient(
    n: vec3<f32>,
    v: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
    ao: f32,
    irradiance: vec3<f32>,
    prefiltered: vec3<f32>,
    brdf: vec2<f32>,
) -> vec3<f32> {
    let n_dot_v = max(dot(n, v), 0.0);
    var f0 = vec3<f32>(0.04);
    f0 = mix(f0, albedo, metallic);
    let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let k_d = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = irradiance * albedo * k_d;
    let specular = prefiltered * (f * brdf.x + brdf.y);
    return (diffuse + specular) * ao;
}
