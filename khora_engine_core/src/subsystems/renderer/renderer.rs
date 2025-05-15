use winit::dpi::PhysicalSize;

use crate::{math::{LinearRgba, Mat4, Vec3}, window::KhoraWindow};


/// Structure representing the view information for rendering.
/// Contains the view matrix, projection matrix, and camera position.
/// This structure is used to pass view-related information to the rendering system.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub view_matrix: Mat4,
    pub projection_matrix: Mat4,
    pub camera_position: Vec3
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


/// Structure representing the rendering settings.
/// Contains the rendering strategy, quality level, and other global rendering parameters.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub strategy: RenderStrategy,
    pub quality_level: u32, // 1 = Low, 2 = Medium, 3 = High
    pub show_wireframe: bool
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


/// Structure representing a renderable object.
#[derive(Debug, Clone)]
pub struct RenderObject {
    pub transform: Mat4,
    pub mesh_id: usize,
    pub color: LinearRgba
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


#[derive(Debug)]
pub enum RenderSystemError {
    InitializationFailed(String),
    SurfaceAcquisitionFailed(String),
    RenderFailed(String),
    ResourceCreationFailed(String)
}

impl std::fmt::Display for RenderSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderSystemError::InitializationFailed(s) => write!(f, "RenderSystem Initialization Failed: {}", s),
            RenderSystemError::SurfaceAcquisitionFailed(s) => write!(f, "RenderSystem Surface Acquisition Failed: {}", s),
            RenderSystemError::RenderFailed(s) => write!(f, "RenderSystem Render Failed: {}", s),
            RenderSystemError::ResourceCreationFailed(s) => write!(f, "RenderSystem Resource Creation Failed: {}", s),
        }
    }
}

impl std::error::Error for RenderSystemError {}


/// Enum representing the type of renderer backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererBackendType {
    Vulkan,
    Metal,
    Dx12,
    OpenGl,
    WebGpu,
    Unknown,
}


/// Structure representing the device type.
/// This structure is used to identify the type of device used for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererDeviceType {
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
    Unknown,
}


/// Structure representing the renderer adapter information.
/// This structure contains the name of the adapter, the backend type, and the device type.
#[derive(Debug, Clone)]
pub struct RendererAdapterInfo {
    pub name: String,
    pub backend_type: RendererBackendType,
    pub device_type: RendererDeviceType
}


/// Trait representing a render system.
/// This trait defines the methods that a render system must implement.
pub trait RenderSystem: std::fmt::Debug + Send + Sync {

    /// Initialize the rendering system.
    /// This method is called once at the beginning of the application.
    fn init(&mut self, window: &KhoraWindow) -> Result<(), RenderSystemError>;

    /// Resize the window of the render system.
    fn resize(&mut self, new_size: PhysicalSize<u32>);

    /// Prepare the frame for rendering.
    /// This method is called before the actual rendering process.
    fn prepare_frame(&mut self, view_info: &ViewInfo);

    /// Render the frame to the window.
    fn render(
        &mut self,
        renderables: &[RenderObject],
        view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderSystemError>;

    /// Get the stats of the last rendered frame.
    fn get_last_frame_stats(&self) -> &RenderStats;

    /// Indicate if a specific feature is supported.
    fn supports_feature(&self, feature_name: &str) -> bool;

    /// Clean up and release the resources of the rendering system.
    fn shutdown(&mut self);


    /// Get the adapter information of the rendering system.
    fn get_adapter_info(&self) -> Option<RendererAdapterInfo>;

}