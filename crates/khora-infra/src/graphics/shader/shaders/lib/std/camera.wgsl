#define_import_path khora::std::camera

// Camera uniform block — bound at @group(0) @binding(0) by every
// pipeline that renders from a primary view. The view-projection
// matrix is `proj * view`; `camera_position` is the world-space camera
// origin (w padding to keep std140 alignment).

struct CameraUniforms {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;
