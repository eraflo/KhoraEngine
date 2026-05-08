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

//! Status bar — branded bottom strip with mono metrics + colored dots.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::brand::paint_diamond_filled;
use crate::widgets::chrome::paint_status_dot;
use crate::widgets::paint::{paint_icon, with_alpha};

const STATUS_HEIGHT: f32 = 24.0;

/// Branded status bar — 24px tall.
pub struct StatusBarPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl StatusBarPanel {
    /// Creates a new status bar.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
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
        Some(STATUS_HEIGHT)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = &self.theme;
        let snapshot = match self.state.lock() {
            Ok(s) => Snapshot {
                fps: s.status.fps,
                frame_time_ms: s.status.frame_time_ms,
                memory_used_mb: s.status.memory_used_mb,
                cpu_load: s.status.cpu_load,
                vram_mb: s.status.vram_mb,
                project: s.project_name.clone(),
                git_branch: s.current_git_branch.clone(),
                engine_version: s.project_engine_version.clone(),
            },
            Err(_) => return,
        };

        let rect = ui.panel_rect();
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, 0.0);
        ui.paint_line([x, y], [x + w, y], with_alpha(theme.separator, 0.55), 1.0);

        let cy = y + h * 0.5;
        let text_y = y + (h - 11.5) * 0.5;
        let label_color = theme.text_dim;

        // ── Left cluster ──────────────────────────────
        let mut cursor = x + 14.0;
        // Brand mark
        paint_diamond_filled(ui, cursor, cy, 4.0, theme.primary);
        cursor += 14.0;
        // Ready dot + label
        paint_status_dot(ui, [cursor, cy], theme.success);
        cursor += 12.0;
        ui.paint_text_styled(
            [cursor, text_y],
            "Ready",
            11.0,
            theme.text,
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        cursor += 44.0;

        // Git branch — only painted if available. Wired in Phase 2.5.
        if let Some(branch) = snapshot.git_branch.as_deref() {
            cursor = vsep(ui, cursor, y, h, theme);
            paint_icon(ui, [cursor, text_y - 1.0], Icon::Branch, 12.0, label_color);
            cursor += 16.0;
            ui.paint_text_styled(
                [cursor, text_y],
                branch,
                11.0,
                label_color,
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            let advance =
                ui.measure_text(branch, 11.0, FontFamilyHint::Monospace)[0].max(40.0) + 12.0;
            cursor += advance;
        }
        let _ = cursor;

        // ── Right cluster ─────────────────────────────
        let project_label = format!(
            "{} · Khora v{}",
            snapshot.project.as_deref().unwrap_or("untitled"),
            snapshot.engine_version.as_deref().unwrap_or("dev"),
        );
        let mut rx = x + w - 14.0;
        // Version + project right-aligned
        ui.paint_text_styled(
            [rx, text_y],
            &project_label,
            11.0,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        rx -= ui.measure_text(&project_label, 11.0, FontFamilyHint::Monospace)[0] + 14.0;
        rx = vsep_right(ui, rx, y, h, theme);

        // CPU — real value (Phase 2.1) coloured amber if hot
        let cpu_pct = (snapshot.cpu_load * 100.0).clamp(0.0, 100.0);
        let cpu_color = if cpu_pct > 70.0 {
            theme.warning
        } else {
            label_color
        };
        let cpu_label = format!("{:>3.0}%", cpu_pct);
        ui.paint_text_styled(
            [rx, text_y],
            &cpu_label,
            11.0,
            cpu_color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        rx -= ui.measure_text(&cpu_label, 11.0, FontFamilyHint::Monospace)[0] + 6.0;
        paint_icon(ui, [rx - 14.0, text_y - 1.0], Icon::Cpu, 12.0, label_color);
        rx -= 26.0;
        rx = vsep_right(ui, rx, y, h, theme);

        // VRAM (only if known)
        if snapshot.vram_mb > 0.0 {
            let vram_label = format!("{:.1} GB", snapshot.vram_mb / 1024.0);
            let vram_w = ui.measure_text(&vram_label, 11.0, FontFamilyHint::Monospace)[0];
            ui.paint_text_styled(
                [rx, text_y],
                &vram_label,
                11.0,
                label_color,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
            rx -= vram_w + 4.0;
            paint_icon(
                ui,
                [rx - 14.0, text_y - 1.0],
                Icon::Image,
                12.0,
                label_color,
            );
            rx -= 26.0;
            rx = vsep_right(ui, rx, y, h, theme);
        }

        // RAM heap
        let mem_label = format!("{:.0} MB", snapshot.memory_used_mb);
        let mem_w = ui.measure_text(&mem_label, 11.0, FontFamilyHint::Monospace)[0];
        ui.paint_text_styled(
            [rx, text_y],
            &mem_label,
            11.0,
            label_color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        rx -= mem_w + 4.0;
        paint_icon(
            ui,
            [rx - 14.0, text_y - 1.0],
            Icon::Memory,
            12.0,
            label_color,
        );
        rx -= 26.0;
        rx = vsep_right(ui, rx, y, h, theme);

        // Frame
        let frame_label = format!("{:>5.2} ms", snapshot.frame_time_ms);
        ui.paint_text_styled(
            [rx, text_y],
            &frame_label,
            11.0,
            label_color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        rx -= ui.measure_text(&frame_label, 11.0, FontFamilyHint::Monospace)[0] + 12.0;
        rx = vsep_right(ui, rx, y, h, theme);

        // FPS (greenish if good)
        let fps_color = if snapshot.fps > 55.0 {
            theme.success
        } else if snapshot.fps > 30.0 {
            theme.warning
        } else {
            theme.error
        };
        let fps_label = format!("{:>5.1} fps", snapshot.fps);
        ui.paint_text_styled(
            [rx, text_y],
            &fps_label,
            11.0,
            fps_color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );

        let _ = vsep_right; // keep helper available
    }
}

fn vsep(ui: &mut dyn UiBuilder, x: f32, y: f32, h: f32, theme: &UiTheme) -> f32 {
    ui.paint_line(
        [x, y + 6.0],
        [x, y + h - 6.0],
        with_alpha(theme.separator, 0.55),
        1.0,
    );
    x + 14.0
}

fn vsep_right(ui: &mut dyn UiBuilder, x: f32, y: f32, h: f32, theme: &UiTheme) -> f32 {
    ui.paint_line(
        [x, y + 6.0],
        [x, y + h - 6.0],
        with_alpha(theme.separator, 0.55),
        1.0,
    );
    x - 14.0
}

struct Snapshot {
    fps: f32,
    frame_time_ms: f32,
    memory_used_mb: f32,
    cpu_load: f32,
    vram_mb: f32,
    project: Option<String>,
    git_branch: Option<String>,
    engine_version: Option<String>,
}
