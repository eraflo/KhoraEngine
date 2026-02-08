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
    math::{LinearRgba, Mat4, Vec3},
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

/// Flags representing which shader stages can access a resource binding.
///
/// This is used in bind group layouts to specify visibility of resources.
/// Multiple stages can be combined using bitwise operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderStageFlags {
    bits: u32,
}

impl ShaderStageFlags {
    /// No shader stages.
    pub const NONE: Self = Self { bits: 0 };
    /// Vertex shader stage.
    pub const VERTEX: Self = Self { bits: 1 << 0 };
    /// Fragment shader stage.
    pub const FRAGMENT: Self = Self { bits: 1 << 1 };
    /// Compute shader stage.
    pub const COMPUTE: Self = Self { bits: 1 << 2 };
    /// All graphics stages (vertex + fragment).
    pub const VERTEX_FRAGMENT: Self = Self {
        bits: Self::VERTEX.bits | Self::FRAGMENT.bits,
    };
    /// All stages.
    pub const ALL: Self = Self {
        bits: Self::VERTEX.bits | Self::FRAGMENT.bits | Self::COMPUTE.bits,
    };

    /// Creates a new set of shader stage flags from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Creates flags from a single shader stage.
    pub const fn from_stage(stage: ShaderStage) -> Self {
        match stage {
            ShaderStage::Vertex => Self::VERTEX,
            ShaderStage::Fragment => Self::FRAGMENT,
            ShaderStage::Compute => Self::COMPUTE,
        }
    }

    /// Returns the raw bits.
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Combines two sets of flags.
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    /// Checks if these flags contain a specific stage.
    pub const fn contains(&self, stage: ShaderStage) -> bool {
        let stage_bits = Self::from_stage(stage).bits;
        (self.bits & stage_bits) == stage_bits
    }

    /// Checks if these flags are empty (no stages).
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

impl std::ops::BitOr for ShaderStageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for ShaderStageFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
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

/// A low-level representation of a single draw call to be processed by a [`RenderLane`].
///
/// This structure links GPU buffers and pipelines, serving as the common data format
/// produced by ISAs (like `RenderAgent`) and consumed by specialized rendering lanes.
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

impl ViewInfo {
    /// Creates a new `ViewInfo` from individual components.
    pub fn new(view_matrix: Mat4, projection_matrix: Mat4, camera_position: Vec3) -> Self {
        Self {
            view_matrix,
            projection_matrix,
            camera_position,
        }
    }

    /// Calculates the combined view-projection matrix.
    ///
    /// This is the product of the projection matrix and the view matrix,
    /// which transforms from world space directly to clip space.
    pub fn view_projection_matrix(&self) -> Mat4 {
        self.projection_matrix * self.view_matrix
    }
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

/// The GPU-side representation of camera uniform data.
///
/// This structure is designed to be directly uploaded to a uniform buffer.
/// The layout must match the uniform block declaration in the shader.
///
/// **Important:** WGSL has specific alignment requirements. Mat4 is aligned to 16 bytes,
/// and Vec3 needs padding to be treated as Vec4 in uniform buffers.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct CameraUniformData {
    /// The combined view-projection matrix (projection * view).
    pub view_projection: Mat4,
    /// The camera's position in world space.
    /// Note: The fourth component is padding for alignment.
    pub camera_position: [f32; 4],
}

impl CameraUniformData {
    /// Creates camera uniform data from a `ViewInfo`.
    pub fn from_view_info(view_info: &ViewInfo) -> Self {
        Self {
            view_projection: view_info.view_projection_matrix(),
            camera_position: [
                view_info.camera_position.x,
                view_info.camera_position.y,
                view_info.camera_position.z,
                0.0, // Padding for alignment
            ],
        }
    }

    /// Returns the data as a byte slice suitable for uploading to a GPU buffer.
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

// Ensure the struct can be safely cast to bytes for GPU upload
unsafe impl bytemuck::Pod for CameraUniformData {}
unsafe impl bytemuck::Zeroable for CameraUniformData {}

// --- Uniform Buffers ---

/// Data for a single directional light, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DirectionalLightUniform {
    /// Direction vector (xyz), with padding (w).
    pub direction: [f32; 4], // w is padding
    /// Color (rgb) and Intensity (a).
    pub color: LinearRgba,
}

/// Data for a single point light, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLightUniform {
    /// Position (xyz) and Range (w).
    pub position: [f32; 4], // w is range
    /// Color (rgb) and Intensity (a).
    pub color: LinearRgba,
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
}

/// Constants for maximum light counts.
pub const MAX_DIRECTIONAL_LIGHTS: usize = 4;
pub const MAX_POINT_LIGHTS: usize = 16;
pub const MAX_SPOT_LIGHTS: usize = 8;

/// The structure of the global lighting uniform buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingUniforms {
    pub directional_lights: [DirectionalLightUniform; MAX_DIRECTIONAL_LIGHTS],
    pub point_lights: [PointLightUniform; MAX_POINT_LIGHTS],
    pub spot_lights: [SpotLightUniform; MAX_SPOT_LIGHTS],
    pub num_directional_lights: u32,
    pub num_point_lights: u32,
    pub num_spot_lights: u32,
    pub _padding: u32,
}

/// Data for a model's transform, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniforms {
    pub model_matrix: [[f32; 4]; 4],
    pub normal_matrix: [[f32; 4]; 4],
}

/// Data for a material's properties, formatted for the standard Lit Shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialUniforms {
    pub base_color: LinearRgba,
    /// Emissive color (rgb) and Specular Power (a).
    pub emissive: LinearRgba,
    /// Ambient color (rgb) and Padding (a).
    pub ambient: LinearRgba,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_uniform_data_size() {
        // Mat4 = 64 bytes, Vec4 (padding included) = 16 bytes
        // Total should be 80 bytes
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 80);
    }

    #[test]
    fn test_camera_uniform_data_alignment() {
        // CameraUniformData should be aligned to 16 bytes for GPU compatibility
        assert_eq!(std::mem::align_of::<CameraUniformData>(), 16);
    }

    #[test]
    fn test_camera_uniform_data_from_view_info() {
        let view_matrix = Mat4::IDENTITY;
        let projection_matrix = Mat4::IDENTITY;
        let camera_position = Vec3::new(1.0, 2.0, 3.0);

        let view_info = ViewInfo::new(view_matrix, projection_matrix, camera_position);
        let uniform_data = CameraUniformData::from_view_info(&view_info);

        // Check that camera position is correctly set
        assert_eq!(uniform_data.camera_position[0], 1.0);
        assert_eq!(uniform_data.camera_position[1], 2.0);
        assert_eq!(uniform_data.camera_position[2], 3.0);
        assert_eq!(uniform_data.camera_position[3], 0.0); // Padding

        // Check that view_projection is set
        let expected_vp = projection_matrix * view_matrix;
        assert_eq!(uniform_data.view_projection, expected_vp);
    }

    #[test]
    fn test_camera_uniform_data_as_bytes() {
        let view_info = ViewInfo::default();
        let uniform_data = CameraUniformData::from_view_info(&view_info);

        let bytes = uniform_data.as_bytes();
        assert_eq!(bytes.len(), std::mem::size_of::<CameraUniformData>());
    }

    #[test]
    fn test_camera_uniform_data_bytemuck() {
        // Test that we can use bytemuck functions
        let uniform_data = CameraUniformData {
            view_projection: Mat4::IDENTITY,
            camera_position: [0.0, 0.0, 0.0, 0.0],
        };

        let data_array = [uniform_data];
        let bytes: &[u8] = bytemuck::cast_slice(&data_array);
        assert_eq!(bytes.len(), std::mem::size_of::<CameraUniformData>());
    }

    #[test]
    fn test_view_info_new() {
        let view = Mat4::from_translation(Vec3::new(0.0, 0.0, -5.0));
        let proj = Mat4::IDENTITY;
        let pos = Vec3::new(0.0, 1.0, 5.0);

        let view_info = ViewInfo::new(view, proj, pos);

        assert_eq!(view_info.view_matrix, view);
        assert_eq!(view_info.projection_matrix, proj);
        assert_eq!(view_info.camera_position, pos);
    }

    #[test]
    fn test_view_info_view_projection_matrix() {
        let view = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let proj = Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0));
        let pos = Vec3::ZERO;

        let view_info = ViewInfo::new(view, proj, pos);
        let vp = view_info.view_projection_matrix();

        // view_projection should be projection * view
        assert_eq!(vp, proj * view);
    }

    #[test]
    fn test_view_info_default() {
        let view_info = ViewInfo::default();

        assert_eq!(view_info.view_matrix, Mat4::IDENTITY);
        assert_eq!(view_info.projection_matrix, Mat4::IDENTITY);
        assert_eq!(view_info.camera_position, Vec3::ZERO);
    }

    #[test]
    fn test_shader_stage_flags_bitwise() {
        let vertex_fragment = ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT;

        assert!(vertex_fragment.contains(ShaderStage::Vertex));
        assert!(vertex_fragment.contains(ShaderStage::Fragment));
        assert!(!vertex_fragment.contains(ShaderStage::Compute));
    }

    #[test]
    fn test_shader_stage_flags_all() {
        let all = ShaderStageFlags::ALL;

        assert!(all.contains(ShaderStage::Vertex));
        assert!(all.contains(ShaderStage::Fragment));
        assert!(all.contains(ShaderStage::Compute));
    }

    #[test]
    fn test_shader_stage_flags_union() {
        let vertex = ShaderStageFlags::VERTEX;
        let fragment = ShaderStageFlags::FRAGMENT;
        let combined = vertex.union(fragment);

        assert!(combined.contains(ShaderStage::Vertex));
        assert!(combined.contains(ShaderStage::Fragment));
        assert!(!combined.contains(ShaderStage::Compute));
    }

    #[test]
    fn test_shader_stage_flags_from_stage() {
        let vertex_flags = ShaderStageFlags::from_stage(ShaderStage::Vertex);
        assert_eq!(vertex_flags, ShaderStageFlags::VERTEX);

        let fragment_flags = ShaderStageFlags::from_stage(ShaderStage::Fragment);
        assert_eq!(fragment_flags, ShaderStageFlags::FRAGMENT);

        let compute_flags = ShaderStageFlags::from_stage(ShaderStage::Compute);
        assert_eq!(compute_flags, ShaderStageFlags::COMPUTE);
    }

    #[test]
    fn test_shader_stage_flags_is_empty() {
        let none = ShaderStageFlags::NONE;
        assert!(none.is_empty());

        let vertex = ShaderStageFlags::VERTEX;
        assert!(!vertex.is_empty());
    }

    #[test]
    fn test_shader_stage_flags_bitor_assign() {
        let mut flags = ShaderStageFlags::VERTEX;
        flags |= ShaderStageFlags::FRAGMENT;

        assert!(flags.contains(ShaderStage::Vertex));
        assert!(flags.contains(ShaderStage::Fragment));
    }
}
