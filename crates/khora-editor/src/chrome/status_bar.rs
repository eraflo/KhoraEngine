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

//! Status bar panel — branded bottom strip with FPS, memory, project info.
//!
//! Phase 2 redesign: monospaced metrics + colored status dots, matching the
//! "Deep Navy" mockup. The bar is 24px tall and sits below the resizable
//! `Bottom` slot (asset browser / console).

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use super::widgets::{paint_diamond_filled, with_alpha};

/// Branded status bar — 24px tall.
pub struct StatusBarPanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
}

impl StatusBarPanel {
    /// Creates a new status bar.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self { state, theme }
    }
}

impl EditorPanel for StatusBarPanel {
    fn id(&self) -> &str {
        "khora.editor.status_bar"
    }

    fn title(&self) -> &str {
        "Status Bar"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(24.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let snapshot = match self.state.lock() {
            Ok(s) => Snapshot {
                fps: s.status.fps,
                frame_time_ms: s.status.frame_time_ms,
                entity_count: s.status.entity_count,
                memory_used_mb: s.status.memory_used_mb,
                gizmo: s.gizmo_mode,
                project: s.project_name.clone(),
            },
            Err(_) => return,
        };

        // ── Background + top hairline ──────────────
        let rect = ui.panel_rect();
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], self.theme.surface, 0.0);
        ui.paint_line(
            [x, y],
            [x + w, y],
            with_alpha(self.theme.separator, 0.55),
            1.0,
        );

        // ── Brand mark (bottom-left) ───────────────
        let mark_cy = y + h * 0.5;
        paint_diamond_filled(ui, x + 12.0, mark_cy, 4.0, self.theme.primary);

        // ── Inline metrics ─────────────────────────
        let gizmo_label = match snapshot.gizmo {
            GizmoMode::Select => "SEL",
            GizmoMode::Move => "MOV",
            GizmoMode::Rotate => "ROT",
            GizmoMode::Scale => "SCA",
        };

        let project_text = snapshot
            .project
            .as_deref()
            .unwrap_or("untitled")
            .to_string();

        ui.horizontal(&mut |ui| {
            ui.spacing(24.0); // past the brand mark
            // Ready dot + label
            paint_dot(ui, self.theme.success);
            ui.colored_label(self.theme.text, " Ready");
            sep(ui, &self.theme);

            // Branch
            ui.colored_label(self.theme.text_dim, "⎇ dev");
            sep(ui, &self.theme);

            // Compile pulse (placeholder)
            paint_dot(ui, self.theme.warning);
            ui.colored_label(self.theme.text_dim, " idle");
            sep(ui, &self.theme);

            // Tool mode
            ui.colored_label(self.theme.primary, gizmo_label);
            sep(ui, &self.theme);

            // FPS / frame
            ui.monospace(&format!("{:>5.1} fps", snapshot.fps));
            sep(ui, &self.theme);
            ui.colored_label(
                self.theme.text_dim,
                &format!("{:>5.2} ms", snapshot.frame_time_ms),
            );
            sep(ui, &self.theme);

            // Entities
            ui.colored_label(
                self.theme.text_dim,
                &format!("{} ent", snapshot.entity_count),
            );
            sep(ui, &self.theme);

            // Memory
            ui.colored_label(
                self.theme.text_dim,
                &format!("{:>4.0} MB", snapshot.memory_used_mb),
            );
            sep(ui, &self.theme);

            // Project + version (right side approximated via spacing)
            let remaining = ui.available_width();
            let right_text = format!("{} · Khora v0.1", project_text);
            // crude right-alignment: pad with spaces if there's room
            let pad = (remaining - 8.0 * right_text.len() as f32 - 16.0).max(8.0);
            ui.spacing(pad);
            ui.colored_label(self.theme.text_muted, &right_text);
        });
    }
}

fn paint_dot(ui: &mut dyn UiBuilder, color: [f32; 4]) {
    // We don't have circle painting in UiBuilder — use a tiny rounded rect.
    // Position it relative to the current widget cursor by measuring panel
    // rect first. To keep things simple, draw inline via a colored "●" glyph.
    ui.colored_label(color, "●");
}

fn sep(ui: &mut dyn UiBuilder, theme: &EditorTheme) {
    ui.colored_label(theme.text_disabled, "│");
}

struct Snapshot {
    fps: f32,
    frame_time_ms: f32,
    entity_count: usize,
    memory_used_mb: f32,
    gizmo: GizmoMode,
    project: Option<String>,
}
