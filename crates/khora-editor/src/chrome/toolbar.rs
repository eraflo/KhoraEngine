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
//
//! Toolbar panel — gizmo mode selector + play / pause / stop transport.
//!
//! Phase 0: visually plain, functionally identical to the previous toolbar
//! that lived inside `EguiEditorShell`. The redesign (brand pill, search,
//! right-aligned transport, etc.) is delivered in Phase 2.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

/// Top-bar tool strip with gizmo and transport buttons.
pub struct ToolbarPanel {
    state: Arc<Mutex<EditorState>>,
}

impl ToolbarPanel {
    /// Creates a new toolbar bound to the shared [`EditorState`].
    pub fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self { state }
    }
}

impl EditorPanel for ToolbarPanel {
    fn id(&self) -> &str {
        "khora.editor.toolbar"
    }

    fn title(&self) -> &str {
        "Toolbar"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(32.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let (current_gizmo, play_mode) = match self.state.lock() {
            Ok(s) => (s.gizmo_mode, s.play_mode),
            Err(_) => return,
        };

        ui.horizontal(&mut |ui| {
            ui.spacing(8.0);
            ui.label("Khora");
            ui.separator();

            // ── Gizmo mode buttons ────────────────────
            if ui.selectable_label(current_gizmo == GizmoMode::Select, "Select") {
                if let Ok(mut s) = self.state.lock() {
                    s.gizmo_mode = GizmoMode::Select;
                }
            }
            if ui.selectable_label(current_gizmo == GizmoMode::Move, "Move") {
                if let Ok(mut s) = self.state.lock() {
                    s.gizmo_mode = GizmoMode::Move;
                }
            }
            if ui.selectable_label(current_gizmo == GizmoMode::Rotate, "Rotate") {
                if let Ok(mut s) = self.state.lock() {
                    s.gizmo_mode = GizmoMode::Rotate;
                }
            }
            if ui.selectable_label(current_gizmo == GizmoMode::Scale, "Scale") {
                if let Ok(mut s) = self.state.lock() {
                    s.gizmo_mode = GizmoMode::Scale;
                }
            }

            ui.separator();

            // ── Transport (Play / Pause / Stop) ───────
            let is_editing = play_mode == PlayMode::Editing;
            let is_playing = play_mode == PlayMode::Playing;
            let is_paused = play_mode == PlayMode::Paused;

            // Play / Resume button
            let play_label = if is_paused { "▶ Resume" } else { "▶ Play" };
            if (is_editing || is_paused) && ui.button(play_label) {
                if let Ok(mut s) = self.state.lock() {
                    s.pending_menu_action = Some("play".to_owned());
                }
            }
            if is_playing && ui.button("⏸ Pause") {
                if let Ok(mut s) = self.state.lock() {
                    s.pending_menu_action = Some("pause".to_owned());
                }
            }
            if (is_playing || is_paused) && ui.button("⏹ Stop") {
                if let Ok(mut s) = self.state.lock() {
                    s.pending_menu_action = Some("stop".to_owned());
                }
            }
        });
    }
}
