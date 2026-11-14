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

//! The GPU form of a light source, as the Forward+ culling and shading shaders read it.

use bytemuck::{Pod, Zeroable};

/// GPU-friendly representation of a light source for compute shader processing.
///
/// This structure is designed for efficient GPU transfer and compute shader access.
/// It uses a unified layout that can represent all light types.
///
/// # Memory Layout
///
/// Total size: 80 bytes — 18 4-byte fields, then 8 bytes of padding. WGSL
/// aligns a struct holding `vec3`s to 16 bytes, so `array<GpuLight>` in the
/// shaders steps 80 bytes per light; the Rust struct must step the same.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuLight {
    /// Light position in world space (ignored for directional lights).
    pub position: [f32; 3],
    /// Maximum range of the light (point/spot lights only).
    pub range: f32,

    /// Light color (RGB, linear space).
    pub color: [f32; 3],
    /// Light intensity multiplier.
    pub intensity: f32,

    /// Light direction (normalized, for directional/spot lights).
    pub direction: [f32; 3],
    /// Light type: 0 = directional, 1 = point, 2 = spot.
    pub light_type: u32,

    /// Cosine of inner cone angle (spot lights only).
    pub inner_cone_cos: f32,
    /// Cosine of outer cone angle (spot lights only).
    pub outer_cone_cos: f32,

    /// Index into the shadow texture array, or -1 if no shadow.
    pub shadow_map_index: i32,
    /// Shadow bias.
    pub shadow_bias: f32,
    /// Shadow normal bias.
    pub shadow_normal_bias: f32,
    /// Far plane of the per-face shadow projection (point lights only —
    /// matches `ShadowEntry::Cube.far_plane`). Unused for directional
    /// and spot lights; left at `0.0` in that case.
    pub shadow_far_plane: f32,
    /// Rounds the struct up to the 16-byte alignment WGSL gives it (it holds
    /// `vec3`s), so light `i` sits where `array<GpuLight>[i]` reads it.
    pub _padding: [f32; 2],
}

impl GpuLight {
    /// Light type constant for directional lights.
    pub const TYPE_DIRECTIONAL: u32 = 0;
    /// Light type constant for point lights.
    pub const TYPE_POINT: u32 = 1;
    /// Light type constant for spot lights.
    pub const TYPE_SPOT: u32 = 2;

    /// Creates a `GpuLight` from world-space position, direction, and light properties.
    pub fn from_parts(position: [f32; 3], direction: [f32; 3], ty: &super::LightType) -> Self {
        match ty {
            super::LightType::Directional(l) => Self {
                position: [0.0; 3],
                range: 0.0,
                color: [l.color.r, l.color.g, l.color.b],
                intensity: l.intensity,
                direction,
                light_type: Self::TYPE_DIRECTIONAL,
                inner_cone_cos: 0.0,
                outer_cone_cos: 0.0,
                shadow_map_index: -1,
                shadow_bias: l.shadow_bias,
                shadow_normal_bias: l.shadow_normal_bias,
                shadow_far_plane: 0.0,
                _padding: [0.0; 2],
            },
            super::LightType::Point(l) => Self {
                position,
                range: l.range,
                color: [l.color.r, l.color.g, l.color.b],
                intensity: l.intensity,
                direction: [0.0; 3],
                light_type: Self::TYPE_POINT,
                inner_cone_cos: 0.0,
                outer_cone_cos: 0.0,
                shadow_map_index: -1,
                shadow_bias: l.shadow_bias,
                shadow_normal_bias: l.shadow_normal_bias,
                shadow_far_plane: 0.0,
                _padding: [0.0; 2],
            },
            super::LightType::Spot(l) => Self {
                position,
                range: l.range,
                color: [l.color.r, l.color.g, l.color.b],
                intensity: l.intensity,
                direction,
                light_type: Self::TYPE_SPOT,
                inner_cone_cos: l.inner_cone_angle.cos(),
                outer_cone_cos: l.outer_cone_angle.cos(),
                shadow_map_index: -1,
                shadow_bias: l.shadow_bias,
                shadow_normal_bias: l.shadow_normal_bias,
                shadow_far_plane: 0.0,
                _padding: [0.0; 2],
            },
        }
    }
}

impl Default for GpuLight {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            range: 10.0,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            direction: [0.0, -1.0, 0.0],
            light_type: Self::TYPE_POINT,
            inner_cone_cos: 0.9, // ~25 degrees
            outer_cone_cos: 0.7, // ~45 degrees
            shadow_map_index: -1,
            shadow_bias: 0.01,
            shadow_normal_bias: 0.0,
            shadow_far_plane: 0.0,
            _padding: [0.0; 2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_light_size_and_alignment() {
        // 18 4-byte fields rounded up to WGSL's 16-byte struct alignment —
        // the stride of `array<GpuLight>` in the shaders.
        assert_eq!(std::mem::size_of::<GpuLight>(), 80);
    }

    #[test]
    fn test_gpu_light_default() {
        let light = GpuLight::default();
        assert_eq!(light.light_type, GpuLight::TYPE_POINT);
        assert_eq!(light.color, [1.0, 1.0, 1.0]);
    }
}
