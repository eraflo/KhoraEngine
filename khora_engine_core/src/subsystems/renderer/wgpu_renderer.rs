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

use crate::{core::timer::Stopwatch, window::KhoraWindow};

use super::{
    api::{
        RenderObject, RenderSettings, RenderStats, RenderSystem, RenderSystemError,
        RendererAdapterInfo, RendererBackendType, RendererDeviceType, ViewInfo,
    },
    graphic_context::GraphicsContext,
};

#[derive(Debug)]
pub struct WgpuRenderer {
    graphics_context: Option<Arc<Mutex<GraphicsContext>>>,
    current_width: u32,
    current_height: u32,
    frame_count: u64,
    last_frame_stats: RenderStats,
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderer {
    /// Create a new WgpuRenderer instance.
    /// This function initializes the renderer with default values.
    pub fn new() -> Self {
        log::info!("WgpuRenderer created (uninitialized).");
        Self {
            graphics_context: None,
            current_width: 0,
            current_height: 0,
            frame_count: 0,
            last_frame_stats: RenderStats::default(),
        }
    }
}

// To make the WgpuRenderer thread-safe, we need to implement the Send and Sync traits.
// This allows the WgpuRenderer to be safely shared between threads. (To be able to use Box<dyn RenderSystem>)
unsafe impl Send for WgpuRenderer {}
unsafe impl Sync for WgpuRenderer {}

impl RenderSystem for WgpuRenderer {
    fn init(&mut self, window: &KhoraWindow) -> Result<(), RenderSystemError> {
        if self.graphics_context.is_some() {
            log::warn!("WgpuRenderer::init called but it's already initialized.");
            return Ok(());
        }
        log::info!("WgpuRenderer: Initializing internal GraphicsContext...");

        // Create a new GraphicsContext instance.
        match GraphicsContext::new(window) {
            Ok(context) => {
                log::info!("WgpuRenderer: Internal GraphicsContext initialized successfully.");
                log::info!(
                    "WgpuRenderer: Initialized with adapter: {}, backend: {:?}, features: {:?}, limits: {:?}",
                    context.adapter_name,
                    context.adapter_backend,
                    context.active_device_features,
                    context.device_limits
                );
                let initial_size = window.inner_size();
                self.current_width = initial_size.0;
                self.current_height = initial_size.1;
                self.graphics_context = Some(Arc::new(Mutex::new(context)));
                Ok(())
            }
            Err(e) => {
                log::error!(
                    "WgpuRenderer: Failed to initialize internal GraphicsContext: {}",
                    e
                );
                Err(RenderSystemError::InitializationFailed(format!(
                    "GraphicsContext creation error: {}",
                    e
                )))
            }
        }
    }

    fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width > 0 && new_height > 0 {
            log::debug!(
                "WgpuRenderer: resize_surface called with W:{}, H:{}",
                new_width,
                new_height
            );
            self.current_width = new_width;
            self.current_height = new_height;

            // Check if the graphics context is initialized before resizing (need to go through the mutex)
            if let Some(gc_arc_mutex) = &self.graphics_context {
                match gc_arc_mutex.lock() {
                    Ok(mut gc_guard) => {
                        gc_guard.resize(new_width, new_height);
                    }
                    Err(e) => {
                        log::error!(
                            "WgpuRenderer::resize_surface: Failed to lock GraphicsContext: {}",
                            e
                        );
                    }
                }
            } else {
                log::warn!("WgpuRenderer::resize_surface called but GraphicsContext is None.");
            }
        } else {
            log::warn!(
                "WgpuRenderer::resize_surface called with zero size ({}, {}). Ignoring.",
                new_width,
                new_height
            );
        }
    }

    fn prepare_frame(&mut self, _view_info: &ViewInfo) {
        if self.graphics_context.is_none() {
            log::trace!("WgpuRenderer::prepare_frame skipped, not initialized.");
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
    ) -> Result<RenderStats, RenderSystemError> {
        let total_cpu_prep_timer = Stopwatch::new();

        // Get the graphics context
        let gc = self.graphics_context.as_ref().ok_or_else(|| {
            RenderSystemError::InitializationFailed(
                "GraphicsContext Arc<Mutex> is None in WgpuRenderer::render".to_string(),
            )
        })?;

        // --- 1. Acquire Frame ---
        let output_surface_texture = loop {
            // Lock the GraphicsContext mutex to access the surface
            let mut gc_guard = gc.lock().map_err(|e| {
                RenderSystemError::Internal(format!(
                    "Render: Failed to lock GraphicsContext Mutex for get_current_texture: {}",
                    e
                ))
            })?;

            match gc_guard.get_current_texture() {
                // gc_guard deferences itself in GraphicsContext
                Ok(texture) => break texture,
                Err(e @ wgpu::SurfaceError::Lost) | Err(e @ wgpu::SurfaceError::Outdated) => {
                    if self.current_width > 0 && self.current_height > 0 {
                        log::warn!(
                            "WgpuRenderer: Swapchain surface lost or outdated ({:?}). Reconfiguring with current dimensions: W={}, H={}",
                            e,
                            self.current_width,
                            self.current_height
                        );
                        gc_guard.resize(self.current_width, self.current_height);
                    } else {
                        log::error!(
                            "WgpuRenderer: Swapchain lost/outdated ({:?}), but current stored size is zero ({},{}). Cannot reconfigure. Waiting for valid resize event.",
                            e,
                            self.current_width,
                            self.current_height
                        );
                        return Err(RenderSystemError::SurfaceAcquisitionFailed(format!(
                            "Surface Lost/Outdated ({:?}) and current size is zero",
                            e
                        )));
                    }
                }
                Err(e @ wgpu::SurfaceError::OutOfMemory) => {
                    log::error!("WgpuRenderer: Swapchain OutOfMemory! ({:?})", e);
                    return Err(RenderSystemError::SurfaceAcquisitionFailed(format!(
                        "OutOfMemory: {:?}",
                        e
                    )));
                }
                Err(e @ wgpu::SurfaceError::Timeout) => {
                    log::warn!("WgpuRenderer: Swapchain Timeout acquiring frame. ({:?})", e);
                    return Err(RenderSystemError::SurfaceAcquisitionFailed(format!(
                        "Timeout: {:?}",
                        e
                    )));
                }
                Err(e) => {
                    log::error!("WgpuRenderer: Unexpected SurfaceError: {:?}", e);
                    return Err(RenderSystemError::SurfaceAcquisitionFailed(format!(
                        "Unexpected SurfaceError: {:?}",
                        e
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
            "WgpuRenderer::render_to_window frame {}, {} objects. Strategy: {:?}, Quality: {}",
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
                RenderSystemError::Internal(format!(
                    "Render: Failed to lock GraphicsContext Mutex for render pass: {}",
                    e
                ))
            })?;

            // --- 3. Create Command Encoder
            let mut encoder =
                gc_guard
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("WgpuRenderer Render Command Encoder"),
                    });

            // --- 4. Begin Render Pass
            {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("WgpuRenderer Clear Screen Pass"),
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
        if let Some(gc_arc_mutex) = &self.graphics_context {
            match gc_arc_mutex.lock() {
                Ok(gc_guard) => match feature_name {
                    "gpu_timestamps" => gc_guard
                        .active_device_features
                        .contains(wgpu::Features::TIMESTAMP_QUERY),
                    "texture_compression_bc" => gc_guard
                        .active_device_features
                        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
                    _ => {
                        log::warn!(
                            "Unsupported feature_name query in supports_feature: {}",
                            feature_name
                        );
                        false
                    }
                },
                Err(e) => {
                    log::error!(
                        "Failed to lock GraphicsContext to check feature '{}': {}. Assuming feature not supported.",
                        feature_name,
                        e
                    );
                    false
                }
            }
        } else {
            log::warn!(
                "Attempted to check feature '{}' but graphics context is not initialized.",
                feature_name
            );
            false
        }
    }

    fn shutdown(&mut self) {
        log::info!("WgpuRenderer shutting down internal GraphicsContext...");
        if let Some(gc_arc_mutex) = self.graphics_context.take() {
            match Arc::try_unwrap(gc_arc_mutex) {
                Ok(mutex_gc) => match mutex_gc.into_inner() {
                    Ok(_gc_instance) => {
                        log::info!("GraphicsContext successfully unwrapped and will be dropped.");
                    }
                    Err(poisoned_err) => {
                        log::error!(
                            "Mutex was poisoned during shutdown: {}. Resources might not be fully cleaned.",
                            poisoned_err
                        );
                    }
                },
                Err(_still_shared_arc) => {
                    log::warn!(
                        "GraphicsContext Arc is still shared elsewhere during shutdown. Resources will be dropped when last Arc reference is gone."
                    );
                }
            }
        }
        self.graphics_context = None;
        log::info!("WgpuRenderer shutdown complete.");
    }

    fn get_adapter_info(&self) -> Option<RendererAdapterInfo> {
        self.graphics_context
            .as_ref()
            .and_then(|gc_arc_mutex| match gc_arc_mutex.lock() {
                Ok(gc_guard) => {
                    let backend_type = match gc_guard.adapter_backend {
                        wgpu::Backend::Vulkan => RendererBackendType::Vulkan,
                        wgpu::Backend::Metal => RendererBackendType::Metal,
                        wgpu::Backend::Dx12 => RendererBackendType::Dx12,
                        wgpu::Backend::Gl => RendererBackendType::OpenGl,
                        wgpu::Backend::BrowserWebGpu => RendererBackendType::WebGpu,
                        _ => RendererBackendType::Unknown,
                    };
                    let device_type = match gc_guard.adapter_device_type {
                        wgpu::DeviceType::IntegratedGpu => RendererDeviceType::IntegratedGpu,
                        wgpu::DeviceType::DiscreteGpu => RendererDeviceType::DiscreteGpu,
                        wgpu::DeviceType::VirtualGpu => RendererDeviceType::VirtualGpu,
                        wgpu::DeviceType::Cpu => RendererDeviceType::Cpu,
                        _ => RendererDeviceType::Unknown,
                    };
                    Some(RendererAdapterInfo {
                        name: gc_guard.adapter_name.clone(),
                        backend_type,
                        device_type,
                    })
                }
                Err(e) => {
                    log::error!("get_adapter_info: Failed to lock GraphicsContext: {}", e);
                    None
                }
            })
    }
}
