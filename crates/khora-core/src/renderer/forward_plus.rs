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

//! Defines data structures for Forward+ (Tiled Forward) rendering.
//!
//! Forward+ is an advanced rendering technique that optimizes multi-light
//! scenarios by dividing the screen into tiles and pre-computing which lights
//! affect each tile using a compute shader pass.
//!
//! # SAA Integration
//!
//! The `ForwardPlusLane` is a **strategy** of the `RenderAgent` ISA. The agent
//! can select between `LitForwardLane` and `ForwardPlusLane` based on:
//! - Scene light count (Forward+ typically wins when > 20 lights)
//! - GORNA budget allocation
//!
//! # Performance Characteristics
//!
//! - **Complexity**: O(meshes × lights_per_tile) vs O(meshes × lights) for Forward
//! - **Overhead**: Fixed compute pass cost for light culling (~0.5ms)
//! - **Memory**: Light grid and index buffers scale with screen resolution

use bytemuck::{Pod, Zeroable};

/// The tile size for Forward+ light culling.
///
/// Smaller tiles provide more precise culling but increase compute overhead.
/// Larger tiles reduce overhead but may include more lights per tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TileSize {
    /// 16×16 pixel tiles (standard, precise culling).
    #[default]
    X16,
    /// 32×32 pixel tiles (less overhead, coarser culling).
    X32,
}

impl TileSize {
    /// Returns the tile size in pixels.
    #[inline]
    pub const fn pixels(&self) -> u32 {
        match self {
            TileSize::X16 => 16,
            TileSize::X32 => 32,
        }
    }

    /// Calculates the number of tiles needed for a given screen dimension.
    #[inline]
    pub const fn tile_count(&self, screen_size: u32) -> u32 {
        screen_size.div_ceil(self.pixels())
    }
}

/// Configuration for Forward+ tiled rendering.
///
/// This configuration is **adaptive** and can be adjusted by GORNA or the
/// `RenderAgent` based on runtime conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardPlusTileConfig {
    /// The tile size for light culling.
    pub tile_size: TileSize,
    /// Maximum number of lights per tile.
    /// Higher values handle dense light clusters but use more memory.
    pub max_lights_per_tile: u32,
    /// Whether to use a depth pre-pass to improve light culling.
    /// Adds ~0.5ms but improves culling by 20-30% for scenes with depth variation.
    pub use_depth_prepass: bool,
}

impl Default for ForwardPlusTileConfig {
    fn default() -> Self {
        Self {
            tile_size: TileSize::X16,
            max_lights_per_tile: 128,
            use_depth_prepass: false,
        }
    }
}

impl ForwardPlusTileConfig {
    /// Creates a new configuration with default values.
    pub const fn new() -> Self {
        Self {
            tile_size: TileSize::X16,
            max_lights_per_tile: 128,
            use_depth_prepass: false,
        }
    }

    /// Creates a configuration optimized for many lights.
    pub const fn high_light_count() -> Self {
        Self {
            tile_size: TileSize::X16,
            max_lights_per_tile: 256,
            use_depth_prepass: true,
        }
    }

    /// Creates a configuration optimized for low overhead.
    pub const fn low_overhead() -> Self {
        Self {
            tile_size: TileSize::X32,
            max_lights_per_tile: 64,
            use_depth_prepass: false,
        }
    }

    /// Calculates the tile grid dimensions for a given screen size.
    #[inline]
    pub const fn tile_dimensions(&self, screen_width: u32, screen_height: u32) -> (u32, u32) {
        (
            self.tile_size.tile_count(screen_width),
            self.tile_size.tile_count(screen_height),
        )
    }

    /// Calculates the total number of tiles for a given screen size.
    #[inline]
    pub fn total_tiles(&self, screen_width: u32, screen_height: u32) -> u32 {
        let (tiles_x, tiles_y) = self.tile_dimensions(screen_width, screen_height);
        tiles_x * tiles_y
    }

    /// Calculates the required light index buffer size in bytes.
    pub fn light_index_buffer_size(&self, screen_width: u32, screen_height: u32) -> u64 {
        let total_tiles = self.total_tiles(screen_width, screen_height) as u64;
        total_tiles * self.max_lights_per_tile as u64 * std::mem::size_of::<u32>() as u64
    }

    /// Calculates the required light grid buffer size in bytes.
    /// Each tile stores (offset: u32, count: u32).
    pub fn light_grid_buffer_size(&self, screen_width: u32, screen_height: u32) -> u64 {
        let total_tiles = self.total_tiles(screen_width, screen_height) as u64;
        total_tiles * 2 * std::mem::size_of::<u32>() as u64
    }
}

/// GPU-friendly representation of a light source for compute shader processing.
///
/// This structure is designed for efficient GPU transfer and compute shader access.
/// It uses a unified layout that can represent all light types.
///
/// # Memory Layout
///
/// Total size: 72 bytes (18 × 4-byte fields), padded from 64 after shadow fields were added.
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
    /// Padding/Reserved.
    pub _unused: f32,
}

impl GpuLight {
    /// Light type constant for directional lights.
    pub const TYPE_DIRECTIONAL: u32 = 0;
    /// Light type constant for point lights.
    pub const TYPE_POINT: u32 = 1;
    /// Light type constant for spot lights.
    pub const TYPE_SPOT: u32 = 2;

    /// Creates a `GpuLight` from world-space position, direction, and light properties.
    pub fn from_parts(
        position: [f32; 3],
        direction: [f32; 3],
        ty: &super::light::LightType,
    ) -> Self {
        match ty {
            super::light::LightType::Directional(l) => Self {
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
                _unused: 0.0,
            },
            super::light::LightType::Point(l) => Self {
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
                _unused: 0.0,
            },
            super::light::LightType::Spot(l) => Self {
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
                _unused: 0.0,
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
            _unused: 0.0,
        }
    }
}

/// Uniforms for the light culling compute shader.
///
/// This structure is uploaded to GPU each frame with the current camera
/// and screen state for the light culling pass.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct LightCullingUniforms {
    /// View-projection matrix for frustum calculations.
    pub view_projection: [[f32; 4]; 4],
    /// Inverse projection matrix for reconstructing view-space positions.
    pub inverse_projection: [[f32; 4]; 4],

    /// Screen dimensions in pixels (width, height).
    pub screen_dimensions: [f32; 2],
    /// Tile grid dimensions (tiles_x, tiles_y).
    pub tile_count: [u32; 2],

    /// Number of active lights in the light buffer.
    pub num_lights: u32,
    /// Tile size in pixels.
    pub tile_size: u32,
    /// Index of the first directional light's shadow map.
    pub shadow_atlas_index: i32,
    /// Padding for 16-byte alignment.
    pub _padding: [f32; 1],
}

impl Default for LightCullingUniforms {
    fn default() -> Self {
        Self {
            view_projection: [[0.0; 4]; 4],
            inverse_projection: [[0.0; 4]; 4],
            screen_dimensions: [1920.0, 1080.0],
            tile_count: [120, 68], // 1920/16, 1080/16 rounded up
            num_lights: 0,
            tile_size: 16,
            shadow_atlas_index: -1,
            _padding: [0.0; 1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_size_pixels() {
        assert_eq!(TileSize::X16.pixels(), 16);
        assert_eq!(TileSize::X32.pixels(), 32);
    }

    #[test]
    fn test_tile_count_calculation() {
        // 1920 / 16 = 120 tiles exactly
        assert_eq!(TileSize::X16.tile_count(1920), 120);
        // 1080 / 16 = 67.5 -> 68 tiles (rounded up)
        assert_eq!(TileSize::X16.tile_count(1080), 68);
        // 1920 / 32 = 60 tiles exactly
        assert_eq!(TileSize::X32.tile_count(1920), 60);
    }

    #[test]
    fn test_forward_plus_tile_config_default() {
        let config = ForwardPlusTileConfig::default();
        assert_eq!(config.tile_size, TileSize::X16);
        assert_eq!(config.max_lights_per_tile, 128);
        assert!(!config.use_depth_prepass);
    }

    #[test]
    fn test_tile_dimensions() {
        let config = ForwardPlusTileConfig::default();
        let (tiles_x, tiles_y) = config.tile_dimensions(1920, 1080);
        assert_eq!(tiles_x, 120);
        assert_eq!(tiles_y, 68);
    }

    #[test]
    fn test_gpu_light_size_and_alignment() {
        // GpuLight should be exactly 72 bytes (18 x 4-byte fields)
        // Updated from 64 after shadow fields (shadow_map_index, shadow_bias, shadow_normal_bias, _padding) were added.
        assert_eq!(std::mem::size_of::<GpuLight>(), 72);
    }

    #[test]
    fn test_light_culling_uniforms_size() {
        // LightCullingUniforms should be a multiple of 16 bytes for GPU alignment
        let size = std::mem::size_of::<LightCullingUniforms>();
        assert_eq!(
            size % 16,
            0,
            "LightCullingUniforms should be 16-byte aligned"
        );
    }

    #[test]
    fn test_gpu_light_default() {
        let light = GpuLight::default();
        assert_eq!(light.light_type, GpuLight::TYPE_POINT);
        assert_eq!(light.color, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_buffer_size_calculation() {
        let config = ForwardPlusTileConfig::default();
        // 120 * 68 = 8160 tiles
        // Light index buffer: 8160 * 128 * 4 = 4,177,920 bytes
        let index_size = config.light_index_buffer_size(1920, 1080);
        assert_eq!(index_size, 8160 * 128 * 4);

        // Light grid buffer: 8160 * 2 * 4 = 65,280 bytes
        let grid_size = config.light_grid_buffer_size(1920, 1080);
        assert_eq!(grid_size, 8160 * 2 * 4);
    }
}
