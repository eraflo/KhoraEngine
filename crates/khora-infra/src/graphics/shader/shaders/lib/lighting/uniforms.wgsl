#define_import_path khora::lighting::uniforms

#import khora::lighting::structs::{DirectionalLight, PointLight, SpotLight}

// Aggregate lighting uniform bound at @group(3) @binding(0). Its array sizes
// are `ShaderDefs`' `MAX_*_LIGHTS`, injected as `#define`s by the
// `PipelineSystem` at compose time.

struct LightingUniforms {
    directional_lights: array<DirectionalLight, #{MAX_DIRECTIONAL_LIGHTS}>,
    point_lights: array<PointLight, #{MAX_POINT_LIGHTS}>,
    spot_lights: array<SpotLight, #{MAX_SPOT_LIGHTS}>,
    num_directional_lights: u32,
    num_point_lights: u32,
    num_spot_lights: u32,
    _padding: u32,
};

@group(3) @binding(0)
var<uniform> lights: LightingUniforms;
