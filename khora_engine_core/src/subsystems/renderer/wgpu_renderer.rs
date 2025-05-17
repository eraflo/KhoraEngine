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

use std::sync::Arc;

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
    graphics_context: Option<Arc<GraphicsContext>>,
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
                self.graphics_context = Some(Arc::new(context));
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
        if new_width == 0 || new_height == 0 {
            log::warn!("WgpuRenderer::resize called with zero size. Ignoring.");
            return;
        }

        if self.graphics_context.is_none() {
            log::warn!("WgpuRenderer::resize called before initialization.");
            return;
        }

        self.current_width = new_width;
        self.current_height = new_height;

        if let Some(_gc_arc) = &self.graphics_context {
            // To call `resize` which takes `&mut self` on GraphicsContext,
            // we need a mutable reference. If Arc::get_mut returns Some, it means
            // this is the only strong reference to the GraphicsContext, allowing mutation.
            // This is generally true if WgpuRenderer is the sole owner of this Arc.
            if let Some(gc_mut) = Arc::get_mut(self.graphics_context.as_mut().unwrap()) {
                gc_mut.resize(new_width, new_height);
            } else {
                log::warn!(
                    "WgpuRenderer::resize: Could not get mutable access to GraphicsContext via Arc. Resize might not have taken full effect if Arc is shared and GraphicsContext resize needs &mut."
                );

                // If Arc::get_mut fails, it means the GraphicsContext is shared.
                // We can still call resize on the Arc, but it will not be a mutable reference.
                let gc_arc_for_mutation_attempt = self
                    .graphics_context
                    .as_mut()
                    .expect("Graphics context should exist for resize");
                if let Some(gc_mut_ref) = Arc::get_mut(gc_arc_for_mutation_attempt) {
                    gc_mut_ref.resize(new_width, new_height);
                } else {
                    log::warn!(
                        "WgpuRenderer::resize: Arc::get_mut failed. GraphicsContext might be shared unexpectedly. Resize might not be fully effective."
                    );
                }
            }
        } else {
            log::warn!("WgpuRenderer::resize called before initialization.");
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
            log::error!("WgpuRenderer::render_to_window called before initialization or after a fatal error.");
            RenderSystemError::RenderFailed("GraphicsContext not available".to_string())
        })?;

        // 1. Get Surface Texture
        let output_surface_texture = gc.get_current_texture().map_err(|e| {
            log::warn!("WgpuRenderer: Failed to get current texture: {:?}", e);
            match e {
                wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                    RenderSystemError::SurfaceAcquisitionFailed(format!(
                        "Surface Lost/Outdated: {:?}",
                        e
                    ))
                }
                wgpu::SurfaceError::OutOfMemory => {
                    RenderSystemError::SurfaceAcquisitionFailed("OutOfMemory".to_string())
                }
                wgpu::SurfaceError::Timeout => {
                    RenderSystemError::SurfaceAcquisitionFailed("Timeout".to_string())
                }
                _ => RenderSystemError::SurfaceAcquisitionFailed(format!(
                    "Other surface error: {:?}",
                    e
                )),
            }
        })?;

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

        let device = gc.device();
        let queue = gc.queue();

        // 3. Create a command encoder
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("WgpuRenderer Command Encoder"),
        });

        // 4. Begin the render pass
        let mut _actual_draw_calls = 0;
        let mut _actual_triangles = 0;

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("WgpuRenderer Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(gc.get_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None, // TODO: add depth buffer
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // TODO: Draw the renderables here
        }

        let cpu_render_logic_duration_ms =
            cpu_render_logic_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

        // --- 5. Submit Commands ---
        let cpu_submission_timer = Stopwatch::new();
        queue.submit(std::iter::once(encoder.finish()));
        let cpu_submission_duration_ms =
            cpu_submission_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

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
        if let Some(gc) = &self.graphics_context {
            match feature_name {
                "gpu_timestamps" => gc
                    .active_device_features
                    .contains(wgpu::Features::TIMESTAMP_QUERY),
                "texture_compression_bc" => gc
                    .active_device_features
                    .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
                _ => false,
            }
        } else {
            false
        }
    }

    fn shutdown(&mut self) {
        log::info!("WgpuRenderer shutting down internal GraphicsContext...");
        self.graphics_context = None;
    }

    fn get_adapter_info(&self) -> Option<RendererAdapterInfo> {
        self.graphics_context.as_ref().map(|gc| {
            let backend_type = match gc.adapter_backend {
                wgpu::Backend::Vulkan => RendererBackendType::Vulkan,
                wgpu::Backend::Metal => RendererBackendType::Metal,
                wgpu::Backend::Dx12 => RendererBackendType::Dx12,
                wgpu::Backend::Gl => RendererBackendType::OpenGl,
                wgpu::Backend::BrowserWebGpu => RendererBackendType::WebGpu,
                _ => RendererBackendType::Unknown, // Catch-all for future/other backends
            };
            let device_type = match gc.adapter_device_type {
                wgpu::DeviceType::Other => RendererDeviceType::Unknown,
                wgpu::DeviceType::IntegratedGpu => RendererDeviceType::IntegratedGpu,
                wgpu::DeviceType::DiscreteGpu => RendererDeviceType::DiscreteGpu,
                wgpu::DeviceType::VirtualGpu => RendererDeviceType::VirtualGpu,
                wgpu::DeviceType::Cpu => RendererDeviceType::Cpu,
            };
            RendererAdapterInfo {
                name: gc.adapter_name.clone(),
                backend_type,
                device_type,
            }
        })
    }
}
