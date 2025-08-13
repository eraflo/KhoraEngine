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
use std::time::Instant;

use crate::subsystems::renderer::{
    RenderError, RenderObject, RenderSettings, RenderStats, ViewInfo,
};
use crate::{core::timer::Stopwatch, window::KhoraWindow};

use super::gpu_timestamp_profiler::WgpuTimestampProfiler;
use super::wgpu_device::WgpuDevice;
use super::wgpu_graphic_context::WgpuGraphicsContext;
use crate::subsystems::renderer::GpuPerformanceMonitor;
use crate::subsystems::renderer::api::common_types::GpuPerfHook;
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
    gpu_profiler: Option<WgpuTimestampProfiler>,
    // Generic GPU performance monitor
    gpu_performance_monitor: Arc<GpuPerformanceMonitor>,
    // --- Resize Heuristics State ---
    // Hybrid throttle + debounce + stability detection to reduce swapchain reconfigure churn
    // and minimize "Suboptimal present" warning bursts while avoiding long mismatch periods.
    last_resize_event: Option<Instant>,
    pending_resize: bool,
    // Track when we last actually reconfigured the surface (throttle immediate reconfigure)
    last_surface_config: Option<Instant>,
    // Count frames since a resize became pending (fallback safety)
    pending_resize_frames: u32,
    // Track stability of last requested size to allow early reconfigure when dragging stops
    last_pending_size: Option<(u32, u32)>,
    stable_size_frame_count: u32,
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
            gpu_profiler: None,
            gpu_performance_monitor: Arc::new(GpuPerformanceMonitor::new("WGPU".to_string())),
            last_resize_event: None,
            pending_resize: false,
            last_surface_config: None,
            pending_resize_frames: 0,
            last_pending_size: None,
            stable_size_frame_count: 0,
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

                if let Ok(gc_guard) = self.graphics_context_shared.as_ref().unwrap().lock() {
                    if WgpuTimestampProfiler::feature_available(gc_guard.active_device_features) {
                        self.gpu_profiler =
                            WgpuTimestampProfiler::new(gc_guard.device(), gc_guard.queue());
                    } else {
                        log::info!(
                            "GPU timestamp feature not available; instrumentation disabled."
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                log::error!("WgpuRenderSystem: Failed to initialize internal GraphicsContext: {e}");
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
            let now = Instant::now();
            // Track stability of requested size
            if let Some((lw, lh)) = self.last_pending_size {
                if lw == new_width && lh == new_height {
                    self.stable_size_frame_count = self.stable_size_frame_count.saturating_add(1);
                } else {
                    self.stable_size_frame_count = 0;
                }
            }
            self.last_pending_size = Some((new_width, new_height));
            // Immediate reconfigure if gap >= threshold since last surface config.
            // Limits time spent presenting with mismatched surface (reduces warning window).
            let immediate_threshold_ms: u128 = 80; // minimum gap between immediate resizes
            let can_immediate = self
                .last_surface_config
                .map(|t| t.elapsed().as_millis() >= immediate_threshold_ms)
                .unwrap_or(true);
            // Also allow an *early* reconfigure if size has remained identical for >=2 consecutive resize events
            let early_stable = self.stable_size_frame_count >= 2
                && self
                    .last_surface_config
                    .map(|t| t.elapsed().as_millis() >= 20) // small guard window
                    .unwrap_or(true);
            if (can_immediate || early_stable)
                && let Some(gc_arc_mutex) = &self.graphics_context_shared
                && let Ok(mut gc_guard) = gc_arc_mutex.lock()
            {
                gc_guard.resize(self.current_width, self.current_height);
                self.last_surface_config = Some(now);
                self.pending_resize = false;
                self.pending_resize_frames = 0;
                if early_stable {
                    log::info!(
                        "WGPUGraphicsContext: Early stable-size surface configuration to {}x{} (stable events {})",
                        self.current_width,
                        self.current_height,
                        self.stable_size_frame_count
                    );
                } else {
                    log::info!(
                        "WGPUGraphicsContext: Immediate throttled surface configuration to {}x{}",
                        self.current_width,
                        self.current_height
                    );
                }
                return; // done
            }
            // Otherwise mark resize as pending → evaluated in render() (debounce/fallback logic).
            self.last_resize_event = Some(now);
            self.pending_resize = true;
            self.pending_resize_frames = 0;
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

        if let Some(p) = self.gpu_profiler.as_mut() {
            p.try_read_previous_frame();
        }

        // Get the graphics context
        let gc = self.graphics_context_shared.as_ref().ok_or_else(|| {
            RenderError::InitializationFailed(
                "GraphicsContext Arc<Mutex> is None in WgpuRenderSystem::render".to_string(),
            )
        })?;

        // Opportunistic debounced resize: triggers if quiet >= 120ms, fallback frame count, or stability >= 3.
        if self.pending_resize {
            self.pending_resize_frames = self.pending_resize_frames.saturating_add(1);
            if let Some(t) = self.last_resize_event {
                let quiet_elapsed = t.elapsed().as_millis();
                let debounce_quiet_ms: u128 = 120; // silence threshold for debounce
                let max_pending_frames: u32 = 10; // fallback frames (~1/6s @ 60fps)
                // Early stable condition inside render loop: size is unchanged for >=3 frames.
                let early_stable = self.stable_size_frame_count >= 3;
                if (quiet_elapsed >= debounce_quiet_ms
                    || self.pending_resize_frames >= max_pending_frames
                    || early_stable)
                    && let Some(gc_arc_mutex) = &self.graphics_context_shared
                    && let Ok(mut gc_guard) = gc_arc_mutex.lock()
                {
                    gc_guard.resize(self.current_width, self.current_height);
                    self.pending_resize = false;
                    self.last_surface_config = Some(Instant::now());
                    log::info!(
                        "WGPUGraphicsContext: Deferred surface configuration to {}x{} (quiet {} ms, frames pending {}, stable events {})",
                        self.current_width,
                        self.current_height,
                        quiet_elapsed,
                        self.pending_resize_frames,
                        self.stable_size_frame_count
                    );
                    self.stable_size_frame_count = 0; // reset after apply
                }
            }
        }

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

            // Pump async GPU work & mapping callbacks (needed for timestamp map_async completions)
            if settings.enable_gpu_timestamps
                && let Some(p) = self.gpu_profiler.as_mut()
            {
                // Try polling with a reasonable default: non-blocking (PollType::Wait is blocking, so prefer PollType::Poll if exists).
                #[allow(unused)]
                use wgpu::PollType;
                // Heuristic: attempt to poll until all submitted work done for callbacks of prior frames.
                let _ = gc_guard.device().poll(wgpu::PollType::Poll); // non-blocking pump for map_async callbacks
                p.try_read_previous_frame();
            }

            // --- 3. Create Command Encoder
            let mut encoder =
                gc_guard
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("WgpuRenderSystem Render Command Encoder"),
                    });

            // --- 4. Pass A (compute only): frame_start + main_pass_begin
            // Previous attempt used an empty render pass (no attachments) which is invalid.
            // A compute pass allows timestamp_writes without needing attachments.
            if settings.enable_gpu_timestamps
                && let Some(p) = self.gpu_profiler.as_ref()
            {
                let _cpass_a = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("WgpuRenderSystem Timestamp Compute Pass A"),
                    timestamp_writes: Some(p.compute_pass_a_timestamp_writes()),
                });
                drop(_cpass_a);
            }

            // --- 5. Main Render Pass (visual)
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("WgpuRenderSystem Clear Screen Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(gc_guard.get_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None, // main pass no longer writes timestamps; handled by A & B
            });
            // Rendering of actual scene objects will be integrated here (set pipelines, bind groups, draw calls).
            drop(_render_pass);

            // --- 6. Pass B (compute only): main_pass_end + frame_end
            if settings.enable_gpu_timestamps
                && let Some(p) = self.gpu_profiler.as_ref()
            {
                let _cpass_b = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("WgpuRenderSystem Timestamp Compute Pass B"),
                    timestamp_writes: Some(p.compute_pass_b_timestamp_writes()),
                });
                drop(_cpass_b);
            }

            // Resolve timestamps (now all 4 indices 0..3)
            if settings.enable_gpu_timestamps
                && let Some(p) = self.gpu_profiler.as_ref()
            {
                p.resolve_and_copy(&mut encoder);
                // Copy resolved region into staging buffer for this frame index
                p.copy_to_staging(&mut encoder, self.frame_count);
            }

            // --- 7. Submit Commands ---
            cpu_submission_timer = Stopwatch::new();
            gc_guard.queue().submit(std::iter::once(encoder.finish()));
            cpu_submission_duration_ms =
                cpu_submission_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;
            // Deferred read plan: map the staging buffer from two frames earlier (triple-buffer, +2 frame latency)
            if settings.enable_gpu_timestamps
                && let Some(p) = self.gpu_profiler.as_mut()
            {
                p.schedule_map_after_submit(self.frame_count);
            }
        }

        let cpu_render_logic_duration_ms =
            cpu_render_logic_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

        // --- 6. Present Frame ---
        output_surface_texture.present();

        let total_cpu_prep_duration_ms =
            total_cpu_prep_timer.elapsed_secs_f64().unwrap_or(0.0) * 1000.0;

        self.frame_count += 1;
        if let Some(p) = self.gpu_profiler.as_ref() {
            self.last_frame_stats.gpu_main_pass_time_ms = p.last_main_pass_ms();
            self.last_frame_stats.gpu_frame_total_time_ms = p.last_frame_total_ms();
        }
        self.last_frame_stats = RenderStats {
            frame_number: self.frame_count,
            cpu_preparation_time_ms: total_cpu_prep_duration_ms as f32
                - cpu_render_logic_duration_ms as f32,
            cpu_render_submission_time_ms: cpu_submission_duration_ms as f32,
            // Preserve values computed earlier in this frame (updated only if timestamps resolved)
            gpu_main_pass_time_ms: self.last_frame_stats.gpu_main_pass_time_ms,
            gpu_frame_total_time_ms: self.last_frame_stats.gpu_frame_total_time_ms,
            draw_calls: _actual_draw_calls,
            triangles_rendered: _actual_triangles,
            vram_usage_estimate_mb: 0.0, // Placeholder until VRAM tracking is implemented
        };

        // Update GPU performance monitor with latest stats using the generic monitor
        self.gpu_performance_monitor
            .update_from_render_system(self, &self.last_frame_stats);

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

    fn gpu_hook_time_ms(&self, hook: GpuPerfHook) -> Option<f32> {
        if let Some(p) = &self.gpu_profiler {
            match hook {
                GpuPerfHook::FrameStart => None, // instantaneous marker
                GpuPerfHook::MainPassBegin => None,
                GpuPerfHook::MainPassEnd => None,
                GpuPerfHook::FrameEnd => Some(p.last_frame_total_ms()),
            }
        } else {
            None
        }
    }
}

impl WgpuRenderSystem {
    /// Get a reference to the GPU performance monitor for external access
    pub fn gpu_performance_monitor(&self) -> &GpuPerformanceMonitor {
        &self.gpu_performance_monitor
    }
}
