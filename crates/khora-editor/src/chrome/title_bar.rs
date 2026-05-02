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

//! Title bar — branded top strip with menu, search and brand pill.
//!
//! Replaces the legacy `MenuBarPanel`. The menu lives inline inside the title
//! bar, alongside the brand pill (logo + project name) and the Cmd+K search
//! affordance that opens the command palette in Phase 4.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use super::widgets::{paint_diamond_filled, paint_vertical_gradient, with_alpha};

/// Top-bar branded strip — height 44px.
pub struct TitleBarPanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
}

impl TitleBarPanel {
    /// Creates a new title bar.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self { state, theme }
    }

    fn dispatch(&self, action: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.pending_menu_action = Some(action.to_owned());
        }
    }

    fn open_command_palette(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.command_palette_open = true;
        }
    }
}

impl EditorPanel for TitleBarPanel {
    fn id(&self) -> &str {
        "khora.editor.title_bar"
    }

    fn title(&self) -> &str {
        "Title Bar"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(44.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let project_name = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_name.clone())
            .unwrap_or_else(|| "untitled".to_owned());

        // ── Background gradient ──────────────────────
        let rect = ui.panel_rect();
        paint_vertical_gradient(
            ui,
            rect,
            self.theme.surface_elevated,
            self.theme.surface,
            6,
        );
        // Bottom hairline separator
        let [x, y, w, h] = rect;
        ui.paint_line(
            [x, y + h],
            [x + w, y + h],
            with_alpha(self.theme.separator, 0.55),
            1.0,
        );

        // ── Brand mark (filled diamond at left) ─────
        let pill_y = y + h * 0.5;
        let mark_cx = x + 18.0;
        paint_diamond_filled(ui, mark_cx, pill_y, 7.0, self.theme.primary);

        // ── Inline content ──────────────────────────
        ui.horizontal(&mut |ui| {
            ui.spacing(34.0); // leave room for the brand mark
            ui.colored_label(self.theme.text, "KhoraEngine");
            ui.colored_label(self.theme.text_muted, "·");
            ui.colored_label(self.theme.text_dim, &project_name);
            ui.spacing(16.0);

            // ── Menu inline ─────────────────────────
            ui.menu_button("File", &mut |ui| {
                if ui.button("New Scene") {
                    self.dispatch("new_scene");
                    ui.close_menu();
                }
                if ui.button("Open…") {
                    self.dispatch("open");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Save") {
                    self.dispatch("save");
                    ui.close_menu();
                }
                if ui.button("Save As…") {
                    self.dispatch("save_as");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit") {
                    self.dispatch("quit");
                    ui.close_menu();
                }
            });

            ui.menu_button("Edit", &mut |ui| {
                if ui.button("Undo  (Ctrl+Z)") {
                    self.dispatch("undo");
                    ui.close_menu();
                }
                if ui.button("Redo  (Ctrl+Y)") {
                    self.dispatch("redo");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Delete  (Del)") {
                    self.dispatch("delete");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Preferences…") {
                    self.dispatch("preferences");
                    ui.close_menu();
                }
            });

            ui.menu_button("View", &mut |ui| {
                if ui.button("Reset Layout") {
                    self.dispatch("reset_layout");
                    ui.close_menu();
                }
            });

            ui.menu_button("Help", &mut |ui| {
                if ui.button("Documentation") {
                    self.dispatch("documentation");
                    ui.close_menu();
                }
                if ui.button("About Khora Engine") {
                    self.dispatch("about");
                    ui.close_menu();
                }
            });

            // ── Spacer ──────────────────────────────
            // egui's horizontal lays out left-to-right; we approximate the
            // spacer by reserving the remaining width before the right cluster.
            let remaining = ui.available_width();
            if remaining > 380.0 {
                ui.spacing(remaining - 360.0);
            }

            // ── Command palette trigger (Cmd+K) ─────
            if ui.button("🔍  Search commands, assets, entities…  ⌘K") {
                self.open_command_palette();
            }
        });
    }
}
