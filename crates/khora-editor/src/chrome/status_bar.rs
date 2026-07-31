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

//! Status bar — one monospace strip of live facts.
//!
//! Every value here changes every frame, so every value is monospace: numbers
//! stay in their columns instead of jittering as digits change width. The
//! colours are semantic — a red frame time means the frame is late, not that
//! red looked nice there.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_tool_ui::widgets::{
    self,
    paint::{icon, mono},
    status_dot, Health,
};

/// Height of the status bar.
pub const STATUS_HEIGHT: f32 = 26.0;

/// The bottom status strip.
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
        let t = &self.theme;
        let Ok(s) = self.state.lock() else { return };
        let snap = Snapshot::from(&*s);
        drop(s);

        let r = ui.panel_rect();
        ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], t.surface, 0.0);
        ui.paint_line([r[0], r[1]], [widgets::right(r), r[1]], t.border, 1.0);

        let size = t.font_size_caption;
        let y = widgets::text_y(r, size);
        let cy = r[1] + r[3] * 0.5;

        // ── Left: is it alive, and where are we ──
        let mut x = r[0] + 14.0;
        status_dot(ui, t, [x, cy], Health::from_ratio(snap.health()));
        x += 12.0;

        if let Some(branch) = snap.git_branch.as_deref() {
            mono(ui, [x, y], branch, size, t.text_dim);
            x += ui.measure_text(branch, size, FontFamilyHint::Monospace)[0] + 14.0;
            x = vsep(ui, t, x, r);
        }

        // Load: cpu, then the two memory pools.
        let cpu = (snap.cpu_load * 100.0).clamp(0.0, 100.0);
        let cpu_label = format!("cpu {cpu:.0}%");
        mono(
            ui,
            [x, y],
            &cpu_label,
            size,
            if cpu > 70.0 { t.warning } else { t.text_dim },
        );
        x += ui.measure_text(&cpu_label, size, FontFamilyHint::Monospace)[0] + 12.0;

        let heap = format!("heap {:.0}M", snap.memory_used_mb);
        mono(ui, [x, y], &heap, size, t.text_dim);
        x += ui.measure_text(&heap, size, FontFamilyHint::Monospace)[0] + 12.0;

        if snap.vram_mb > 0.0 {
            let vram = format!("vram {:.1}G", snap.vram_mb / 1024.0);
            mono(ui, [x, y], &vram, size, t.text_dim);
        }

        // ── Right: what we are editing, and how fast ──
        let mut rx = widgets::right(r) - 14.0;

        let project = format!(
            "{} · v{}",
            snap.project.as_deref().unwrap_or("untitled"),
            snap.engine_version.as_deref().unwrap_or("dev"),
        );
        rx -= ui.measure_text(&project, size, FontFamilyHint::Monospace)[0];
        mono(ui, [rx, y], &project, size, t.text_muted);
        rx -= 14.0;
        rx = vsep_left(ui, t, rx, r);

        // The frame budget, in the colour of whether we are meeting it.
        let frame = format!("{:.0} fps · {:.1}ms", snap.fps, snap.frame_time_ms);
        let frame_color = if snap.fps > 55.0 {
            t.success
        } else if snap.fps > 30.0 {
            t.warning
        } else {
            t.error
        };
        rx -= ui.measure_text(&frame, size, FontFamilyHint::Monospace)[0];
        mono(ui, [rx, y], &frame, size, frame_color);
        rx -= 8.0;

        icon(
            ui,
            [rx - 13.0, y - 1.0],
            Icon::Zap,
            size + 1.0,
            t.text_disabled,
        );
    }
}

/// A vertical rule; returns the x cursor past it.
fn vsep(ui: &mut dyn UiBuilder, t: &UiTheme, x: f32, r: [f32; 4]) -> f32 {
    ui.paint_line([x, r[1] + 7.0], [x, r[1] + r[3] - 7.0], t.separator, 1.0);
    x + 14.0
}

/// A vertical rule laid out right-to-left; returns the x cursor left of it.
fn vsep_left(ui: &mut dyn UiBuilder, t: &UiTheme, x: f32, r: [f32; 4]) -> f32 {
    ui.paint_line([x, r[1] + 7.0], [x, r[1] + r[3] - 7.0], t.separator, 1.0);
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

impl Snapshot {
    /// How healthy the frame loop is, as a `0..=1` ratio — drives the liveness
    /// dot through the same thresholds every other health indicator uses.
    fn health(&self) -> f32 {
        (self.fps / 60.0).clamp(0.0, 1.0)
    }
}

impl From<&EditorState> for Snapshot {
    fn from(s: &EditorState) -> Self {
        Self {
            fps: s.status.fps,
            frame_time_ms: s.status.frame_time_ms,
            memory_used_mb: s.status.memory_used_mb,
            cpu_load: s.status.cpu_load,
            vram_mb: s.status.vram_mb,
            project: s.project_name.clone(),
            git_branch: s.current_git_branch.clone(),
            engine_version: s.project_engine_version.clone(),
        }
    }
}
