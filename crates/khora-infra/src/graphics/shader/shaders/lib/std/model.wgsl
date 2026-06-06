#define_import_path khora::std::model

// Per-mesh transform uniform — bound at @group(1) @binding(0). The
// normal matrix is provided as a separate `mat4x4<f32>` (with its
// translation column zeroed) so non-uniform scales transform normals
// correctly.

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniforms;
