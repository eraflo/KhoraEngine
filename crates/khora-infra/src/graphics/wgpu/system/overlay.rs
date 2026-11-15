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
    /// Creates an [`EguiOverlay`](crate::ui::egui::overlay::EguiOverlay) on the
    /// current graphics context.
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

    /// Creates the editor overlay and shell, sharing one `egui::Context`, plus
    /// the offscreen target the viewport panel shows.
    ///
    /// The overlay is an [`EguiOverlay`](crate::ui::egui::overlay::EguiOverlay),
    /// the shell an [`EguiEditorShell`](crate::ui::egui::shell::EguiEditorShell).
    /// Must be called after [`RenderSystem::init`](khora_core::renderer::traits::RenderSystem::init).
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

        let egui_id = self.create_viewport_target(800, 600, &mut overlay)?;

        let mut shell = crate::ui::egui::shell::EguiEditorShell::new(overlay.context(), theme);
        shell.register_viewport_texture(viewport_handle, egui_id);

        Ok((overlay, shell))
    }
}
