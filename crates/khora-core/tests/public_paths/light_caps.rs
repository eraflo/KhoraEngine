// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The light caps the lighting uniforms are sized by are the values the
//! shaders receive as defines.

use khora_core::renderer::api::shader::defs::ShaderDefs;
use khora_core::renderer::light::{
    LightingUniforms, MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
};

#[test]
fn light_caps_are_the_shader_defs_values() {
    assert_eq!(
        MAX_DIRECTIONAL_LIGHTS,
        ShaderDefs::MAX_DIRECTIONAL_LIGHTS as usize
    );
    assert_eq!(MAX_POINT_LIGHTS, ShaderDefs::MAX_POINT_LIGHTS as usize);
    assert_eq!(MAX_SPOT_LIGHTS, ShaderDefs::MAX_SPOT_LIGHTS as usize);
}

/// The arrays the shaders index up to `MAX_*_LIGHTS` hold that many lights.
#[test]
fn lighting_uniform_arrays_hold_the_shader_defs_counts() {
    let uniforms: LightingUniforms = bytemuck::Zeroable::zeroed();
    assert_eq!(
        uniforms.directional_lights.len(),
        ShaderDefs::MAX_DIRECTIONAL_LIGHTS as usize
    );
    assert_eq!(
        uniforms.point_lights.len(),
        ShaderDefs::MAX_POINT_LIGHTS as usize
    );
    assert_eq!(
        uniforms.spot_lights.len(),
        ShaderDefs::MAX_SPOT_LIGHTS as usize
    );
}
