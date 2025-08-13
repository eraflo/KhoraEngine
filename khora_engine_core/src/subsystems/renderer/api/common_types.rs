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

/// Specify the size of indices in the index buffer.
/// Used to optimize drawing by reusing vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    Uint16,
    Uint32,
}

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

/// Structure representing the rendering strategy.
/// This structure defines how the rendering will be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStrategy {
    Forward,
    Deferred,
    Custom(u32),
}

/// Number of samples per pixel for MSAA (Multisample Anti-Aliasing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SampleCount {
    #[default]
    X1,
    X2,
    X4,
    X8,
    X16,
    X32,
    X64,
}

/// Defines the stage in the graphics pipeline a shader module is intended for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

/// Defines pixels format for textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    R8Unorm,
    Rg8Unorm,
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8UnormSrgb,
    R16Float,
    Rg16Float,
    Rgba16Float,
    R32Float,
    Rg32Float,
    Rgba32Float,
    Depth16Unorm,
    Depth24Plus,
    Depth24PlusStencil8,
    Depth32Float,
    Depth32FloatStencil8,
}

impl TextureFormat {
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            TextureFormat::R8Unorm => 1,
            TextureFormat::Rg8Unorm => 2,
            TextureFormat::Rgba8Unorm => 4,
            TextureFormat::Rgba8UnormSrgb => 4,
            TextureFormat::Bgra8UnormSrgb => 4,
            TextureFormat::R16Float => 2,
            TextureFormat::Rg16Float => 4,
            TextureFormat::Rgba16Float => 8,
            TextureFormat::R32Float => 4,
            TextureFormat::Rg32Float => 8,
            TextureFormat::Rgba32Float => 16,
            TextureFormat::Depth16Unorm => 2,
            TextureFormat::Depth24Plus => 4, // Often treated as 4 bytes for alignment
            TextureFormat::Depth24PlusStencil8 => 4, // Common packed size for both depth and stencil
            TextureFormat::Depth32Float => 4,
            TextureFormat::Depth32FloatStencil8 => 5, // 4 bytes for depth + 1 byte for stencil (may vary by backend)
        }
    }
}

// --- Structs ---

/// Provides standardized, backend-agnostic information about the graphics adapter.
#[derive(Debug, Clone, Default)]
pub struct RendererAdapterInfo {
    pub name: String,
    pub backend_type: RendererBackendType,
    pub device_type: RendererDeviceType,
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
    /// Enable/disable GPU timestamp instrumentation (runtime toggle)
    pub enable_gpu_timestamps: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            strategy: RenderStrategy::Forward,
            quality_level: 1,
            show_wireframe: false,
            enable_gpu_timestamps: true,
        }
    }
}

/// Structure representing the render statistics.
#[derive(Debug, Clone)]
pub struct RenderStats {
    /// Sequential frame counter (incremented each successful render)
    pub frame_number: u64,
    /// CPU time spent in pre-render preparation (resource updates, culling, etc.)
    /// NOTE: Current calculation is provisional and will be refined.
    pub cpu_preparation_time_ms: f32,
    /// CPU time to submit encoded GPU work (encoder.finish + queue.submit overhead). Typically tiny.
    pub cpu_render_submission_time_ms: f32,
    /// GPU duration (ms) of the "main render pass" measured between pass_begin & pass_end timestamps.
    /// If timestamp queries unsupported or not yet resolved, retains previous value.
    pub gpu_main_pass_time_ms: f32,
    /// Total GPU frame time (ms) between frame_start & frame_end timestamps (can include future multi-pass work).
    /// May differ from main pass time when multiple passes or GPU-side operations are added.
    pub gpu_frame_total_time_ms: f32,
    /// Number of draw calls actually encoded (currently placeholder until drawing implemented).
    pub draw_calls: u32,
    /// Total triangles submitted (currently placeholder).
    pub triangles_rendered: u32,
    /// Estimated VRAM usage in MB (placeholder until integrated with VRAM tracker / metrics system).
    pub vram_usage_estimate_mb: f32,
}

/// Generic GPU performance hook identifiers. Other backends may ignore some hooks
/// or support additional ones in the future. The logical order reflects a simple
/// frame; extra passes (Shadow, PostProcess, etc.) can be appended later without
/// breaking the existing API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPerfHook {
    FrameStart,
    MainPassBegin,
    MainPassEnd,
    FrameEnd,
}

impl GpuPerfHook {
    pub const ALL: [GpuPerfHook; 4] = [
        GpuPerfHook::FrameStart,
        GpuPerfHook::MainPassBegin,
        GpuPerfHook::MainPassEnd,
        GpuPerfHook::FrameEnd,
    ];
}

impl Default for RenderStats {
    fn default() -> Self {
        Self {
            frame_number: 0,
            cpu_preparation_time_ms: 0.0,
            cpu_render_submission_time_ms: 0.0,
            gpu_main_pass_time_ms: 0.0,
            gpu_frame_total_time_ms: 0.0,
            draw_calls: 0,
            triangles_rendered: 0,
            vram_usage_estimate_mb: 0.0,
        }
    }
}

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
