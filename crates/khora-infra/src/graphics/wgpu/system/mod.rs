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

//! The concrete, WGPU-based implementation of the `RenderSystem` trait.

use crate::telemetry::gpu_monitor::GpuMonitor;

use crate::graphics::wgpu::backend::WgpuBackendSelector;
use crate::graphics::wgpu::device::WgpuDevice;
use crate::graphics::wgpu::graphics_context::WgpuGraphicsContext;
use crate::graphics::wgpu::profiler::WgpuTimestampProfiler;
use crate::graphics::wgpu::resilience::{
    classify_acquire, surface_is_renderable, AcquireAction, SurfaceAcquireStatus,
};
use khora_core::platform::window::KhoraWindowHandle;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::device::{BackendSelectionConfig, RenderStats};
use khora_core::renderer::api::resource::{
    BufferId, ImageAspect, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewId, ViewInfo,
};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_core::renderer::api::util::ShaderStageFlags;
use khora_core::renderer::traits::{GpuProfiler, GraphicsBackendSelector};
use khora_core::renderer::{GraphicsDevice, RenderError};
use khora_core::telemetry::ResourceMonitor;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::dpi::PhysicalSize;

mod viewport;

mod overlay;

mod render_system;

/// The concrete, WGPU-based implementation of the [`RenderSystem`](khora_core::renderer::traits::RenderSystem) trait.
///
/// This struct encapsulates all the state necessary to drive rendering with WGPU,
/// including the graphics context, the logical device, GPU profiler, and complex
/// state for handling window resizing gracefully.
///
/// It acts as the primary rendering backend for the engine when the WGPU feature is enabled.
pub struct WgpuRenderSystem {
    graphics_context_shared: Option<Arc<Mutex<WgpuGraphicsContext>>>,
    wgpu_device: Option<Arc<WgpuDevice>>,
    gpu_monitor: Option<Arc<GpuMonitor>>,
    current_width: u32,
    current_height: u32,
    frame_count: u64,
    last_frame_stats: RenderStats,
    gpu_profiler: Option<Box<dyn GpuProfiler>>,
    current_frame_view_id: Option<TextureViewId>,

    // --- Camera Uniform Resources ---
    camera_uniform_buffer: Option<BufferId>,
    camera_bind_group: Option<BindGroupId>,
    camera_bind_group_layout: Option<BindGroupLayoutId>,

    // --- Depth Buffer Resources ---
    depth_texture: Option<TextureId>,
    depth_texture_view: Option<TextureViewId>,

    // --- Frame lifecycle ---
    /// Surface texture acquired by `begin_frame()`, consumed by `end_frame()`.
    active_frame_texture: Option<wgpu::SurfaceTexture>,

    // --- Resize Heuristics State ---
    last_resize_event: Option<Instant>,
    pending_resize: bool,
    last_surface_config: Option<Instant>,
    pending_resize_frames: u32,
    last_pending_size: Option<(u32, u32)>,
    stable_size_frame_count: u32,

    // --- Offscreen Viewport ---
    viewport_texture: Option<wgpu::Texture>,
    viewport_view: Option<wgpu::TextureView>,
    viewport_depth_texture: Option<wgpu::Texture>,
    viewport_depth_view: Option<wgpu::TextureView>,
    viewport_width: u32,
    viewport_height: u32,
    /// Registered abstract view IDs for the viewport (returned by `begin_frame` when
    /// `render_to_viewport == true`).
    viewport_color_view_id: Option<khora_core::renderer::api::resource::TextureViewId>,
    viewport_depth_view_id: Option<khora_core::renderer::api::resource::TextureViewId>,
    /// When true, `begin_frame` returns viewport targets instead of the swapchain
    /// and the engine skips its own present (caller manages the viewport).
    render_to_viewport: bool,
    // Grid + gizmo rendering moved to the engine-side `GridLane` /
    // `GizmoLane` (under `OverlayAgent`) — the backend owns no
    // render-strategy pipelines.
}

impl fmt::Debug for WgpuRenderSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgpuRenderSystem")
            .field("graphics_context_shared", &self.graphics_context_shared)
            .field("wgpu_device", &self.wgpu_device)
            .field("gpu_monitor", &self.gpu_monitor)
            .field("current_width", &self.current_width)
            .field("current_height", &self.current_height)
            .field("frame_count", &self.frame_count)
            .field("last_frame_stats", &self.last_frame_stats)
            .field(
                "gpu_profiler",
                &self.gpu_profiler.as_ref().map(|_| "GpuProfiler(...)"),
            )
            .field("current_frame_view_id", &self.current_frame_view_id)
            .field(
                "camera_uniform_buffer",
                &self.camera_uniform_buffer.as_ref().map(|_| "Buffer(...)"),
            )
            .field(
                "camera_bind_group",
                &self.camera_bind_group.as_ref().map(|_| "BindGroup(...)"),
            )
            .field(
                "camera_bind_group_layout",
                &self
                    .camera_bind_group_layout
                    .as_ref()
                    .map(|_| "BindGroupLayout(...)"),
            )
            .finish()
    }
}

impl Default for WgpuRenderSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderSystem {
    /// Creates a new, uninitialized `WgpuRenderSystem`.
    ///
    /// The system is not usable until [`RenderSystem::init`](khora_core::renderer::traits::RenderSystem::init) is called.
    pub fn new() -> Self {
        log::info!("WgpuRenderSystem created (uninitialized).");
        Self {
            graphics_context_shared: None,
            wgpu_device: None,
            gpu_monitor: None,
            current_width: 0,
            current_height: 0,
            frame_count: 0,
            last_frame_stats: RenderStats::default(),
            gpu_profiler: None,
            current_frame_view_id: None,
            camera_uniform_buffer: None,
            camera_bind_group: None,
            camera_bind_group_layout: None,
            depth_texture: None,
            depth_texture_view: None,
            active_frame_texture: None,
            last_resize_event: None,
            pending_resize: false,
            last_surface_config: None,
            pending_resize_frames: 0,
            last_pending_size: None,
            stable_size_frame_count: 0,
            viewport_texture: None,
            viewport_view: None,
            viewport_depth_texture: None,
            viewport_depth_view: None,
            viewport_width: 0,
            viewport_height: 0,
            viewport_color_view_id: None,
            viewport_depth_view_id: None,
            render_to_viewport: false,
        }
    }

    async fn initialize(
        &mut self,
        window_handle: KhoraWindowHandle,
        window_size: PhysicalSize<u32>,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError> {
        if self.graphics_context_shared.is_some() {
            return Err(RenderError::InitializationFailed(
                "WgpuRenderSystem is already initialized.".to_string(),
            ));
        }
        log::info!("WgpuRenderSystem: Initializing...");

        // wgpu 29 requires the display handle to be registered with the Instance up
        // front — surfaces created later are validated against it. Hand the window's
        // Arc directly; it implements `HasDisplayHandle + Debug + Send + Sync + 'static`,
        // which satisfies `wgpu::WgpuHasDisplayHandle`.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window_handle.clone()),
        ));
        let backend_selector = WgpuBackendSelector::new(instance.clone());
        let selection_config = BackendSelectionConfig::default();

        let selection_result = backend_selector
            .select_backend(&selection_config)
            .await
            .map_err(|e| RenderError::InitializationFailed(e.to_string()))?;
        let adapter = selection_result.adapter;

        let context = WgpuGraphicsContext::new(&instance, window_handle, adapter, window_size)
            .await
            .map_err(|e| RenderError::InitializationFailed(e.to_string()))?;

        self.current_width = context.get_size().0;
        self.current_height = context.get_size().1;
        let context_arc = Arc::new(Mutex::new(context));
        self.graphics_context_shared = Some(context_arc.clone());

        log::info!(
            "WgpuRenderSystem: GraphicsContext created with size: {}x{}",
            self.current_width,
            self.current_height
        );

        let graphics_device = WgpuDevice::new(context_arc.clone());
        let device_arc = Arc::new(graphics_device);
        self.wgpu_device = Some(device_arc.clone());

        if let Ok(gc_guard) = context_arc.lock() {
            if WgpuTimestampProfiler::feature_available(gc_guard.active_device_features) {
                if let Some(mut profiler) = WgpuTimestampProfiler::new(&gc_guard.device) {
                    let period = gc_guard.queue.get_timestamp_period();
                    profiler.set_timestamp_period(period);
                    self.gpu_profiler = Some(Box::new(profiler));
                }
            } else {
                log::info!("GPU timestamp feature not available; instrumentation disabled.");
            }
        }

        let mut created_monitors: Vec<Arc<dyn ResourceMonitor>> = Vec::new();
        let gpu_monitor = Arc::new(GpuMonitor::new("WGPU".to_string()));
        created_monitors.push(gpu_monitor.clone());
        self.gpu_monitor = Some(gpu_monitor);

        let vram_monitor = device_arc as Arc<dyn ResourceMonitor>;
        created_monitors.push(vram_monitor);

        // Initialize camera uniform resources
        self.initialize_camera_uniforms()?;

        // Initialize depth texture for depth buffering
        self.create_depth_texture()?;

        Ok(created_monitors)
    }

    /// Initializes the camera uniform buffer and bind group.
    ///
    /// This creates:
    /// - A uniform buffer to hold camera data (view-projection matrix and camera position)
    /// - A bind group layout describing the shader resource binding
    /// - A bind group that binds the buffer to group 0, binding 0
    fn initialize_camera_uniforms(&mut self) -> Result<(), RenderError> {
        use khora_core::renderer::api::command::{
            BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingResource, BindingType, BufferBinding, BufferBindingType,
        };
        use khora_core::renderer::api::resource::{
            BufferDescriptor, BufferUsage, CameraUniformData,
        };

        let device = self.wgpu_device.as_ref().ok_or_else(|| {
            RenderError::InitializationFailed("WGPU device not initialized".to_string())
        })?;

        let buffer_size = std::mem::size_of::<CameraUniformData>() as u64;

        // Create the uniform buffer using the abstract API
        let buffer_descriptor = BufferDescriptor {
            label: Some(std::borrow::Cow::Borrowed("Camera Uniform Buffer")),
            size: buffer_size,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        };

        let uniform_buffer = device.create_buffer(&buffer_descriptor).map_err(|e| {
            RenderError::InitializationFailed(format!(
                "Failed to create camera uniform buffer: {:?}",
                e
            ))
        })?;

        // Create the bind group layout using the abstract API
        let layout_entry = BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        };

        let layout_descriptor = BindGroupLayoutDescriptor {
            label: Some("Camera Bind Group Layout"),
            entries: &[layout_entry],
        };

        let bind_group_layout = device
            .create_bind_group_layout(&layout_descriptor)
            .map_err(|e| {
                RenderError::InitializationFailed(format!(
                    "Failed to create camera bind group layout: {:?}",
                    e
                ))
            })?;

        // Create the bind group using the abstract API
        let bind_group_entry = BindGroupEntry {
            binding: 0,
            resource: BindingResource::Buffer(BufferBinding {
                buffer: uniform_buffer,
                offset: 0,
                size: None,
            }),
            _phantom: std::marker::PhantomData,
        };

        let bind_group_descriptor = BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: bind_group_layout,
            entries: &[bind_group_entry],
        };

        let bind_group = device
            .create_bind_group(&bind_group_descriptor)
            .map_err(|e| {
                RenderError::InitializationFailed(format!(
                    "Failed to create camera bind group: {:?}",
                    e
                ))
            })?;

        self.camera_uniform_buffer = Some(uniform_buffer);
        self.camera_bind_group_layout = Some(bind_group_layout);
        self.camera_bind_group = Some(bind_group);

        log::info!("Camera uniform resources initialized with abstract API");

        Ok(())
    }

    /// Updates the camera uniform buffer with the current ViewInfo data.
    ///
    /// This method is called every frame to upload the latest camera matrices
    /// to the GPU uniform buffer.
    fn update_camera_uniforms(&mut self, view_info: &ViewInfo) {
        use khora_core::renderer::api::resource::CameraUniformData;

        let uniform_data = CameraUniformData::from_view_info(view_info);

        if let (Some(device), Some(buffer_id)) = (&self.wgpu_device, &self.camera_uniform_buffer) {
            // Write the uniform data to the buffer using the abstract API
            if let Err(e) =
                device.write_buffer(*buffer_id, 0, bytemuck::cast_slice(&[uniform_data]))
            {
                log::warn!("Failed to write camera uniform data: {:?}", e);
            }
        }
    }

    /// Creates or recreates the depth texture for depth buffering.
    ///
    /// This method should be called during initialization and whenever the window is resized.
    /// It destroys any existing depth texture resources before creating new ones.
    fn create_depth_texture(&mut self) -> Result<(), RenderError> {
        use khora_core::math::Extent3D;
        use std::borrow::Cow;

        let device = self.wgpu_device.as_ref().ok_or_else(|| {
            RenderError::InitializationFailed("WGPU device not initialized".to_string())
        })?;

        // Skip if dimensions are zero
        if self.current_width == 0 || self.current_height == 0 {
            return Ok(());
        }

        // Destroy old depth texture resources if they exist
        if let Some(old_view) = self.depth_texture_view.take() {
            let _ = device.destroy_texture_view(old_view);
        }
        if let Some(old_tex) = self.depth_texture.take() {
            let _ = device.destroy_texture(old_tex);
        }

        // Create new depth texture
        let texture_desc = TextureDescriptor {
            label: Some(Cow::Borrowed("Depth Texture")),
            size: Extent3D {
                width: self.current_width,
                height: self.current_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsage::RENDER_ATTACHMENT,
            view_formats: Cow::Borrowed(&[]),
        };

        let texture_id = device.create_texture(&texture_desc).map_err(|e| {
            RenderError::InitializationFailed(format!("Failed to create depth texture: {:?}", e))
        })?;

        // Create depth texture view
        let view_desc = TextureViewDescriptor {
            label: Some(Cow::Borrowed("Depth Texture View")),
            format: Some(TextureFormat::Depth32Float),
            dimension: None,
            aspect: ImageAspect::DepthOnly,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        };

        let view_id = device
            .create_texture_view(texture_id, &view_desc)
            .map_err(|e| {
                RenderError::InitializationFailed(format!(
                    "Failed to create depth texture view: {:?}",
                    e
                ))
            })?;

        self.depth_texture = Some(texture_id);
        self.depth_texture_view = Some(view_id);

        log::info!(
            "Depth texture created: {}x{} (Depth32Float)",
            self.current_width,
            self.current_height
        );

        Ok(())
    }

    /// Checks the device-health flags raised by the wgpu error callbacks.
    ///
    /// Returns a fatal, non-panicking [`RenderError`] when the device has been
    /// lost or has run out of memory so the host can tear down cleanly. The
    /// frame loop calls this before any GPU submission; once a fatal condition
    /// is observed the system stops acquiring/submitting work.
    fn check_device_health(&self) -> Result<(), RenderError> {
        let Some(device) = self.wgpu_device.as_ref() else {
            return Ok(());
        };
        if device.is_device_out_of_memory() {
            return Err(RenderError::DeviceOutOfMemory(
                "device reported out-of-memory via wgpu error callback".to_string(),
            ));
        }
        if device.is_device_lost() {
            return Err(RenderError::DeviceLost);
        }
        Ok(())
    }

    /// Acquires the swapchain texture with full resilience.
    ///
    /// Covers every [`wgpu::CurrentSurfaceTexture`] outcome via the pure
    /// [`classify_acquire`] policy:
    /// - `Success`/`Suboptimal` → return the texture.
    /// - `Lost`/`Outdated` with a valid size → reconfigure and retry in-frame.
    /// - `Lost`/`Outdated` at zero size, `Timeout`, `Occluded` → skip the frame
    ///   (returns `Ok(None)`), no error spam.
    /// - `Validation`/unknown → non-fatal [`RenderError::SurfaceAcquisitionFailed`].
    ///
    /// `Ok(None)` means "skip this frame, retry next frame"; the caller must
    /// not treat it as an error.
    fn acquire_surface_texture(
        &mut self,
        gc: &Arc<Mutex<WgpuGraphicsContext>>,
    ) -> Result<Option<wgpu::SurfaceTexture>, RenderError> {
        // A zero-size (minimized) window has no renderable surface. Skip the
        // frame silently rather than churning reconfigure/acquire every tick.
        if !surface_is_renderable(self.current_width, self.current_height) {
            log::debug!(
                "WgpuRenderSystem: surface not renderable ({}x{}); skipping frame.",
                self.current_width,
                self.current_height
            );
            return Ok(None);
        }

        // Bounded retry: at most one in-frame reconfigure for a lost/outdated
        // surface, then a single re-acquire. Avoids any unbounded spin.
        let max_attempts = 2;
        for attempt in 0..max_attempts {
            let mut gc_guard = gc
                .lock()
                .map_err(|_| RenderError::Internal("graphics context lock poisoned".into()))?;

            let (status, texture) = match gc_guard.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    (SurfaceAcquireStatus::Usable, Some(t))
                }
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                    (SurfaceAcquireStatus::LostOrOutdated, None)
                }
                wgpu::CurrentSurfaceTexture::Timeout => (SurfaceAcquireStatus::Timeout, None),
                wgpu::CurrentSurfaceTexture::Occluded => (SurfaceAcquireStatus::Occluded, None),
                wgpu::CurrentSurfaceTexture::Validation => (SurfaceAcquireStatus::Validation, None),
                // Forward-compat: should wgpu add a swapchain-status variant in
                // a future release, classify it as a non-fatal unknown hiccup
                // rather than failing to compile or panicking. Unreachable today
                // because the enum is currently exhaustive.
                #[allow(unreachable_patterns)]
                _ => (SurfaceAcquireStatus::Unknown, None),
            };

            let has_valid_size = surface_is_renderable(self.current_width, self.current_height);
            match classify_acquire(status, has_valid_size) {
                AcquireAction::Proceed => {
                    // `texture` is `Some` exactly for the `Usable` status.
                    return Ok(texture);
                }
                AcquireAction::ReconfigureAndRetry => {
                    log::warn!(
                        "WgpuRenderSystem: surface lost/outdated; reconfiguring to {}x{} (attempt {}).",
                        self.current_width,
                        self.current_height,
                        attempt + 1
                    );
                    gc_guard.resize(self.current_width, self.current_height);
                    drop(gc_guard);
                    self.last_surface_config = Some(Instant::now());
                    self.pending_resize = false;
                    // Loop to re-acquire on the next iteration.
                    continue;
                }
                AcquireAction::SkipFrame => {
                    log::debug!(
                        "WgpuRenderSystem: acquire status {:?} — skipping frame.",
                        status
                    );
                    return Ok(None);
                }
                AcquireAction::NonFatalError => {
                    log::error!(
                        "WgpuRenderSystem: non-fatal surface acquire failure ({:?}).",
                        status
                    );
                    return Err(RenderError::SurfaceAcquisitionFailed(format!("{status:?}")));
                }
            }
        }

        // Exhausted the in-frame reconfigure budget without a usable texture.
        // Skip this frame; the next frame retries from a freshly configured
        // surface rather than erroring out.
        log::warn!(
            "WgpuRenderSystem: surface still unavailable after {} acquire attempts; skipping frame.",
            max_attempts
        );
        Ok(None)
    }
}

unsafe impl Send for WgpuRenderSystem {}

unsafe impl Sync for WgpuRenderSystem {}
