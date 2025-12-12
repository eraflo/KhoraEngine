// Emissive Material Shader
// For self-illuminating surfaces with HDR intensity support

// --- Vertex Shader ---

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
    @location(0) uv: vec2<f32>,
};

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    out.clip_position = camera.view_projection * world_pos;
    out.uv = input.uv;
    
    return out;
}


// --- Fragment Shader ---

struct EmissiveMaterialUniforms {
    emissive_color: vec4<f32>,  // RGB + padding
    intensity: f32,             // HDR intensity multiplier
    _padding: vec3<f32>,
};

@group(2) @binding(0)
var<uniform> material: EmissiveMaterialUniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Emissive materials simply output their color multiplied by intensity
    // No lighting calculations needed - they ARE the light source
    
    let emissive = material.emissive_color.rgb * material.intensity;
    
    // Simple tone mapping for HDR values
    let tone_mapped = emissive / (emissive + vec3<f32>(1.0));
    
    // Gamma correction
    let gamma_corrected = pow(tone_mapped, vec3<f32>(1.0 / 2.2));
    
    return vec4<f32>(gamma_corrected, material.emissive_color.a);
}
