// ============================================================
// egui overlay shader — renders egui UI primitives
// ============================================================
// Vertex format matches egui::epaint::Vertex:
//   pos:   vec2<f32>  (screen-space pixel coordinates)
//   uv:    vec2<f32>  (texture coordinates)
//   color: vec4<f32>  (sRGB-normalized [0,1] via Unorm8x4 — NOT linear)
//
// Color space contract:
//   - Vertex colors come from egui Color32 (sRGB bytes) normalized by
//     Unorm8x4 to [0,1]. They must be decoded sRGB→linear before use.
//   - Textures use Rgba8UnormSrgb; the GPU auto-decodes them to linear.
//   - The render target is Bgra8UnormSrgb; the GPU auto-encodes linear→sRGB.
//   - All arithmetic in the fragment shader must be in linear space.
// ============================================================

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> screen_size: vec2<f32>;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Convert pixel coordinates to NDC: x ∈ [-1, 1], y ∈ [-1, 1]
    // Screen origin is top-left, NDC origin is center.
    out.clip_position = vec4<f32>(
        2.0 * in.position.x / screen_size.x - 1.0,
        1.0 - 2.0 * in.position.y / screen_size.y,
        0.0,
        1.0,
    );
    out.uv = in.uv;
    // Pass sRGB-normalized color to the fragment stage for linear decode.
    out.color = in.color;
    return out;
}

@group(1) @binding(0)
var t_egui: texture_2d<f32>;
@group(1) @binding(1)
var s_egui: sampler;

// sRGB → linear conversion per IEC 61966-2-1.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

// Vertex colors from egui's tessellator are **premultiplied sRGB**
// (RGB bytes have already been multiplied by alpha/255 before normalization).
// To decode correctly:
//   1. Unpremultiply: recover straight sRGB channels (rgb / a).
//   2. Apply sRGB → linear per-channel.
//   3. Re-premultiply: multiply linear RGB by alpha.
// This preserves correct blending for fully-opaque pixels and for the
// anti-aliased feathering edge pixels (alpha < 1) that dominate small shapes.
fn linear_from_premult_srgb(c: vec4<f32>) -> vec4<f32> {
    let a = c.a;
    if a < 0.0001 { return vec4<f32>(0.0); }
    let srgb = c.rgb / a;
    let lin  = vec3<f32>(srgb_to_linear(srgb.r), srgb_to_linear(srgb.g), srgb_to_linear(srgb.b));
    return vec4<f32>(lin * a, a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // tex_color is already linear (Rgba8UnormSrgb auto-decoded by the GPU).
    let tex_color = textureSample(t_egui, s_egui, in.uv);
    // Decode premultiplied sRGB vertex color to premultiplied linear.
    let linear = linear_from_premult_srgb(in.color);
    // Both operands are now in linear premultiplied space; the sRGB surface encodes the result.
    return linear * tex_color;
}
