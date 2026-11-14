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

//! The uniform data of the light culling compute shader.

/// Data for the light culling compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CullingUniformsData {
    /// The combined view-projection matrix (world → clip); the shader
    /// extracts each tile's planes from it.
    pub view_projection: [[f32; 4]; 4],
    /// The dimensions of the screen in pixels.
    pub screen_dimensions: [f32; 2],
    /// The number of tiles in the X and Y dimensions.
    pub tile_count: [u32; 2],
    /// The total number of lights in the scene.
    pub num_lights: u32,
    /// The size of a tile in pixels.
    pub tile_size: u32,
    /// How many lights a tile may list; the shader clamps it to its stride.
    pub max_lights_per_tile: u32,
    /// Padding for 16-byte alignment.
    pub _padding: u32,
}
