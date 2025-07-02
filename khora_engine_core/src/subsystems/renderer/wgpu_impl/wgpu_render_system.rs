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

use std::sync::{Arc, Mutex};

use crate::subsystems::renderer::{
    RenderError, RenderObject, RenderSettings, RenderStats, ViewInfo,
};
use crate::{core::timer::Stopwatch, window::KhoraWindow};

use super::wgpu_device::WgpuDevice;
use super::wgpu_graphic_context::WgpuGraphicsContext;
use crate::subsystems::renderer::api::common_types::RendererAdapterInfo;
use crate::subsystems::renderer::traits::graphics_device::GraphicsDevice;
use crate::subsystems::renderer::traits::render_system::RenderSystem;

#[derive(Debug)]
pub struct WgpuRenderSystem {
    graphics_context_shared: Option<Arc<Mutex<WgpuGraphicsContext>>>,
    wgpu_device: Option<Arc<WgpuDevice>>,
    current_width: u32,
    current_height: u32,
    frame_count: u64,
    last_frame_stats: RenderStats,
}

impl Default for WgpuRenderSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderSystem {
    /// Create a new WgpuRenderSystem instance.
    /// This function initializes the renderer with default values.
    pub fn new() -> Self {
        log::info!("WgpuRenderSystem created (uninitialized).");
        Self {
            graphics_context_shared: None,
            wgpu_device: None,
            current_width: 0,
            current_height: 0,
            frame_count: 0,
            last_frame_stats: RenderStats::default(),
        }
    }
}

// To make the WgpuRenderSystem thread-safe, we need to implement the Send and Sync traits.
// This allows the WgpuRenderSystem to be safely shared between threads. (To be able to use Box<dyn RenderSystem>)
unsafe impl Send for WgpuRenderSystem {}
unsafe impl Sync for WgpuRenderSystem {}

impl RenderSystem for WgpuRenderSystem {
    fn init(&mut self, window: &KhoraWindow) -> Result<(), RenderError> {
        if self.graphics_context_shared.is_some() {
            log::warn!("WgpuRenderSystem::init called but it's already initialized.");
            return Ok(());
        }
        log::info!("WgpuRenderSystem: Initializing internal GraphicsContext...");

        // Create a new GraphicsContext instance.
        match WgpuGraphicsContext::new(window) {
            Ok(context) => {
                log::info!("WgpuRenderSystem: Internal GraphicsContext initialized successfully.");

                let initial_size = window.inner_size();
                self.current_width = initial_size.0.max(1);
                self.current_height = initial_size.1.max(1);
                self.graphics_context_shared = Some(Arc::new(Mutex::new(context)));

                log::info!(
                    "WgpuRenderSystem: GraphicsContext created with size: {}x{}",
                    self.current_width,
                    self.current_height
                );

                // Initialize the graphics device
                let graphics_device =
                    WgpuDevice::new(self.graphics_context_shared.clone().unwrap());
                self.wgpu_device = Some(Arc::new(graphics_device));

                log::info!(
                    "WgpuRenderSystem: GraphicsDevice initialized with adapter: {}",
                    self.wgpu_device.as_ref().unwrap().get_adapter_info().name
                );

                Ok(())
            }
            Err(e) => {
                log::error!(
                    "WgpuRenderSystem: Failed to initialize internal GraphicsContext: {e}"
                );
                Err(RenderError::InitializationFailed(format!(
                    "GraphicsContext creation error: {e}"
                )))
            }
        }
    }

    fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width > 0 && new_height > 0 {
            log::debug!(
                "WgpuRenderSystem: resize_surface called with W:{new_width}, H:{new_height}"
            );
            self.current_width = new_width;
            self.current_height = new_height;

            // Check if the graphics context is initialized before resizing (need to go through the mutex)
            if let Some(gc_arc_mutex) = &self.graphics_context_shared {
                match gc_arc_mutex.lock() {
                    Ok(mut gc_guard) => {
                        gc_guard.resize(new_width, new_height);
                    }
                    Err(e) => {
                        log::error!(
                            "WgpuRenderSystem::resize_surface: Failed to lock GraphicsContext: {e}"
                        );
                    }
                }
            } else {
                log::warn!("WgpuRenderSystem::resize_surface called but GraphicsContext is None.");
            }
        } else {
            log::warn!(
                "WgpuRenderSystem::resize_surface called with zero size ({new_width}, {new_height}). Ignoring."
            );
        }
    }

    fn prepare_frame(&mut self, _view_info: &ViewInfo) {
        if self.graphics_context_shared.is_none() {
            log::trace!("WgpuRenderSystem::prepare_frame skipped, not initialized.");
            return;
        }

        let stopwatch = Stopwatch::new();

        self.last_frame_stats.cpu_preparation_time_ms = stopwatch.elapsed_ms().unwrap_or(0) as f32;
    }

    fn render(
        &mut self,
        renderables: &[RenderObject],
        _view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError> {
        let total_cpu_prep_timer = Stopwatch::new();

        // Get the graphics context
        let gc = self.graphics_context_shared.as_ref().ok_or_else(|| {
            RenderError::InitializationFailed(
                "GraphicsContext Arc<Mutex> is None in WgpuRenderSystem::render".to_string(),
            )
        })?;

        // --- 1. Acquire Frame ---
        let output_surface_texture = loop {
            // Lock the GraphicsContext mutex to access the surface
            let mut gc_guard = gc.lock().map_err(|e| {
                RenderError::Internal(format!(
                    "Render: Failed to lock GraphicsContext Mutex for get_current_texture: {e}"
                ))
            })?;

            match gc_guard.get_current_texture() {
                // gc_guard deferences itself in GraphicsContext
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
                    log::warn!(
                        "WgpuRenderSystem: Swapchain Timeout acquiring frame. ({e:?})"
                    );
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
            // Free the lock before the next iteration
        };

        let cpu_render_logic_timer = Stopwatch::new();

        // 2. Create the view
        let target_texture_view = output_surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        log::trace!(
            "WgpuRenderSystem::render_to_window frame {}, {} objects. Strategy: {:?}, Quality: {}",
            self.frame_count,
            renderables.len(),
            settings.strategy,
            settings.quality_level
        );

        // New scope to minimize the lock duration
        let cpu_submission_timer;
        let cpu_submission_duration_ms;

        let mut _actual_draw_calls = 0;
        let mut _actual_triangles = 0;

        {
            let gc_guard = gc.lock().map_err(|e| {
                RenderError::Internal(format!(
                    "Render: Failed to lock GraphicsContext Mutex for render pass: {e}"
                ))
            })?;

            // --- 3. Create Command Encoder
            let mut encoder =
                gc_guard
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("WgpuRenderSystem Render Command Encoder"),
                    });

            // --- 4. Begin Render Pass
            {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("WgpuRenderSystem Clear Screen Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target_texture_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(gc_guard.get_clear_color()),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None, // TODO: Add depth buffer
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });
                // TODO: Draw the renderables here
            }

            // --- 5. Submit Commands ---
            cpu_submission_timer = Stopwatch::new();
            gc_guard.queue().submit(std::iter::once(encoder.finish()));
            cpu_submission_duration_ms =
                cpu_submission_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;
        }

        let cpu_render_logic_duration_ms =
            cpu_render_logic_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

        // --- 6. Present Frame ---
        output_surface_texture.present();

        let total_cpu_prep_duration_ms =
            total_cpu_prep_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

        self.frame_count += 1;
        self.last_frame_stats = RenderStats {
            frame_number: self.frame_count,
            cpu_preparation_time_ms: total_cpu_prep_duration_ms as f32
                - cpu_render_logic_duration_ms as f32,
            cpu_render_submission_time_ms: cpu_submission_duration_ms as f32,
            gpu_time_ms: 0.0, // TODO: Implement GPU time measurement
            draw_calls: _actual_draw_calls,
            triangles_rendered: _actual_triangles,
            vram_usage_estimate_mb: 0.0, // TODO: Implement VRAM usage estimation
        };

        Ok(self.last_frame_stats.clone())
    }

    fn get_last_frame_stats(&self) -> &RenderStats {
        &self.last_frame_stats
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        self.graphics_device().supports_feature(feature_name)
    }

    fn shutdown(&mut self) {
        log::info!("WgpuRenderSystem shutting down internal WGPUGraphicsContext...");
        // Drop wgpu_device first, which holds a clone of the context Arc
        if let Some(device_arc) = self.wgpu_device.take() {
            log::debug!(
                "WgpuDevice Arc count before drop: {}",
                Arc::strong_count(&device_arc)
            );
        }

        if let Some(gc_arc_mutex) = self.graphics_context_shared.take() {
            match Arc::try_unwrap(gc_arc_mutex) {
                Ok(mutex_gc) => match mutex_gc.into_inner() {
                    Ok(_gc_instance) => {
                        log::info!(
                            "WGPUGraphicsContext successfully unwrapped and will be dropped."
                        );
                    }
                    Err(poisoned_err) => {
                        log::error!(
                            "Mutex was poisoned during shutdown: {poisoned_err}. Resources might not be fully cleaned."
                        );
                    }
                },
                Err(_still_shared_arc) => {
                    log::warn!(
                        "WGPUGraphicsContext Arc is still shared elsewhere during shutdown. Resources will be dropped when last Arc reference is gone."
                    );
                }
            }
        }
        self.graphics_context_shared = None;
        self.wgpu_device = None;
        log::info!("WgpuRenderSystem shutdown complete.");
    }

    fn get_adapter_info(&self) -> Option<RendererAdapterInfo> {
        Some(self.graphics_device().get_adapter_info())
    }

    fn graphics_device(&self) -> &dyn GraphicsDevice {
        self.wgpu_device
            .as_ref()
            .expect("WgpuRenderSystem: No WgpuDevice available.")
            .as_ref()
    }
}
