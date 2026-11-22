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
//! See the book: concepts/rendering.md, "The frame lifecycle".

use super::WgpuRenderSystem;
use crate::graphics::wgpu::conversions::from_wgpu_texture_format;
use crate::graphics::wgpu::profiler::WgpuTimestampProfiler;
use khora_core::math::{Extent2D, LinearRgba};
use khora_core::platform::window::KhoraWindow;
use khora_core::renderer::api::command::{
    LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::device::{GraphicsAdapterInfo, RenderSettings, RenderStats};
use khora_core::renderer::api::gpu_scene::RenderObject;
use khora_core::renderer::api::resource::ViewInfo;
use khora_core::renderer::api::resource::{IndexFormat, TextureFormat};
use khora_core::renderer::traits::{FrameTargets, RenderSystem};
use khora_core::renderer::{GraphicsDevice, RenderError};
use khora_core::telemetry::ResourceMonitor;
use khora_core::Stopwatch;
use std::sync::Arc;
use winit::dpi::PhysicalSize;

/// The editor viewport's colour format: always bindable in shaders, so the
/// editor can show it in a panel.
const VIEWPORT_COLOR_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

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
            log::debug!("WgpuRenderSystem: window resized to {new_width}x{new_height}");
            // Only the wanted size is recorded: the surface follows it at the
            // start of a frame, once it has settled. A window wider than the
            // device can render is drawn at the largest size it can.
            let max = self
                .graphics_context_shared
                .as_ref()
                .and_then(|gc| gc.lock().ok())
                .map(|gc| gc.device.limits().max_texture_dimension_2d)
                .unwrap_or(wgpu::Limits::downlevel_defaults().max_texture_dimension_2d);
            (self.current_width, self.current_height) =
                super::resize_debounce::clamp_extent((new_width, new_height), max);
        } else {
            log::warn!(
                "WgpuRenderSystem::resize called with zero size ({new_width}, {new_height}). Ignoring."
            );
        }
    }

    fn prepare_frame(&mut self, view_info: &ViewInfo) {
        if self.graphics_context_shared.is_none() {
            return;
        }
        let stopwatch = Stopwatch::new();

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

        // Runs the profiler's `map_async` callbacks, so it can read a past frame.
        device.poll_device_non_blocking();

        let gc = self
            .graphics_context_shared
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        if let Some(p) = self.gpu_profiler.as_mut() {
            p.try_read_previous_frame();
        }

        self.settle_swapchain_size(&gc);

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

        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.register_texture_view(
            &output_surface_texture.texture,
            Some("Primary Swap Chain View"),
        )?;
        self.current_frame_view_id = Some(target_view_id);

        let mut command_encoder = device.create_command_encoder(Some("Khora Main Command Encoder"));

        if settings.enable_gpu_timestamps {
            if let Some(profiler) = self.gpu_profiler.as_ref() {
                let _pass_a = command_encoder.begin_profiler_compute_pass(
                    Some("Timestamp Pass A"),
                    profiler.as_ref(),
                    0,
                );
            }
        }

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

            let mut render_pass = command_encoder.begin_render_pass(&pass_descriptor)?;

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

        if settings.enable_gpu_timestamps {
            if let Some(profiler) = self.gpu_profiler.as_ref() {
                // The pass must end, freeing its borrow of the encoder, before
                // the resolve and the copy borrow it again.
                {
                    let _pass_b = command_encoder.begin_profiler_compute_pass(
                        Some("Timestamp Pass B"),
                        profiler.as_ref(),
                        1,
                    );
                }
                profiler.resolve_and_copy(command_encoder.as_mut());
                profiler.copy_to_staging(command_encoder.as_mut(), self.frame_count + 1);
            }
        }

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
                p.schedule_map_after_submit(self.frame_count + 1);
            }
        }

        output_surface_texture.present();

        self.frame_count += 1;
        if let Some(p) = self.gpu_profiler.as_ref() {
            self.last_frame_stats.gpu_main_pass_time_ms = p.last_main_pass_ms();
            self.last_frame_stats.gpu_frame_total_time_ms = p.last_frame_total_ms();
            self.last_frame_stats.gpu_frame = p.last_frame();
        }
        let full_frame_ms = full_frame_timer.elapsed_ms().unwrap_or(0);
        self.last_frame_stats.frame_number = self.frame_count;
        self.last_frame_stats.cpu_preparation_time_ms =
            (full_frame_ms - command_recording_timer.elapsed_ms().unwrap_or(0)) as f32;
        self.last_frame_stats.cpu_render_submission_time_ms = submission_ms as f32;

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

        // Runs the profiler's `map_async` callbacks, so it can read a past frame.
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

        self.settle_swapchain_size(&gc);

        // The surface's format, known before anything is acquired: a format
        // the engine cannot name refuses the frame without holding a texture.
        let surface_format = {
            let format = gc
                .lock()
                .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?
                .surface_config
                .format;
            from_wgpu_texture_format(format).ok_or_else(|| {
                RenderError::Internal(format!("surface format {format:?} has no engine format"))
            })?
        };

        let output_surface_texture = match self.acquire_surface_texture(&gc)? {
            Some(texture) => texture,
            None => {
                // A transient skip, reported as a non-fatal failure so the
                // engine skips this frame and retries the next.
                return Err(RenderError::SurfaceAcquisitionFailed(
                    "frame skipped (surface not ready)".to_string(),
                ));
            }
        };

        if let Some(old_id) = self.current_frame_view_id.take() {
            device.destroy_texture_view(old_id)?;
        }
        let target_view_id = device.register_texture_view(
            &output_surface_texture.texture,
            Some("Primary Swap Chain View"),
        )?;
        self.current_frame_view_id = Some(target_view_id);

        self.active_frame_texture = Some(output_surface_texture);

        // The editor viewport takes the size its panel asked for, once that
        // size has settled — before anything is drawn into it this frame.
        self.settle_viewport_size();

        let (color, depth, (width, height), color_format) = if self.render_to_viewport {
            let c = self
                .viewport_color_view_id
                .ok_or_else(|| RenderError::Internal("viewport color view not created".into()))?;
            (
                c,
                self.viewport_depth_view_id,
                self.viewport_size(),
                VIEWPORT_COLOR_FORMAT,
            )
        } else {
            // The configured surface, not the last resize request: while a
            // resize is debounced the swapchain still has its old size.
            let surface = gc
                .lock()
                .map(|gc| (gc.surface_config.width, gc.surface_config.height))
                .unwrap_or((self.current_width, self.current_height));
            (
                target_view_id,
                self.depth_texture_view,
                surface,
                surface_format,
            )
        };

        Ok(FrameTargets {
            color,
            depth,
            size: Extent2D { width, height },
            color_format,
            // Both depth buffers, the window's and the viewport's.
            depth_format: depth.map(|_| TextureFormat::Depth32Float),
        })
    }

    fn end_frame(&mut self) -> Result<RenderStats, RenderError> {
        // If the device went down between acquire and present, surface a fatal
        // error instead of presenting a texture from a dead device.
        self.check_device_health()?;

        // `present()` cannot fail: a failure inside it reaches the device error
        // callback, which `check_device_health` reads next frame.
        if let Some(texture) = self.active_frame_texture.take() {
            texture.present();
        }

        self.frame_count += 1;
        self.last_frame_stats.frame_number = self.frame_count;
        if let Some(p) = self.gpu_profiler.as_ref() {
            self.last_frame_stats.gpu_main_pass_time_ms = p.last_main_pass_ms();
            self.last_frame_stats.gpu_frame_total_time_ms = p.last_frame_total_ms();
            self.last_frame_stats.gpu_frame = p.last_frame();
        }

        Ok(self.last_frame_stats.clone())
    }

    /// Submits a timestamp pass before `submit`'s work and one after it;
    /// returns whether it timed the frame.
    ///
    /// The four timestamps are copied out for a read-back two frames later.
    /// Without a profiler (no `TIMESTAMP_QUERY`) or a device, runs `submit`
    /// untimed. See the book: concepts/telemetry.md, "The graphics card".
    fn time_on_gpu(&mut self, submit: &mut dyn FnMut()) -> bool {
        let (Some(device), Some(profiler)) = (self.wgpu_device.clone(), self.gpu_profiler.as_mut())
        else {
            submit();
            return false;
        };
        // The number `end_frame` gives this frame: its reading is named by it.
        let frame = self.frame_count + 1;
        let mut opening = device.create_command_encoder(Some("Khora GPU Timing: open"));
        {
            let _pass = opening.begin_profiler_compute_pass(
                Some("Khora GPU Timing: frame start"),
                profiler.as_ref(),
                0,
            );
        }
        if let Some(buffer) = opening.finish() {
            device.submit_command_buffer(buffer);
        }

        submit();

        let mut closing = device.create_command_encoder(Some("Khora GPU Timing: close"));
        {
            let _pass = closing.begin_profiler_compute_pass(
                Some("Khora GPU Timing: frame end"),
                profiler.as_ref(),
                1,
            );
        }
        profiler.resolve_and_copy(closing.as_mut());
        profiler.copy_to_staging(closing.as_mut(), frame);
        match closing.finish() {
            Some(buffer) => {
                device.submit_command_buffer(buffer);
                profiler.schedule_map_after_submit(frame);
                true
            }
            None => false,
        }
    }

    fn render_overlay(
        &mut self,
        overlay: &mut dyn khora_core::ui::EditorOverlay,
        _screen: khora_core::ui::OverlayScreenDescriptor,
    ) -> Result<(), RenderError> {
        let gc_arc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .clone();

        // The overlay takes the context's lock itself: release it before.
        let (encoder, target_view, (width_px, height_px)) = {
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
            // The overlay covers the texture it is drawn on: while a window
            // resize is still settling the surface keeps its old size, and the
            // window's would put scissor rects outside it.
            let target_size = (surface_tex.texture.width(), surface_tex.texture.height());

            let encoder = gc
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui_overlay_encoder"),
                });

            (encoder, target_view, target_size)
        }; // gc lock released — overlay will re-acquire it

        let viewport_retarget = self.take_viewport_retarget();
        let hands_over_viewport = viewport_retarget.is_some();
        let mut render_state = crate::ui::egui::overlay::EguiFrameRenderState {
            graphics_context: gc_arc.clone(),
            encoder: Some(encoder),
            target_view,
            width_px,
            height_px,
            retargets: viewport_retarget.into_iter().collect(),
        };

        if let Err(e) = overlay.end_frame_and_render(&mut render_state as &mut dyn std::any::Any) {
            // The panel would keep showing the old texture: hand the new one
            // over again next frame.
            if hands_over_viewport {
                self.viewport_rebind_pending = true;
            }
            return Err(RenderError::RenderingFailed(e.to_string()));
        }

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
        // The device lives from `init` to `shutdown`, and the boot asks for it
        // once, right after `init`: a `None` here is an ordering bug, and the
        // infallible trait has no placeholder device to give instead.
        self.wgpu_device.clone().expect(
            "WgpuRenderSystem::graphics_device called before initialization or after shutdown",
        )
    }
}
