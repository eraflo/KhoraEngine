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

//! The editor while it starts: the door film over the shell, painted with the
//! overlay alone until the engine is up, then fading out over the editor.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::ui::{EditorOverlay, OverlayScreenDescriptor};
use khora_sdk::{winit, EditorShell, RenderSystem, Runtime};

use super::EditorApp;
use crate::panels::loading::LoadingPanel;

impl EditorApp {
    /// The editor is starting: the shell gets the brand theme and typefaces,
    /// and the loading screen goes over it.
    pub(super) fn begin_startup(&mut self, runtime: &Runtime) {
        self.overlay = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn EditorOverlay>>>>()
            .cloned();
        self.raw_window = runtime
            .resources
            .get::<Arc<winit::window::Window>>()
            .cloned();
        let Some(shell_ref) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn EditorShell>>>>()
            .cloned()
        else {
            log::warn!("EditorApp: no EditorShell to show the loading screen on.");
            return;
        };
        if let Ok(mut shell) = shell_ref.lock() {
            self.install_brand(&mut **shell);
            let panel = LoadingPanel::new(0.0, khora_tool_ui::khora_dark());
            self.loading = Some(panel.handle());
            shell.register_panel(panel.location(), Box::new(panel));
        }
        self.shell = Some(shell_ref);
    }

    /// The brand theme and typefaces on `shell`, once.
    pub(super) fn install_brand(&mut self, shell: &mut dyn EditorShell) {
        if self.brand_installed {
            return;
        }
        shell.set_theme(khora_tool_ui::khora_dark());
        shell.set_fonts(khora_tool_ui::fonts::brand_fonts());
        self.brand_installed = true;
    }

    /// One frame while the engine starts: the shell — whose cover is the
    /// loading screen — painted by the overlay straight onto the window.
    pub(super) fn paint_loading_frame(&mut self, runtime: &Runtime) {
        let (Some(overlay_arc), Some(raw_window)) = (self.overlay.clone(), self.raw_window.clone())
        else {
            return;
        };
        let size = raw_window.inner_size();
        let screen = OverlayScreenDescriptor {
            width_px: size.width,
            height_px: size.height,
            scale_factor: raw_window.scale_factor() as f32,
        };
        if let Ok(mut overlay) = overlay_arc.lock() {
            overlay.begin_frame((&*raw_window) as &dyn std::any::Any, screen);
        }
        if let Some(shell) = self.shell.as_ref() {
            if let Ok(mut shell) = shell.lock() {
                shell.show_frame();
            }
        }
        let Some(rs_arc) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .cloned()
        else {
            return;
        };
        let (Ok(mut rs), Ok(mut overlay)) = (rs_arc.lock(), overlay_arc.lock()) else {
            return;
        };
        let overlay_ref: &mut dyn EditorOverlay = &mut **overlay;
        if let Err(e) = rs.render_overlay(overlay_ref, screen) {
            log::error!("editor: loading frame overlay failed: {e:?}");
        }
    }

    /// The engine is up: the loading screen plays on to the end of its
    /// chapter, then fades out.
    pub(super) fn end_startup(&mut self) {
        if let Some(loading) = self.loading.as_ref() {
            loading.ready();
        }
    }

    /// Takes the loading screen off the shell once it has faded out.
    pub(super) fn drop_finished_loading_screen(&mut self) {
        if !self.loading.as_ref().is_some_and(|l| l.is_done()) {
            return;
        }
        if let Some(shell) = self.shell.as_ref() {
            if let Ok(mut shell) = shell.lock() {
                shell.remove_panel(LoadingPanel::ID);
            }
        }
        self.loading = None;
    }
}
