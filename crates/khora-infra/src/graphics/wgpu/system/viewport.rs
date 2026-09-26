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

//! The editor viewport target: creating it, clearing it, its size.

use super::WgpuRenderSystem;
use khora_core::math::LinearRgba;
use khora_core::renderer::RenderError;

impl WgpuRenderSystem {
    /// Creates an offscreen render target for the editor viewport and
    /// registers it as an egui texture.
    ///
    /// Returns the `egui::TextureId` that can be displayed via
    /// [`UiBuilder::viewport_image`].
    pub fn create_viewport_target(
        &mut self,
        width: u32,
        height: u32,
        overlay: &mut crate::ui::egui::overlay::EguiOverlay,
    ) -> Result<egui::TextureId, RenderError> {
        let gc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;

        // Use RGBA8 so the viewport texture is always bindable in shaders.
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;

        // --- Color texture ---
        let color_tex = gc.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Depth texture ---
        let depth_tex = gc.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Register with the egui overlay renderer.
        let egui_id = overlay.register_viewport_texture(&gc.device, &color_view);

        let device = self
            .wgpu_device
            .clone()
            .ok_or(RenderError::NotInitialized)?;
        let viewport_color_vid = device
            .register_texture_view(&color_tex, Some("viewport_color_view"))
            .map_err(|e| RenderError::Internal(format!("register viewport color view: {e}")))?;
        let viewport_depth_vid = device
            .register_texture_view(&depth_tex, Some("viewport_depth_view"))
            .map_err(|e| RenderError::Internal(format!("register viewport depth view: {e}")))?;

        self.viewport_texture = Some(color_tex);
        self.viewport_view = Some(color_view);
        self.viewport_depth_texture = Some(depth_tex);
        self.viewport_depth_view = Some(depth_view);
        self.viewport_width = width;
        self.viewport_height = height;
        self.viewport_color_view_id = Some(viewport_color_vid);
        self.viewport_depth_view_id = Some(viewport_depth_vid);

        log::info!("Viewport target created: {width}x{height} ({format:?})");

        Ok(egui_id)
    }

    /// Renders a clear pass to the offscreen viewport target.
    ///
    /// Call this once per frame (after `begin_frame()`, before
    /// `render_overlay()`). The cleared image will be visible in the
    /// egui viewport panel.
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
            // Pass drops here — the clear is all we need for now.
        }

        gc.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Returns the current viewport dimensions `(width, height)` in pixels.
    pub fn viewport_size(&self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }
}
