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

//! Provides common, backend-agnostic enums and data structures for the rendering API.

use crate::{
    math::{Mat4, Vec3},
    renderer::{BufferId, RenderPipelineId},
};

/// Specifies the data type of indices in an index buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    /// Indices are 16-bit unsigned integers.
    Uint16,
    /// Indices are 32-bit unsigned integers.
    Uint32,
}

/// A backend-agnostic representation of a graphics API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GraphicsBackendType {
    /// Vulkan API.
    Vulkan,
    /// Apple's Metal API.
    Metal,
    /// Microsoft's DirectX 12 API.
    Dx12,
    /// Microsoft's DirectX 11 API.
    Dx11,
    /// OpenGL API.
    OpenGL,
    /// WebGPU API (for web builds).
    WebGpu,
    /// An unknown or unsupported backend.
    #[default]
    Unknown,
}

/// The physical type of a graphics device (GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RendererDeviceType {
    /// A GPU integrated into the CPU.
    IntegratedGpu,
    /// A discrete, dedicated GPU.
    DiscreteGpu,
    /// A virtualized or software-based GPU.
    VirtualGpu,
    /// A software renderer running on the CPU.
    Cpu,
    /// An unknown or unsupported device type.
    #[default]
    Unknown,
}

/// Defines a high-level rendering strategy.
/// This will be used by the `RenderAgent` to select the appropriate `RenderLane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStrategy {
    /// A forward rendering pipeline.
    Forward,
    /// A deferred shading pipeline.
    Deferred,
    /// A custom, user-defined strategy identified by a number.
    Custom(u32),
}

/// The number of samples per pixel for Multisample Anti-Aliasing (MSAA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SampleCount {
    /// 1 sample per pixel (MSAA disabled).
    #[default]
    X1,
    /// 2 samples per pixel.
    X2,
    /// 4 samples per pixel.
    X4,
    /// 8 samples per pixel.
    X8,
    /// 16 samples per pixel.
    X16,
    /// 32 samples per pixel.
    X32,
    /// 64 samples per pixel.
    X64,
}

/// Defines the programmable stage in the graphics pipeline a shader module is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    /// The vertex shader stage.
    Vertex,
    /// The fragment (or pixel) shader stage.
    Fragment,
    /// The compute shader stage.
    Compute,
}

/// Defines the memory format of pixels in a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    // 8-bit formats
    /// One 8-bit unsigned normalized component.
    R8Unorm,
    /// Two 8-bit unsigned normalized components.
    Rg8Unorm,
    /// Four 8-bit unsigned normalized components (RGBA).
    Rgba8Unorm,
    /// Four 8-bit unsigned normalized components (RGBA) in the sRGB color space.
    Rgba8UnormSrgb,
    /// Four 8-bit unsigned normalized components (BGRA) in the sRGB color space. This is a common swapchain format.
    Bgra8UnormSrgb,
    // 16-bit float formats
    /// One 16-bit float component.
    R16Float,
    /// Two 16-bit float components.
    Rg16Float,
    /// Four 16-bit float components.
    Rgba16Float,
    // 32-bit float formats
    /// One 32-bit float component.
    R32Float,
    /// Two 32-bit float components.
    Rg32Float,
    /// Four 32-bit float components.
    Rgba32Float,
    // Depth/stencil formats
    /// A 16-bit unsigned normalized depth format.
    Depth16Unorm,
    /// A 24-bit unsigned normalized depth format.
    Depth24Plus,
    /// A 24-bit unsigned normalized depth format with an 8-bit stencil component.
    Depth24PlusStencil8,
    /// A 32-bit float depth format.
    Depth32Float,
    /// A 32-bit float depth format with an 8-bit stencil component.
    Depth32FloatStencil8,
}

impl TextureFormat {
    /// Returns the size in bytes of a single pixel for this format.
    /// Note: This can be an approximation for packed or complex formats.
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
            TextureFormat::Depth24Plus => 4,
            TextureFormat::Depth24PlusStencil8 => 4,
            TextureFormat::Depth32Float => 4,
            TextureFormat::Depth32FloatStencil8 => 5,
        }
    }
}

// --- Structs ---

/// Provides standardized, backend-agnostic information about the graphics adapter.
#[derive(Debug, Clone, Default)]
pub struct RendererAdapterInfo {
    /// The name of the adapter (e.g., "NVIDIA GeForce RTX 4090").
    pub name: String,
    /// The graphics API backend this adapter is associated with.
    pub backend_type: GraphicsBackendType,
    /// The physical type of the adapter.
    pub device_type: RendererDeviceType,
}

/// Provides standardized, backend-agnostic information about a graphics adapter.
#[derive(Debug, Clone, Default)]
pub struct GraphicsAdapterInfo {
    /// The name of the adapter (e.g., "NVIDIA GeForce RTX 4090").
    pub name: String,
    /// The graphics API backend this adapter is associated with.
    pub backend_type: GraphicsBackendType,
    /// The physical type of the adapter.
    pub device_type: RendererDeviceType,
}

/// A simple representation of a single object to be rendered in a pass.
/// TODO: evolve this structure to a more high level abstraction for real 3D objects.
#[derive(Debug, Clone)]
pub struct RenderObject {
    /// The [`RenderPipelineId`] to bind for this object.
    pub pipeline: RenderPipelineId,
    /// The vertex buffer to bind.
    pub vertex_buffer: BufferId,
    /// The index buffer to bind.
    pub index_buffer: BufferId,
    /// The number of indices to draw from the index buffer.
    pub index_count: u32,
}

/// A collection of global settings that can affect the rendering process.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    /// The desired high-level rendering strategy.
    pub strategy: RenderStrategy,
    /// A generic quality level (e.g., 1=Low, 2=Medium, 3=High).
    pub quality_level: u32,
    /// If `true`, objects should be rendered in wireframe mode.
    pub show_wireframe: bool,
    /// The quiet period in milliseconds after a resize event before the surface is reconfigured.
    pub resize_debounce_ms: u64,
    /// A fallback number of frames after which a pending resize is forced, even if events are still incoming.
    pub resize_max_pending_frames: u32,
    /// A runtime toggle to enable/disable GPU timestamp instrumentation for profiling.
    pub enable_gpu_timestamps: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            strategy: RenderStrategy::Forward,
            quality_level: 1,
            show_wireframe: false,
            resize_debounce_ms: 120,
            resize_max_pending_frames: 10,
            enable_gpu_timestamps: true,
        }
    }
}

/// A collection of performance statistics for a single rendered frame.
#[derive(Debug, Clone)]
pub struct RenderStats {
    /// A sequential counter for rendered frames.
    pub frame_number: u64,
    /// The CPU time spent in pre-render preparation (resource updates, culling, etc.).
    pub cpu_preparation_time_ms: f32,
    /// The CPU time spent submitting encoded command buffers to the GPU.
    pub cpu_render_submission_time_ms: f32,
    /// The GPU execution time of the main render pass, as measured by timestamp queries.
    pub gpu_main_pass_time_ms: f32,
    /// The total GPU execution time for the entire frame, as measured by timestamp queries.
    pub gpu_frame_total_time_ms: f32,
    /// The number of draw calls encoded for the frame.
    pub draw_calls: u32,
    /// The total number of triangles submitted for the frame.
    pub triangles_rendered: u32,
    /// An estimate of the VRAM usage in megabytes.
    pub vram_usage_estimate_mb: f32,
}

/// Represents a specific point in a frame's GPU execution for timestamping.
///
/// These are used by a [`GpuProfiler`] to record timestamps and measure performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuHook {
    /// Marks the absolute beginning of GPU work for a frame.
    FrameStart,
    /// Marks the beginning of the main render pass.
    MainPassBegin,
    /// Marks the end of the main render pass.
    MainPassEnd,
    /// Marks the absolute end of GPU work for a frame.
    FrameEnd,
}

impl GpuHook {
    /// An array containing all `GpuHook` variants.
    pub const ALL: [GpuHook; 4] = [
        GpuHook::FrameStart,
        GpuHook::MainPassBegin,
        GpuHook::MainPassEnd,
        GpuHook::FrameEnd,
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

/// Contains camera and projection information needed to render a specific view.
/// This data is typically uploaded to a uniform buffer for shader access.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    /// The camera's view matrix (world to view space).
    pub view_matrix: Mat4,
    /// The camera's projection matrix (view to clip space).
    pub projection_matrix: Mat4,
    /// The camera's position in world space.
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
