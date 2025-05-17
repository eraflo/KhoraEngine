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

use crate::math::{LinearRgba, Mat4, Vec3};

// --- Shader Types ---

/// Defines the stage in the graphics pipeline a shader module is intended for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

// --- Renderer Types ---

/// Generic representation of the graphics backend type (e.g., Vulkan, Metal, OpenGL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RendererBackendType {
    Vulkan,
    Metal,
    Dx12,
    OpenGl,
    WebGpu,
    #[default]
    Unknown,
}

/// Generic representation of the GPU device type (e.g., Integrated, Discrete).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RendererDeviceType {
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
    #[default]
    Unknown,
}

/// Provides standardized, backend-agnostic information about the graphics adapter.
#[derive(Debug, Clone, Default)]
pub struct RendererAdapterInfo {
    pub name: String,
    pub backend_type: RendererBackendType,
    pub device_type: RendererDeviceType,
}

// --- Render Types ---

/// Structure representing the view information for rendering.
/// Contains the view matrix, projection matrix, and camera position.
/// This structure is used to pass view-related information to the rendering system.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub view_matrix: Mat4,
    pub projection_matrix: Mat4,
    pub camera_position: Vec3,
}

impl Default for ViewInfo {
    fn default() -> Self {
        Self {
            view_matrix: Mat4::IDENTITY,
            projection_matrix: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
        }
    }
}

/// Structure representing the rendering strategy.
/// This structure defines how the rendering will be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStrategy {
    Forward,
    Deferred,
    Custom(u32),
}

/// Structure representing the render statistics.
#[derive(Debug, Default, Clone)]
pub struct RenderStats {
    pub frame_number: u64,
    pub cpu_preparation_time_ms: f32,
    pub cpu_render_submission_time_ms: f32,
    pub gpu_time_ms: f32,
    pub draw_calls: u32,
    pub triangles_rendered: u32,
    pub vram_usage_estimate_mb: f32,
}

/// Structure representing a renderable object.
#[derive(Debug, Clone)]
pub struct RenderObject {
    pub transform: Mat4,
    pub mesh_id: usize,
    pub color: LinearRgba,
}

/// Structure representing the rendering settings.
/// Contains the rendering strategy, quality level, and other global rendering parameters.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub strategy: RenderStrategy,
    pub quality_level: u32, // 1 = Low, 2 = Medium, 3 = High
    pub show_wireframe: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            strategy: RenderStrategy::Forward,
            quality_level: 1,
            show_wireframe: false,
        }
    }
}
