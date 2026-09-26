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

//! Creating the editor overlay and shell on top of the render system.

use super::WgpuRenderSystem;
use khora_core::renderer::RenderError;

impl WgpuRenderSystem {
    /// Creates an [`EguiOverlay`] backed by the current wgpu graphics context.
    ///
    /// Must be called after [`RenderSystem::init`](khora_core::renderer::traits::RenderSystem::init).
    pub fn create_editor_overlay(
        &self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        shader_source: &str,
    ) -> Result<crate::ui::egui::overlay::EguiOverlay, RenderError> {
        let gc = self
            .graphics_context_shared
            .as_ref()
            .ok_or(RenderError::NotInitialized)?
            .lock()
            .map_err(|_| RenderError::Internal("Context lock poisoned".into()))?;

        Ok(crate::ui::egui::overlay::EguiOverlay::new(
            event_loop,
            gc.surface_config.format,
            &gc.device,
            shader_source,
        ))
    }
}

impl WgpuRenderSystem {
    /// Creates an [`EguiOverlay`] **and** an [`EguiEditorShell`] that share
    /// the same `egui::Context`, plus an offscreen viewport target.
    ///
    /// The overlay handles input / rendering while the shell manages the
    /// dock layout, menu bar, toolbar, and panel dispatch. The viewport
    /// target is an offscreen texture used to display the 3D scene inside
    /// an egui panel.
    pub fn create_editor_overlay_and_shell(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        shader_source: &str,
        theme: khora_core::ui::UiTheme,
        viewport_handle: khora_core::ui::editor::viewport_texture::ViewportTextureHandle,
    ) -> Result<
        (
            crate::ui::egui::overlay::EguiOverlay,
            crate::ui::egui::shell::EguiEditorShell,
        ),
        RenderError,
    > {
        let mut overlay = self.create_editor_overlay(event_loop, shader_source)?;

        // Create an offscreen viewport target (initial 800×600).
        let egui_id = self.create_viewport_target(800, 600, &mut overlay)?;

        // Grid + gizmo rendering are owned by the engine-side `GridLane`
        // / `GizmoLane` (under `OverlayAgent`) — no render-strategy
        // pipeline is created on the backend.

        let mut shell = crate::ui::egui::shell::EguiEditorShell::new(overlay.context(), theme);
        shell.register_viewport_texture(viewport_handle, egui_id);

        Ok((overlay, shell))
    }
}
