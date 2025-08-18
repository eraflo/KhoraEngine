// --- Vertex Shader ---

// This structure describes the data we read from the Vertex Buffer.
// The `@location(n)` must correspond to the `shader_location` defined
// in the `VertexBufferLayoutDescriptor` in Rust.
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

// This structure describes the data that the vertex shader sends to the fragment shader.
// The GPU will interpolate these values for each pixel of the triangle.
struct VertexOutput {
    // `@builtin(position)` is a special variable. It's the vertex position
    // in "clip space" that the GPU will use for rasterization.
    @builtin(position) clip_position: vec4<f32>,

    // We pass the color to the fragment shader via `@location(0)`.
    @location(0) color: vec3<f32>,
};

// `@vertex` indicates that this is the main function of the vertex shader.
// The name `vs_main` must correspond to the `vertex_entry_point` in Rust.
@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // We convert the 3D position to a 4D position for clip space.
    // The `1.0` for the 'w' coordinate is standard.
    out.clip_position = vec4<f32>(model.position, 1.0);
    // We simply pass through the color.
    out.color = model.color;
    return out;
}


// --- Fragment Shader ---

// `@fragment` indicates that this is the main function of the fragment shader.
// The name `fs_main` must correspond to the `fragment_entry_point` in Rust.
// It takes as input the `VertexOutput` structure (interpolated) and returns
// the final color of the pixel at location `@location(0)`.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // We take the interpolated color (a vec3) and add an alpha component
    // of 1.0 (fully opaque) to create the final color (a vec4).
    return vec4<f32>(in.color, 1.0);
}