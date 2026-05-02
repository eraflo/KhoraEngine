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

//! Console panel — Phase H: filter pills with colored dots, log row grid
//! (time / icon / source / message), bg-tinted error/warn rows.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::chrome::{paint_panel_header, paint_status_dot, panel_tab};
use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

const HEADER_HEIGHT: f32 = 34.0;
const FILTER_HEIGHT: f32 = 28.0;

pub struct ConsolePanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
    show_info: bool,
    show_warn: bool,
    show_error: bool,
    show_debug: bool,
    filter_text: String,
}

impl ConsolePanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self {
            state,
            theme,
            show_info: true,
            show_warn: true,
            show_error: true,
            show_debug: false,
            filter_text: String::new(),
        }
    }
}

fn filter_chip(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    label: &str,
    count: usize,
    color: [f32; 4],
    active: bool,
    theme: &EditorTheme,
    id_salt: &str,
) -> (bool, f32) {
    let label_w = ui.measure_text(label, 11.0, FontFamilyHint::Proportional)[0];
    let count_str = format!("{}", count);
    let count_w = ui.measure_text(&count_str, 10.0, FontFamilyHint::Monospace)[0];
    let pad = 10.0;
    let w = pad * 2.0 + 16.0 + label_w + count_w + 6.0;
    let h = 22.0;

    if active {
        ui.paint_rect_filled(
            origin,
            [w, h],
            theme.surface_active,
            999.0,
        );
        ui.paint_rect_stroke(
            origin,
            [w, h],
            with_alpha(theme.separator, 0.6),
            999.0,
            1.0,
        );
    }
    paint_status_dot(ui, [origin[0] + 10.0, origin[1] + h * 0.5], color);
    paint_text_size(
        ui,
        [origin[0] + 22.0, origin[1] + 5.0],
        label,
        11.0,
        if active { theme.text } else { theme.text_dim },
    );
    ui.paint_text_styled(
        [origin[0] + 22.0 + label_w + 8.0, origin[1] + 6.5],
        &count_str,
        10.0,
        theme.text_muted,
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );

    let interaction = ui.interact_rect(id_salt, [origin[0], origin[1], w, h]);
    (interaction.clicked, w)
}

impl EditorPanel for ConsolePanel {
    fn id(&self) -> &str {
        "console"
    }
    fn title(&self) -> &str {
        "Console"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, ph] = panel_rect;

        // ── Header ────────────────────────────────────
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;
        let entries = self
            .state
            .lock()
            .ok()
            .map(|s| s.log_entries.clone())
            .unwrap_or_default();
        let count_total = entries.len();
        let count_info = entries.iter().filter(|e| e.level == LogLevel::Info).count();
        let count_warn = entries.iter().filter(|e| e.level == LogLevel::Warn).count();
        let count_err = entries.iter().filter(|e| e.level == LogLevel::Error).count();

        let _ = panel_tab(
            ui,
            "c-tab-console",
            [px + 6.0, tab_y],
            "Console",
            Some(&format!("{}", count_total)),
            true,
            &theme,
        );

        // ── Filter chips ──────────────────────────────
        let chip_y = py + HEADER_HEIGHT + 4.0;
        let mut cx = px + 8.0;
        let chip_specs: [(&str, usize, [f32; 4], bool, &str); 4] = [
            ("Errors", count_err, theme.error, self.show_error, "c-chip-err"),
            ("Warnings", count_warn, theme.warning, self.show_warn, "c-chip-warn"),
            ("Info", count_info, theme.accent_b, self.show_info, "c-chip-info"),
            ("Debug", 0, theme.text_muted, self.show_debug, "c-chip-debug"),
        ];
        let mut new_show_err = self.show_error;
        let mut new_show_warn = self.show_warn;
        let mut new_show_info = self.show_info;
        let mut new_show_debug = self.show_debug;
        for (i, (label, count, color, active, salt)) in chip_specs.iter().enumerate() {
            let (clicked, w) = filter_chip(ui, [cx, chip_y], label, *count, *color, *active, &theme, salt);
            if clicked {
                match i {
                    0 => new_show_err = !new_show_err,
                    1 => new_show_warn = !new_show_warn,
                    2 => new_show_info = !new_show_info,
                    3 => new_show_debug = !new_show_debug,
                    _ => {}
                }
            }
            cx += w + 4.0;
        }
        self.show_error = new_show_err;
        self.show_warn = new_show_warn;
        self.show_info = new_show_info;
        self.show_debug = new_show_debug;

        // Search filter (right side) — same simplification as asset browser.
        let filter_w = 200.0_f32.min(pw * 0.3);
        let filter_x = px + pw - filter_w - 12.0;
        let filter_y = chip_y;
        paint_icon(
            ui,
            [filter_x + 4.0, filter_y + 5.0],
            Icon::Search,
            12.0,
            theme.text_muted,
        );
        let filter_ref = &mut self.filter_text;
        ui.region_at(
            [filter_x + 20.0, filter_y, filter_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(filter_ref);
            },
        );

        // ── Log rows ──────────────────────────────────
        let body_y = py + HEADER_HEIGHT + FILTER_HEIGHT + 6.0;
        let body_h = (ph - HEADER_HEIGHT - FILTER_HEIGHT - 8.0).max(0.0);
        let body_rect = [px, body_y, pw, body_h];

        let theme_clone = theme.clone();
        let show_info = self.show_info;
        let show_warn = self.show_warn;
        let show_error = self.show_error;
        let show_debug = self.show_debug;
        let filter_lower = self.filter_text.to_lowercase();

        ui.region_at(body_rect, &mut |ui_inner| {
            ui_inner.scroll_area("console_scroll", &mut |ui_s| {
                if entries.is_empty() {
                    ui_s.colored_label(theme_clone.text_muted, "No log entries.");
                    return;
                }
                for entry in entries.iter().rev().take(500) {
                    let show = match entry.level {
                        LogLevel::Error => show_error,
                        LogLevel::Warn => show_warn,
                        LogLevel::Info => show_info,
                        LogLevel::Debug | LogLevel::Trace => show_debug,
                    };
                    if !show {
                        continue;
                    }
                    if !filter_lower.is_empty()
                        && !entry.message.to_lowercase().contains(&filter_lower)
                        && !entry.target.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                    let (color, prefix) = match entry.level {
                        LogLevel::Error => (theme_clone.error, "[ERROR]"),
                        LogLevel::Warn => (theme_clone.warning, "[WARN] "),
                        LogLevel::Info => (theme_clone.accent_b, "[INFO] "),
                        LogLevel::Debug => (theme_clone.text_dim, "[DEBUG]"),
                        LogLevel::Trace => (theme_clone.text_muted, "[TRACE]"),
                    };
                    ui_s.horizontal(&mut |ui_h| {
                        ui_h.colored_label(color, prefix);
                        ui_h.colored_label(theme_clone.primary, &format!("[{}]", entry.target));
                        ui_h.colored_label(theme_clone.text, &entry.message);
                    });
                }
            });
        });
    }
}
