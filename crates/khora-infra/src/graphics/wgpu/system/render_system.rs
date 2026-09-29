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

//! `RenderSystem` for the wgpu backend: frames, resize, stats.

use super::WgpuRenderSystem;
use crate::graphics::wgpu::profiler::WgpuTimestampProfiler;
use khora_core::math::LinearRgba;
use khora_core::platform::window::KhoraWindow;
use khora_core::renderer::api::command::{
    LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::device::{GraphicsAdapterInfo, RenderSettings, RenderStats};
use khora_core::renderer::api::gpu_scene::RenderObject;
use khora_core::renderer::api::resource::IndexFormat;
use khora_core::renderer::api::resource::ViewInfo;
use khora_core::renderer::traits::{FrameTargets, RenderSystem};
use khora_core::renderer::{GraphicsDevice, RenderError};
use khora_core::telemetry::ResourceMonitor;
use khora_core::Stopwatch;
use std::sync::Arc;
use std::time::Instant;
use winit::dpi::PhysicalSize;

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

        // Bail out early (non-panicking) on a lost / out-of-memory device.
        self.check_device_health()?;

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

        // --- 1. Acquire Frame from Swap Chain (resilient) ---
        device.wait_for_last_submission();
        let output_surface_texture = match self.acquire_surface_texture(&gc)? {
            Some(texture) => texture,
            None => {
                // Transient skip — return last frame's stats unchanged so the
                // caller treats this as a no-op frame, not a hard failure.
                return Ok(self.last_frame_stats.clone());
            }
        };

        let command_recording_timer = Stopwatch::new();

        // --- 2. Create a managed, abstract view for the swap chain texture ---
        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.register_texture_view(
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
            let gc_guard = gc
                .lock()
                .map_err(|_| RenderError::Internal("graphics context lock poisoned".into()))?;
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
                base_array_layer: 0,
                base_mip_level: 0,
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
                    base_array_layer: 0,
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
        let submission_ms = match command_encoder.finish() {
            Some(command_buffer) => {
                device.submit_command_buffer(command_buffer);
                submission_timer.elapsed_ms().unwrap_or(0)
            }
            None => {
                log::error!(
                    "WgpuRenderSystem: command_encoder.finish() returned None — skipping submit"
                );
                0
            }
        };

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

    fn begin_frame(&mut self) -> Result<FrameTargets, RenderError> {
        let device = self
            .wgpu_device
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        // Bail out early (non-panicking) if the device was reported lost or
        // out-of-memory: there is no point acquiring or submitting any work.
        self.check_device_health()?;

        // Process any pending GPU-to-CPU callbacks (profiler map_async, etc.).
        device.poll_device_non_blocking();
        // Block until the previous submission is consumed so the acquire
        // semaphore is guaranteed to be unsignaled.
        device.wait_for_last_submission();

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
        }

        if resized_this_frame {
            if let Err(e) = self.create_depth_texture() {
                log::warn!("Failed to recreate depth texture: {:?}", e);
            }
        }

        // --- Acquire swapchain texture (resilient: see acquire_surface_texture) ---
        let output_surface_texture = match self.acquire_surface_texture(&gc)? {
            Some(texture) => texture,
            None => {
                // Transient skip (minimized / timeout / occluded / reconfigure
                // in flight). Not an error — report a non-fatal acquisition
                // failure so the engine skips this frame and retries next one.
                return Err(RenderError::SurfaceAcquisitionFailed(
                    "frame skipped (surface not ready)".to_string(),
                ));
            }
        };

        // --- Create texture view for the frame ---
        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.register_texture_view(
            &output_surface_texture.texture,
            Some("Primary Swap Chain View"),
        )?;
        self.current_frame_view_id = Some(target_view_id);

        self.active_frame_texture = Some(output_surface_texture);

        // Compute targets for the engine: choose between swapchain and viewport.
        let (color, depth) = if self.render_to_viewport {
            let c = self
                .viewport_color_view_id
                .ok_or_else(|| RenderError::Internal("viewport color view not created".into()))?;
            (c, self.viewport_depth_view_id)
        } else {
            (target_view_id, self.depth_texture_view)
        };

        Ok(FrameTargets { color, depth })
    }

    fn end_frame(&mut self) -> Result<RenderStats, RenderError> {
        // If the device went down between acquire and present, surface a fatal
        // error instead of presenting a texture from a dead device.
        self.check_device_health()?;

        // `SurfaceTexture::present()` is infallible in this wgpu version: if a
        // present fails internally it is reported through the device error
        // callback (handled by `check_device_health`), and an un-presented
        // texture is discarded on drop rather than panicking.
        if let Some(texture) = self.active_frame_texture.take() {
            texture.present();
        }

        self.frame_count += 1;
        self.last_frame_stats.frame_number = self.frame_count;

        if let Some(monitor) = &self.gpu_monitor {
            monitor.update_from_frame_stats(&self.last_frame_stats);
        }

        Ok(self.last_frame_stats.clone())
    }

    fn render_overlay(
        &mut self,
        overlay: &mut dyn khora_core::ui::EditorOverlay,
        screen: khora_core::ui::OverlayScreenDescriptor,
    ) -> Result<(), RenderError> {
        let gc_arc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .clone();

        // Create encoder and target view while holding the lock, then release.
        let (encoder, target_view) = {
            let gc = gc_arc
                .lock()
                .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;

            let surface_tex = self
                .active_frame_texture
                .as_ref()
                .ok_or(RenderError::NotInitialized)?;

            let target_view = surface_tex
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let encoder = gc
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui_overlay_encoder"),
                });

            (encoder, target_view)
        }; // gc lock released — overlay will re-acquire it

        let mut render_state = crate::ui::egui::overlay::EguiFrameRenderState {
            graphics_context: gc_arc.clone(),
            encoder: Some(encoder),
            target_view,
            width_px: screen.width_px,
            height_px: screen.height_px,
        };

        overlay
            .end_frame_and_render(&mut render_state as &mut dyn std::any::Any)
            .map_err(|e| RenderError::RenderingFailed(e.to_string()))?;

        // Submit the encoder
        let encoder = render_state.encoder.take().ok_or_else(|| {
            RenderError::RenderingFailed("Encoder consumed during overlay render".into())
        })?;

        let gc = gc_arc
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;
        gc.queue.submit(std::iter::once(encoder.finish()));

        Ok(())
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

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn render_to_viewport(&self) -> bool {
        self.render_to_viewport
    }

    fn set_render_to_viewport(&mut self, enabled: bool) {
        self.render_to_viewport = enabled;
    }

    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo> {
        self.wgpu_device.as_ref().map(|d| d.get_adapter_info())
    }

    fn graphics_device(&self) -> Arc<dyn GraphicsDevice> {
        // Invariant: `wgpu_device` is populated during render-system
        // initialization (adapter + device creation) and is only cleared on
        // shutdown. The bootstrap path calls `graphics_device()` exactly once,
        // right after a successful init and long before shutdown — so the
        // device is always present here. The trait returns a bare
        // `Arc<dyn GraphicsDevice>` (no fallible variant), and there is no
        // sound placeholder device to substitute, so a `None` at this point is
        // an init-ordering bug rather than a recoverable runtime condition.
        self.wgpu_device.clone().expect(
            "WgpuRenderSystem::graphics_device called before initialization or after shutdown",
        )
    }
}
