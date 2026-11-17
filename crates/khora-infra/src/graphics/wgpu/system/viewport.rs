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

//! The editor viewport target: creating it, following its panel's size,
//! clearing it, its size.

use super::resize_debounce::clamp_extent;
use super::WgpuRenderSystem;
use khora_core::math::LinearRgba;
use khora_core::renderer::{GraphicsDevice, RenderError};
use std::time::Instant;

impl WgpuRenderSystem {
    /// Creates the editor viewport's offscreen target and registers it as an egui texture.
    ///
    /// The returned id keeps showing the target across every resize asked with
    /// [`request_viewport_size`](Self::request_viewport_size); a panel shows it with
    /// [`viewport_image`](khora_core::ui::editor::ui_builder::UiBuilder::viewport_image).
    ///
    /// # Errors
    ///
    /// [`RenderError::NotInitialized`] before `init`, [`RenderError::Internal`]
    /// when the graphics context's lock is poisoned or a view of the target
    /// cannot be registered with the device.
    pub fn create_viewport_target(
        &mut self,
        width: u32,
        height: u32,
        overlay: &mut crate::ui::egui::overlay::EguiOverlay,
    ) -> Result<egui::TextureId, RenderError> {
        self.build_viewport_textures(width, height)?;

        let gc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;
        let color_view = self
            .viewport_view
            .as_ref()
            .ok_or(RenderError::NotInitialized)?;
        let egui_id = overlay.register_viewport_texture(&gc.device, color_view);
        self.viewport_egui_texture = Some(egui_id);
        self.viewport_rebind_pending = false;

        Ok(egui_id)
    }

    /// Asks for the viewport target to be `width` × `height` physical pixels,
    /// the size of its panel.
    ///
    /// Called once per frame by whoever lays the panel out; the target follows
    /// at the start of a frame, once the size has settled.
    pub fn request_viewport_size(&mut self, width: u32, height: u32) {
        self.viewport_resize
            .request((width, height), Instant::now());
    }

    /// Rebuilds the viewport target at the requested size when the debounce
    /// says so. A failed rebuild keeps the old target.
    pub(super) fn settle_viewport_size(&mut self) {
        if self.viewport_view.is_none() {
            return;
        }
        let current = self.viewport_size();
        let Some(wanted) = self.viewport_resize.poll(current, Instant::now()) else {
            return;
        };
        let max = self
            .graphics_context_shared
            .as_ref()
            .and_then(|gc| gc.lock().ok())
            .map(|gc| gc.device.limits().max_texture_dimension_2d)
            .unwrap_or(wgpu::Limits::downlevel_defaults().max_texture_dimension_2d);
        let (width, height) = clamp_extent(wanted, max);
        if (width, height) == self.viewport_size() {
            return;
        }
        match self.build_viewport_textures(width, height) {
            Ok(()) => self.viewport_rebind_pending = self.viewport_egui_texture.is_some(),
            Err(e) => log::warn!("Viewport target not resized to {width}x{height}: {e}"),
        }
    }

    /// Takes the egui texture to re-point at the viewport's colour view, once
    /// after each rebuild.
    pub(super) fn take_viewport_retarget(
        &mut self,
    ) -> Option<(egui::TextureId, wgpu::TextureView)> {
        if !self.viewport_rebind_pending {
            return None;
        }
        let id = self.viewport_egui_texture?;
        let view = self.viewport_view.clone()?;
        self.viewport_rebind_pending = false;
        Some((id, view))
    }

    /// Creates the viewport's colour and depth textures and registers their views,
    /// unregistering the previous ones.
    ///
    /// The egui binding is left as it is.
    fn build_viewport_textures(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let gc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;
        let device = self
            .wgpu_device
            .clone()
            .ok_or(RenderError::NotInitialized)?;

        // RGBA8, so the panel can always bind the texture in a shader.
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let color_tex = gc.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = gc.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let viewport_color_vid = device
            .register_texture_view(&color_tex, Some("viewport_color_view"))
            .map_err(|e| RenderError::Internal(format!("register viewport color view: {e}")))?;
        let viewport_depth_vid = device
            .register_texture_view(&depth_tex, Some("viewport_depth_view"))
            .map_err(|e| RenderError::Internal(format!("register viewport depth view: {e}")))?;

        // Only the ids go: the GPU keeps the old textures alive while
        // submitted work or egui's bind group still references them.
        for old in [
            self.viewport_color_view_id.take(),
            self.viewport_depth_view_id.take(),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(e) = device.destroy_texture_view(old) {
                log::warn!("Viewport target: old view not unregistered: {e}");
            }
        }

        self.viewport_texture = Some(color_tex);
        self.viewport_view = Some(color_view);
        self.viewport_depth_texture = Some(depth_tex);
        self.viewport_depth_view = Some(depth_view);
        self.viewport_width = width;
        self.viewport_height = height;
        self.viewport_color_view_id = Some(viewport_color_vid);
        self.viewport_depth_view_id = Some(viewport_depth_vid);

        log::debug!("Viewport target built: {width}x{height} ({format:?})");
        Ok(())
    }

    /// Clears the viewport target's colour and depth.
    ///
    /// Call it once per frame, after `begin_frame()` and before `render_overlay()`.
    ///
    /// # Errors
    ///
    /// [`RenderError::NotInitialized`] before the viewport target exists, and
    /// [`RenderError::Internal`] when the graphics context's lock is poisoned.
    pub fn render_viewport_clear(&mut self, clear_color: LinearRgba) -> Result<(), RenderError> {
        let color_view = self
            .viewport_view
            .as_ref()
            .ok_or(RenderError::NotInitialized)?;
        let depth_view = self
            .viewport_depth_view
            .as_ref()
            .ok_or(RenderError::NotInitialized)?;

        let gc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;

        let mut encoder = gc
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport_clear_encoder"),
            });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color.r as f64,
                            g: clear_color.g as f64,
                            b: clear_color.b as f64,
                            a: clear_color.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // The pass only clears: its load ops do the work.
        }

        gc.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Returns the current viewport dimensions `(width, height)` in pixels.
    pub fn viewport_size(&self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }
}
