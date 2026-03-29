// ============================================================
// egui overlay shader — renders egui UI primitives
// ============================================================
// Vertex format matches egui::epaint::Vertex:
//   pos:   vec2<f32>  (screen-space pixel coordinates)
//   uv:    vec2<f32>  (texture coordinates)
//   color: vec4<f32>  (sRGB color decoded from Rgba8Unorm)
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
    // The color is already decoded by the Unorm8x4 vertex format.
    out.color = in.color;
    return out;
}

@group(1) @binding(0)
var t_egui: texture_2d<f32>;
@group(1) @binding(1)
var s_egui: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_egui, s_egui, in.uv);
    // Multiply vertex color by texture sample. Both are in linear space
    // after Unorm8x4 + sRGB surface decoding.
    return in.color * tex_color;
}
