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

//! Lighting uniform structures.

use crate::math::LinearRgba;

/// Data for a single directional light, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DirectionalLightUniform {
    /// Direction vector (xyz), with padding (w).
    pub direction: [f32; 4], // w is padding
    /// Color (rgb) and Intensity (a).
    pub color: LinearRgba,
    /// View-projection matrix for the shadow map.
    pub shadow_view_proj: [[f32; 4]; 4],
    /// Shadow parameters: x = shadow_map_index, y = shadow_bias, z = shadow_normal_bias, w = padding.
    pub shadow_params: [f32; 4],
}

/// Data for a single point light, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLightUniform {
    /// Position (xyz) and Range (w).
    pub position: [f32; 4], // w is range
    /// Color (rgb) and Intensity (a).
    pub color: LinearRgba,
    /// Shadow parameters: x = shadow_map_index (-1 if no shadow), y = shadow_bias, z = shadow_normal_bias, w = padding.
    pub shadow_params: [f32; 4],
}

/// Data for a single spot light, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotLightUniform {
    /// Position (xyz) and Range (w).
    pub position: [f32; 4], // w is range
    /// Direction (xyz) and Inner Cone Cosine (w).
    pub direction: [f32; 4], // w is inner_cone_cos
    /// Color (rgb) and Intensity (a).
    pub color: LinearRgba,
    /// Outer Cone Cosine (x) and Padding (yzw).
    pub params: [f32; 4], // x = outer_cone_cos, yzw = padding
    /// View-projection matrix for the shadow map.
    pub shadow_view_proj: [[f32; 4]; 4],
    /// Shadow parameters: x = shadow_map_index, y = shadow_bias, z = shadow_normal_bias, w = padding.
    pub shadow_params: [f32; 4],
}

/// Constants for maximum light counts.
pub const MAX_DIRECTIONAL_LIGHTS: usize = 4;
/// Maximum number of point lights supported in the global lighting buffer.
pub const MAX_POINT_LIGHTS: usize = 16;
/// Maximum number of spot lights supported in the global lighting buffer.
pub const MAX_SPOT_LIGHTS: usize = 8;

/// The structure of the global lighting uniform buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingUniforms {
    /// Array of directional light uniforms.
    pub directional_lights: [DirectionalLightUniform; MAX_DIRECTIONAL_LIGHTS],
    /// Array of point light uniforms.
    pub point_lights: [PointLightUniform; MAX_POINT_LIGHTS],
    /// Array of spot light uniforms.
    pub spot_lights: [SpotLightUniform; MAX_SPOT_LIGHTS],
    /// Number of active directional lights.
    pub num_directional_lights: u32,
    /// Number of active point lights.
    pub num_point_lights: u32,
    /// Number of active spot lights.
    pub num_spot_lights: u32,
    /// Padding for 16-byte alignment.
    pub _padding: u32,
}

/// Data for the light culling compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CullingUniformsData {
    /// The combined view-projection matrix.
    pub view_projection: [[f32; 4]; 4],
    /// The inverse of the view-projection matrix.
    pub inverse_projection: [[f32; 4]; 4],
    /// The dimensions of the screen in pixels.
    pub screen_dimensions: [f32; 2],
    /// The number of tiles in the X and Y dimensions.
    pub tile_count: [u32; 2],
    /// The total number of lights in the scene.
    pub num_lights: u32,
    /// The size of a tile in pixels.
    pub tile_size: u32,
    /// Padding for 16-byte alignment.
    pub _padding: [f32; 2],
}
