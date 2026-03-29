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

//! Concrete [`EditorOverlay`] implementation backed by egui + custom wgpu renderer.
//!
//! This struct manages:
//! - An [`egui::Context`] for UI building (shared with `khora-editor`)
//! - An [`egui_winit::State`] for input event translation
//! - A custom [`EguiWgpuRenderer`] for rendering egui output with wgpu 28

use super::renderer::{EguiRenderState, EguiWgpuRenderer};
use crate::graphics::wgpu::context::WgpuGraphicsContext;
use egui::ViewportId;
use khora_core::ui::editor_overlay::{EditorOverlay, OverlayError, OverlayScreenDescriptor};
use std::any::Any;
use std::sync::{Arc, Mutex};

/// Render state passed through `end_frame_and_render` as `&mut dyn Any`.
///
/// The caller (`WgpuRenderSystem`) constructs this with the current frame's GPU resources.
/// All fields are owned (`'static`), so this type can be passed through `dyn Any`.
pub struct EguiFrameRenderState {
    /// The graphics context (device + queue), shared via Arc.
    pub graphics_context: Arc<Mutex<WgpuGraphicsContext>>,
    /// The wgpu command encoder for this overlay pass (owned, moved back after render).
    pub encoder: Option<wgpu::CommandEncoder>,
    /// The swapchain texture view to render onto (owned).
    pub target_view: wgpu::TextureView,
    /// Physical width in pixels.
    pub width_px: u32,
    /// Physical height in pixels.
    pub height_px: u32,
}

/// Egui-based editor overlay implementation.
///
/// Created once during engine initialization and stored in the `ServiceRegistry`.
/// The editor application accesses the shared `egui::Context` to build its UI.
pub struct EguiOverlay {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: EguiWgpuRenderer,
    /// Current screen descriptor.
    screen: OverlayScreenDescriptor,
}

impl EguiOverlay {
    /// Creates a new `EguiOverlay`.
    ///
    /// # Arguments
    /// * `event_loop` - The winit event loop (needed by `egui_winit::State`).
    /// * `surface_format` - The wgpu surface texture format.
    /// * `device` - The wgpu device for pipeline initialization.
    /// * `shader_source` - The WGSL shader source for egui rendering.
    pub fn new(
        event_loop: &winit::event_loop::ActiveEventLoop,
        surface_format: wgpu::TextureFormat,
        device: &wgpu::Device,
        shader_source: &str,
    ) -> Self {
        let ctx = egui::Context::default();
        let winit_state = egui_winit::State::new(
            ctx.clone(),
            ViewportId::ROOT,
            event_loop,
            Some(1.0), // default pixels_per_point
            None,       // theme
            None,       // max_texture_side
        );

        let mut renderer = EguiWgpuRenderer::new(surface_format);
        renderer.initialize(device, shader_source);

        Self {
            ctx,
            winit_state,
            renderer,
            screen: OverlayScreenDescriptor {
                width_px: 1,
                height_px: 1,
                scale_factor: 1.0,
            },
        }
    }

    /// Returns a clone of the egui context (cheap — internally Arc'd).
    ///
    /// The editor application stores this to build UI during `update()`.
    pub fn context(&self) -> egui::Context {
        self.ctx.clone()
    }

    /// Registers an external wgpu texture view with the egui renderer.
    ///
    /// Returns the egui `TextureId` that can later be mapped to a
    /// [`ViewportTextureHandle`].
    pub fn register_viewport_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
    ) -> egui::TextureId {
        self.renderer.register_external_texture(device, view)
    }

    /// Updates an existing viewport texture to point to a new wgpu view
    /// (e.g. after a resize).
    pub fn update_viewport_texture(
        &mut self,
        device: &wgpu::Device,
        id: egui::TextureId,
        view: &wgpu::TextureView,
    ) {
        self.renderer.update_external_texture(device, id, view);
    }
}

impl EditorOverlay for EguiOverlay {
    fn handle_window_event(&mut self, window: &dyn Any, event: &dyn Any) -> bool {
        let Some(winit_window) = window.downcast_ref::<winit::window::Window>() else {
            return false;
        };
        let Some(winit_event) = event.downcast_ref::<winit::event::WindowEvent>() else {
            return false;
        };

        let response = self.winit_state.on_window_event(winit_window, winit_event);
        response.consumed
    }

    fn begin_frame(&mut self, window: &dyn Any, screen: OverlayScreenDescriptor) {
        self.screen = screen;
        if let Some(winit_window) = window.downcast_ref::<winit::window::Window>() {
            let raw_input = self.winit_state.take_egui_input(winit_window);
            self.ctx.begin_pass(raw_input);
        } else {
            log::warn!("EguiOverlay: begin_frame called with non-winit window");
        }
    }

    fn ui_context(&self) -> &dyn Any {
        &self.ctx
    }

    fn end_frame_and_render(
        &mut self,
        render_state: &mut dyn Any,
    ) -> Result<(), OverlayError> {
        let output = self.ctx.end_pass();

        // Tessellate
        let pixels_per_point = output.pixels_per_point;
        let primitives = self.ctx.tessellate(output.shapes, pixels_per_point);
        let textures_delta = output.textures_delta;

        // Get the typed render state
        let state = render_state
            .downcast_mut::<EguiFrameRenderState>()
            .ok_or_else(|| OverlayError("Expected EguiFrameRenderState".to_string()))?;

        // Lock the graphics context for device/queue access
        let gc = state.graphics_context.lock()
            .map_err(|_| OverlayError("Failed to lock graphics context".to_string()))?;

        // Update textures
        self.renderer
            .update_textures(&gc.device, &gc.queue, &textures_delta);

        // Get a mutable reference to the encoder
        let encoder = state.encoder.as_mut()
            .ok_or_else(|| OverlayError("No command encoder available".to_string()))?;

        // Render
        let mut egui_render_state = EguiRenderState {
            device: &gc.device,
            queue: &gc.queue,
            encoder,
            target_view: &state.target_view,
            width_px: self.screen.width_px,
            height_px: self.screen.height_px,
        };

        self.renderer
            .render(&mut egui_render_state, &primitives, pixels_per_point);

        Ok(())
    }

    fn wants_pointer_input(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    fn wants_keyboard_input(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// SAFETY: egui::Context is Send+Sync (Arc-based). EguiWgpuRenderer holds only
// wgpu types which are Send+Sync. egui_winit::State requires Send which it
// satisfies on desktop platforms.
unsafe impl Send for EguiOverlay {}
unsafe impl Sync for EguiOverlay {}
