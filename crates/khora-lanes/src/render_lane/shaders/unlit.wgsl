// Simple Unlit Shader - With camera and model transforms
// Renders geometry with a flat white color (no lighting, no vertex colors)

// --- Camera Uniforms (group 0, binding 0) ---

struct CameraUniforms {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

// --- Model Uniforms (group 1, binding 0) ---

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;

// --- Material Uniforms (group 2, binding 0) ---

// Note: WGSL Uniform struct size MUST exactly match the Rust `MaterialUniforms` struct size
// sent by the renderer, even if we don't use all the fields in the Unlit pipeline.
struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive: vec3<f32>,
    specular_power: f32,
    ambient: vec3<f32>,
    _padding: f32,
};

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;

// Vertex input matching the standard mesh layout: pos + normal + uv
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

// Output from vertex shader to fragment shader
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

// Vertex shader - transforms positions through model and view-projection matrices
@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    output.clip_position = camera.view_projection * world_pos;
    output.normal = normalize((model.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    return output;
}

// Fragment shader - simple shading based on normal direction for visual feedback
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Use normal-based shading so geometry is visible (half-Lambert)
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.3));
    let ndotl = max(dot(input.normal, light_dir), 0.0);
    let ambient = 0.15;
    let brightness = ambient + ndotl * 0.85;
    let base = vec3<f32>(brightness, brightness, brightness);
    return vec4<f32>(base * material.base_color.rgb, material.base_color.a);
}