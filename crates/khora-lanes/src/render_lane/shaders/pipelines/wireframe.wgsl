// Wireframe Material Shader
// For debug visualization of mesh geometry

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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) barycentric: vec3<f32>,  // For wireframe rendering
};

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    tint_color: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;

@vertex
fn vs_main(input: VertexInput, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    out.clip_position = camera.view_projection * world_pos;
    
    // Assign barycentric coordinates based on vertex index in triangle
    // This is a simple approach - each vertex gets one coordinate as 1.0
    let tri_index = vertex_index % 3u;
    if (tri_index == 0u) {
        out.barycentric = vec3<f32>(1.0, 0.0, 0.0);
    } else if (tri_index == 1u) {
        out.barycentric = vec3<f32>(0.0, 1.0, 0.0);
    } else {
        out.barycentric = vec3<f32>(0.0, 0.0, 1.0);
    }
    
    return out;
}


// --- Fragment Shader ---

struct WireframeMaterialUniforms {
    color: vec4<f32>,           // Wireframe line color
    line_width: f32,            // Line width in pixels
    _padding: vec3<f32>,
};

@group(2) @binding(0)
var<uniform> material: WireframeMaterialUniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Use barycentric coordinates to determine distance from edges
    // The minimum barycentric coordinate tells us how far we are from an edge
    let min_bary = min(min(in.barycentric.x, in.barycentric.y), in.barycentric.z);
    
    // Convert to screen space distance (approximation)
    let edge_distance = min_bary;
    
    // Threshold for line width (this is a simplified approach)
    let threshold = material.line_width * 0.01;  // Scale down for barycentric space
    
    // Discard fragments far from edges
    if (edge_distance > threshold) {
        discard;
    }
    
    // Anti-aliasing: smooth the edge
    let alpha = 1.0 - smoothstep(threshold * 0.5, threshold, edge_distance);
    
    return vec4<f32>(material.color.rgb, material.color.a * alpha);
}
