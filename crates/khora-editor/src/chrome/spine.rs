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

//! Spine — vertical mode switcher on the far-left edge of the editor.
//!
//! Six modes, mirrored from the design mockup:
//! Scene, 2D Canvas, Node Graph, Animation, Shader Graph, Control Plane.
//! Currently only `Scene` and `ControlPlane` have dedicated workspaces — the
//! others are placeholders that flip the active mode but still render the
//! Scene workspace under the hood (Phase 5 will hide other modes' content).

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use super::widgets::{paint_diamond_outline, paint_hairline_h};

/// Vertical mode-switcher strip — 56px wide.
pub struct SpinePanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
}

impl SpinePanel {
    /// Creates a new spine.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self { state, theme }
    }

    fn current_mode(&self) -> EditorMode {
        self.state
            .lock()
            .ok()
            .map(|s| s.active_mode)
            .unwrap_or_default()
    }

    fn set_mode(&self, mode: EditorMode) {
        if let Ok(mut s) = self.state.lock() {
            s.active_mode = mode;
        }
    }
}

const MODES: &[(EditorMode, &str, &str)] = &[
    (EditorMode::Scene, "▣", "Scene"),
    (EditorMode::Canvas2D, "▤", "2D Canvas"),
    (EditorMode::NodeGraph, "▥", "Node Graph"),
    (EditorMode::Animation, "▦", "Animation"),
    (EditorMode::Shader, "✦", "Shader"),
    (EditorMode::ControlPlane, "◉", "Control Plane"),
];

impl EditorPanel for SpinePanel {
    fn id(&self) -> &str {
        "khora.editor.spine"
    }

    fn title(&self) -> &str {
        "Spine"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(56.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // ── Background ──────────────────────────────
        let rect = ui.panel_rect();
        let [px, py, pw, ph] = rect;
        ui.paint_rect_filled([px, py], [pw, ph], self.theme.background, 0.0);
        // Right hairline separator
        ui.paint_line(
            [px + pw, py],
            [px + pw, py + ph],
            self.theme.separator,
            1.0,
        );

        // ── Brand block (top diamond) ───────────────
        let brand_cy = py + 26.0;
        let brand_cx = px + pw * 0.5;
        ui.paint_rect_filled(
            [brand_cx - 14.0, brand_cy - 14.0],
            [28.0, 28.0],
            self.theme.surface_active,
            6.0,
        );
        paint_diamond_outline(ui, brand_cx, brand_cy, 9.0, self.theme.primary, 1.5);

        // Hairline divider under brand
        paint_hairline_h(
            ui,
            px + 14.0,
            py + 50.0,
            pw - 28.0,
            self.theme.separator,
        );

        // ── Mode buttons ────────────────────────────
        let current = self.current_mode();
        ui.spacing(56.0); // push past the brand block

        ui.vertical(&mut |ui| {
            for (mode, glyph, label) in MODES {
                let active = current == *mode;
                ui.horizontal(&mut |ui| {
                    let label_text = format!(" {} ", glyph);
                    if ui.selectable_label(active, &label_text) {
                        self.set_mode(*mode);
                    }
                    // Tooltip via hover
                    if ui.is_last_item_hovered() {
                        ui.colored_label(self.theme.text_dim, label);
                    }
                });
                ui.spacing(2.0);
            }
        });
    }
}
