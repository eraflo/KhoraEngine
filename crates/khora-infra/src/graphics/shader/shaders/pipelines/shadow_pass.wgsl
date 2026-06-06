// Shadow pass pipeline — depth-only rendering, no fragment shader.
// Used by every shadow strategy (Standard/LowRes); the strategy picks
// which atlas layer/view to render into via the render-pass descriptor.

#import khora::std::camera::camera
#import khora::std::model::model

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_projection * model.model_matrix * vec4<f32>(input.position, 1.0);
    return out;
}

// No fragment shader — depth-only pass writes the auto-generated depth.
