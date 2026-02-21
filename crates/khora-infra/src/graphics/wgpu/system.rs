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

use super::backend::WgpuBackendSelector;
use super::context::WgpuGraphicsContext;
use super::device::WgpuDevice;
use super::profiler::WgpuTimestampProfiler;
use khora_core::math::LinearRgba;
use khora_core::platform::window::{KhoraWindow, KhoraWindowHandle};
use khora_core::renderer::api::command::{
    LoadOp, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::texture::TextureViewId;
use khora_core::renderer::traits::{GpuProfiler, GraphicsBackendSelector};
use khora_core::renderer::{
    BackendSelectionConfig, BindGroupId, BindGroupLayoutId, BufferId, GraphicsAdapterInfo,
    GraphicsDevice, IndexFormat, Operations, RenderError, RenderObject, RenderSettings,
    RenderStats, RenderSystem, ViewInfo,
};
use khora_core::telemetry::ResourceMonitor;
use khora_core::Stopwatch;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::dpi::PhysicalSize;

/// The concrete, WGPU-based implementation of the [`RenderSystem`] trait.
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
    depth_texture: Option<khora_core::renderer::TextureId>,
    depth_texture_view: Option<TextureViewId>,

    // --- Resize Heuristics State ---
    last_resize_event: Option<Instant>,
    pending_resize: bool,
    last_surface_config: Option<Instant>,
    pending_resize_frames: u32,
    last_pending_size: Option<(u32, u32)>,
    stable_size_frame_count: u32,
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
    /// The system is not usable until [`RenderSystem::init`] is called.
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
            last_resize_event: None,
            pending_resize: false,
            last_surface_config: None,
            pending_resize_frames: 0,
            last_pending_size: None,
            stable_size_frame_count: 0,
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

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
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
        use khora_core::renderer::{
            BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingResource, BindingType, BufferBinding, BufferBindingType, BufferDescriptor,
            BufferUsage, CameraUniformData, ShaderStageFlags,
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
        use khora_core::renderer::CameraUniformData;

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
        use khora_core::renderer::{
            ImageAspect, SampleCount, TextureDescriptor, TextureDimension, TextureFormat,
            TextureUsage, TextureViewDescriptor,
        };
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
}

impl RenderSystem for WgpuRenderSystem {
    fn init(
        &mut self,
        window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError> {
        let (width, height) = window.inner_size();
        let window_size = PhysicalSize::new(width, height);
        let window_handle_arc = window.clone_handle_arc();
        pollster::block_on(self.initialize(window_handle_arc, window_size))
    }

    fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width > 0 && new_height > 0 {
            log::debug!(
                "WgpuRenderSystem: resize_surface called with W:{new_width}, H:{new_height}"
            );
            self.current_width = new_width;
            self.current_height = new_height;
            let now = Instant::now();
            if let Some((lw, lh)) = self.last_pending_size {
                if lw == new_width && lh == new_height {
                    self.stable_size_frame_count = self.stable_size_frame_count.saturating_add(1);
                } else {
                    self.stable_size_frame_count = 0;
                }
            }
            self.last_pending_size = Some((new_width, new_height));

            let immediate_threshold_ms: u128 = 80;
            let can_immediate = self
                .last_surface_config
                .map(|t| t.elapsed().as_millis() >= immediate_threshold_ms)
                .unwrap_or(true);
            let early_stable = self.stable_size_frame_count >= 2
                && self
                    .last_surface_config
                    .map(|t| t.elapsed().as_millis() >= 20)
                    .unwrap_or(true);
            if can_immediate || early_stable {
                let mut did_resize = false;
                if let Some(gc_arc_mutex) = &self.graphics_context_shared {
                    if let Ok(mut gc_guard) = gc_arc_mutex.lock() {
                        gc_guard.resize(self.current_width, self.current_height);
                        self.last_surface_config = Some(now);
                        self.pending_resize = false;
                        self.pending_resize_frames = 0;
                        did_resize = true;
                    }
                }
                if did_resize {
                    // Recreate depth texture to match new size (after lock is released)
                    if let Err(e) = self.create_depth_texture() {
                        log::warn!("Failed to recreate depth texture during resize: {:?}", e);
                    }
                    log::info!(
                        "WGPUGraphicsContext: Immediate/Early surface configuration to {}x{}",
                        self.current_width,
                        self.current_height
                    );
                    return;
                }
            }
            self.last_resize_event = Some(now);
            self.pending_resize = true;
            self.pending_resize_frames = 0;
        } else {
            log::warn!(
                "WgpuRenderSystem::resize_surface called with zero size ({new_width}, {new_height}). Ignoring."
            );
        }
    }

    fn prepare_frame(&mut self, view_info: &ViewInfo) {
        if self.graphics_context_shared.is_none() {
            return;
        }
        let stopwatch = Stopwatch::new();

        // Update camera uniform buffer with the current ViewInfo
        self.update_camera_uniforms(view_info);

        self.last_frame_stats.cpu_preparation_time_ms = stopwatch.elapsed_ms().unwrap_or(0) as f32;
    }

    fn render(
        &mut self,
        renderables: &[RenderObject],
        _view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError> {
        let full_frame_timer = Stopwatch::new();

        let device = self
            .wgpu_device
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        // Poll the device to process any pending GPU-to-CPU callbacks, such as
        // those from the profiler's `map_async` calls. This is crucial.
        device.poll_device_non_blocking();

        let gc = self
            .graphics_context_shared
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        if let Some(p) = self.gpu_profiler.as_mut() {
            p.try_read_previous_frame();
        }

        // --- Handle Pending Resizes ---
        let mut resized_this_frame = false;
        if self.pending_resize {
            self.pending_resize_frames = self.pending_resize_frames.saturating_add(1);
            if let Some(t) = self.last_resize_event {
                let quiet_elapsed = t.elapsed().as_millis();
                let debounce_quiet_ms = settings.resize_debounce_ms as u128;
                let max_pending_frames = settings.resize_max_pending_frames;
                let early_stable = self.stable_size_frame_count >= 3;

                if quiet_elapsed >= debounce_quiet_ms
                    || self.pending_resize_frames >= max_pending_frames
                    || early_stable
                {
                    if let Ok(mut gc_guard) = gc.lock() {
                        gc_guard.resize(self.current_width, self.current_height);
                        self.pending_resize = false;
                        self.last_surface_config = Some(Instant::now());
                        self.stable_size_frame_count = 0;
                        resized_this_frame = true;
                        log::info!(
                            "Deferred surface configuration to {}x{}",
                            self.current_width,
                            self.current_height
                        );
                    }
                }
            }
            if self.pending_resize && !resized_this_frame {
                return Ok(self.last_frame_stats.clone());
            }
        }

        // Recreate depth texture if we just resized
        if resized_this_frame {
            if let Err(e) = self.create_depth_texture() {
                log::warn!(
                    "Failed to recreate depth texture during deferred resize: {:?}",
                    e
                );
            }
        }

        // --- 1. Acquire Frame from Swap Chain ---
        let output_surface_texture = loop {
            let mut gc_guard = gc.lock().unwrap();
            match gc_guard.get_current_texture() {
                Ok(texture) => break texture,
                Err(e @ wgpu::SurfaceError::Lost) | Err(e @ wgpu::SurfaceError::Outdated) => {
                    if self.current_width > 0 && self.current_height > 0 {
                        log::warn!(
                            "WgpuRenderSystem: Swapchain surface lost or outdated ({:?}). Reconfiguring with current dimensions: W={}, H={}",
                            e,
                            self.current_width,
                            self.current_height
                        );
                        gc_guard.resize(self.current_width, self.current_height);
                        self.last_surface_config = Some(Instant::now());
                        self.pending_resize = false; // reset pending state after forced reconfigure
                    } else {
                        log::error!(
                            "WgpuRenderSystem: Swapchain lost/outdated ({:?}), but current stored size is zero ({},{}). Cannot reconfigure. Waiting for valid resize event.",
                            e,
                            self.current_width,
                            self.current_height
                        );
                        return Err(RenderError::SurfaceAcquisitionFailed(format!(
                            "Surface Lost/Outdated ({e:?}) and current size is zero",
                        )));
                    }
                }
                Err(e @ wgpu::SurfaceError::OutOfMemory) => {
                    log::error!("WgpuRenderSystem: Swapchain OutOfMemory! ({e:?})");
                    return Err(RenderError::SurfaceAcquisitionFailed(format!(
                        "OutOfMemory: {e:?}"
                    )));
                }
                Err(e @ wgpu::SurfaceError::Timeout) => {
                    log::warn!("WgpuRenderSystem: Swapchain Timeout acquiring frame. ({e:?})");
                    return Err(RenderError::SurfaceAcquisitionFailed(format!(
                        "Timeout: {e:?}"
                    )));
                }
                Err(e) => {
                    log::error!("WgpuRenderSystem: Unexpected SurfaceError: {e:?}");
                    return Err(RenderError::SurfaceAcquisitionFailed(format!(
                        "Unexpected SurfaceError: {e:?}"
                    )));
                }
            }
        };

        let command_recording_timer = Stopwatch::new();

        // --- 2. Create a managed, abstract view for the swap chain texture ---
        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.create_texture_view_for_surface(
            &output_surface_texture.texture,
            Some("Primary Swap Chain View"),
        )?;
        self.current_frame_view_id = Some(target_view_id);

        // --- 3. Create an abstract Command Encoder ---
        let mut command_encoder = device.create_command_encoder(Some("Khora Main Command Encoder"));

        // --- 4. Profiler Pass A (records start timestamps) ---
        if settings.enable_gpu_timestamps {
            if let Some(profiler) = self.gpu_profiler.as_ref() {
                let _pass_a = command_encoder.begin_profiler_compute_pass(
                    Some("Timestamp Pass A"),
                    profiler.as_ref(),
                    0,
                );
            }
        }

        // --- 5. Main Render Pass (drawing all objects) ---
        {
            let gc_guard = gc.lock().unwrap();
            let wgpu_color = gc_guard.get_clear_color();
            let clear_color = LinearRgba::new(
                wgpu_color.r as f32,
                wgpu_color.g as f32,
                wgpu_color.b as f32,
                wgpu_color.a as f32,
            );

            let color_attachment = RenderPassColorAttachment {
                view: &target_view_id,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(clear_color),
                    store: StoreOp::Store,
                },
            };

            // Create depth/stencil attachment if depth texture is available
            use khora_core::renderer::api::command::RenderPassDepthStencilAttachment;
            let depth_attachment = self.depth_texture_view.as_ref().map(|depth_view| {
                RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0), // Clear to far plane (1.0)
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None, // No stencil operations
                }
            });

            let pass_descriptor = RenderPassDescriptor {
                label: Some("Khora Main Abstract Render Pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: depth_attachment,
            };

            let mut render_pass = command_encoder.begin_render_pass(&pass_descriptor);

            // Apply the same bind group and pipeline to all chunks to limit state changes
            if let Some(camera_bind_group) = &self.camera_bind_group {
                render_pass.set_bind_group(0, camera_bind_group, &[]);
            }
            let (draw_calls, triangles) = renderables.iter().fold((0, 0), |(dc, tris), obj| {
                render_pass.set_pipeline(&obj.pipeline);
                render_pass.set_vertex_buffer(0, &obj.vertex_buffer, 0);
                render_pass.set_index_buffer(&obj.index_buffer, 0, IndexFormat::Uint16);
                render_pass.draw_indexed(0..obj.index_count, 0, 0..1);
                (dc + 1, tris + obj.index_count / 3)
            });
            self.last_frame_stats.draw_calls = draw_calls;
            self.last_frame_stats.triangles_rendered = triangles;
        }

        // --- 6. Profiler Pass B and Timestamp Resolution ---
        if settings.enable_gpu_timestamps {
            if let Some(profiler) = self.gpu_profiler.as_ref() {
                // This scope ensures the compute pass ends, releasing its mutable borrow on the encoder,
                // before we try to mutably borrow the encoder again for resolve/copy.
                {
                    let _pass_b = command_encoder.begin_profiler_compute_pass(
                        Some("Timestamp Pass B"),
                        profiler.as_ref(),
                        1,
                    );
                }
                profiler.resolve_and_copy(command_encoder.as_mut());
                profiler.copy_to_staging(command_encoder.as_mut(), self.frame_count);
            }
        }

        // --- 7. Finalize and Submit Commands ---
        let submission_timer = Stopwatch::new();
        let command_buffer = command_encoder.finish();
        device.submit_command_buffer(command_buffer);
        let submission_ms = submission_timer.elapsed_ms().unwrap_or(0);

        if settings.enable_gpu_timestamps {
            if let Some(p) = self.gpu_profiler.as_mut() {
                p.schedule_map_after_submit(self.frame_count);
            }
        }

        // --- 8. Present the final image to the screen ---
        output_surface_texture.present();

        // --- 9. Update final frame statistics ---
        self.frame_count += 1;
        if let Some(p) = self.gpu_profiler.as_ref() {
            self.last_frame_stats.gpu_main_pass_time_ms = p.last_main_pass_ms();
            self.last_frame_stats.gpu_frame_total_time_ms = p.last_frame_total_ms();
        }
        let full_frame_ms = full_frame_timer.elapsed_ms().unwrap_or(0);
        self.last_frame_stats.frame_number = self.frame_count;
        self.last_frame_stats.cpu_preparation_time_ms =
            (full_frame_ms - command_recording_timer.elapsed_ms().unwrap_or(0)) as f32;
        self.last_frame_stats.cpu_render_submission_time_ms = submission_ms as f32;

        if let Some(monitor) = &self.gpu_monitor {
            monitor.update_from_frame_stats(&self.last_frame_stats);
        }

        Ok(self.last_frame_stats.clone())
    }

    fn render_with_encoder(
        &mut self,
        clear_color: khora_core::math::LinearRgba,
        encoder_fn: Box<
            dyn FnOnce(
                    &mut dyn khora_core::renderer::traits::CommandEncoder,
                    &khora_core::renderer::RenderContext,
                ) + Send
                + '_,
        >,
    ) -> Result<RenderStats, RenderError> {
        use khora_core::renderer::RenderContext;

        let full_frame_timer = Stopwatch::new();

        let device = self
            .wgpu_device
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        device.poll_device_non_blocking();

        let gc = self
            .graphics_context_shared
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        if let Some(p) = self.gpu_profiler.as_mut() {
            p.try_read_previous_frame();
        }

        // --- Handle Pending Resizes ---
        let mut resized_this_frame = false;
        if self.pending_resize {
            self.pending_resize_frames = self.pending_resize_frames.saturating_add(1);
            if let Some(t) = self.last_resize_event {
                let quiet_elapsed = t.elapsed().as_millis();
                let debounce_quiet_ms = 120u128;
                let max_pending_frames = 10u32;
                let early_stable = self.stable_size_frame_count >= 3;

                if quiet_elapsed >= debounce_quiet_ms
                    || self.pending_resize_frames >= max_pending_frames
                    || early_stable
                {
                    if let Ok(mut gc_guard) = gc.lock() {
                        gc_guard.resize(self.current_width, self.current_height);
                        self.pending_resize = false;
                        self.last_surface_config = Some(Instant::now());
                        self.stable_size_frame_count = 0;
                        resized_this_frame = true;
                    }
                }
            }
            if self.pending_resize && !resized_this_frame {
                return Ok(self.last_frame_stats.clone());
            }
        }

        if resized_this_frame {
            if let Err(e) = self.create_depth_texture() {
                log::warn!("Failed to recreate depth texture: {:?}", e);
            }
        }

        // --- Acquire Frame ---
        let output_surface_texture = loop {
            let mut gc_guard = gc.lock().unwrap();
            match gc_guard.get_current_texture() {
                Ok(texture) => break texture,
                Err(e) => {
                    if self.current_width > 0 && self.current_height > 0 {
                        gc_guard.resize(self.current_width, self.current_height);
                        continue;
                    }
                    return Err(RenderError::SurfaceAcquisitionFailed(format!("{:?}", e)));
                }
            }
        };

        let command_recording_timer = Stopwatch::new();

        // --- Create texture view ---
        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.create_texture_view_for_surface(
            &output_surface_texture.texture,
            Some("Primary Swap Chain View"),
        )?;
        self.current_frame_view_id = Some(target_view_id);

        // --- Create encoder ---
        let mut command_encoder = device.create_command_encoder(Some("Khora Main Command Encoder"));

        // --- Build RenderContext with actual target ---
        let actual_render_ctx = RenderContext {
            color_target: &target_view_id,
            depth_target: self.depth_texture_view.as_ref(),
            clear_color,
        };

        // --- Call the encoder function (agents do their rendering here) ---
        encoder_fn(command_encoder.as_mut(), &actual_render_ctx);

        // --- Submit ---
        let submission_timer = Stopwatch::new();
        let command_buffer = command_encoder.finish();
        device.submit_command_buffer(command_buffer);
        let submission_ms = submission_timer.elapsed_ms().unwrap_or(0);

        // --- Present ---
        output_surface_texture.present();

        // --- Update stats ---
        self.frame_count += 1;
        let full_frame_ms = full_frame_timer.elapsed_ms().unwrap_or(0);
        self.last_frame_stats.frame_number = self.frame_count;
        self.last_frame_stats.cpu_preparation_time_ms =
            (full_frame_ms - command_recording_timer.elapsed_ms().unwrap_or(0)) as f32;
        self.last_frame_stats.cpu_render_submission_time_ms = submission_ms as f32;

        if let Some(monitor) = &self.gpu_monitor {
            monitor.update_from_frame_stats(&self.last_frame_stats);
        }

        Ok(self.last_frame_stats.clone())
    }

    fn get_last_frame_stats(&self) -> &RenderStats {
        &self.last_frame_stats
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        self.wgpu_device
            .as_ref()
            .is_some_and(|d| d.supports_feature(feature_name))
    }

    fn shutdown(&mut self) {
        log::info!("WgpuRenderSystem shutting down...");
        if let Some(mut profiler) = self.gpu_profiler.take() {
            if let Some(device) = self.wgpu_device.as_ref() {
                if let Some(wgpu_profiler) = profiler
                    .as_any_mut()
                    .downcast_mut::<WgpuTimestampProfiler>()
                {
                    wgpu_profiler.shutdown(device);
                }
            }
        }
        if let Some(old_id) = self.current_frame_view_id.take() {
            if let Some(device) = self.wgpu_device.as_ref() {
                let _ = device.destroy_texture_view(old_id);
            }
        }
        self.wgpu_device = None;
        self.graphics_context_shared = None;
        self.gpu_monitor = None;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo> {
        self.wgpu_device.as_ref().map(|d| d.get_adapter_info())
    }

    fn graphics_device(&self) -> Arc<dyn GraphicsDevice> {
        self.wgpu_device
            .clone()
            .expect("WgpuRenderSystem: No WgpuDevice available.")
    }
}

unsafe impl Send for WgpuRenderSystem {}
unsafe impl Sync for WgpuRenderSystem {}
