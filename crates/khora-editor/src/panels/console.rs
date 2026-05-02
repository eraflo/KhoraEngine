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

//! Console panel — displays captured log entries with level filtering.
//!
//! Phase 3: themed log levels, monospaced rows, branded filter chips.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

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

/// Renders a labelled "filter pill" toggle. Free function so callers can use
/// it inside a closure that already mutably borrows `self`.
fn filter_pill(ui: &mut dyn UiBuilder, label: &str, active: &mut bool) {
    let glyph = if *active { "●" } else { "○" };
    let text = format!("{} {}", glyph, label);
    if ui.selectable_label(*active, &text) {
        *active = !*active;
    }
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

        // ── Filter pills + search ─────────────────────
        let show_info_ref = &mut self.show_info;
        let show_warn_ref = &mut self.show_warn;
        let show_error_ref = &mut self.show_error;
        let show_debug_ref = &mut self.show_debug;
        let filter_text_ref = &mut self.filter_text;
        let theme_ref = &theme;

        ui.horizontal(&mut |ui| {
            filter_pill(ui, "Errors", show_error_ref);
            filter_pill(ui, "Warnings", show_warn_ref);
            filter_pill(ui, "Info", show_info_ref);
            filter_pill(ui, "Debug", show_debug_ref);
            ui.spacing(12.0);
            ui.colored_label(theme_ref.text_dim, "🔍");
            ui.text_edit_singleline(filter_text_ref);
        });
        ui.separator();

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let filter_lower = self.filter_text.to_lowercase();
        let show_info = self.show_info;
        let show_warn = self.show_warn;
        let show_error = self.show_error;
        let show_debug = self.show_debug;

        let entries: Vec<LogEntry> = state.log_entries.clone();
        drop(state);

        ui.scroll_area("console_scroll", &mut |ui| {
            if entries.is_empty() {
                ui.colored_label(theme.text_muted, "No log entries.");
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
                    LogLevel::Error => (theme.error, "[ERROR]"),
                    LogLevel::Warn => (theme.warning, "[WARN] "),
                    LogLevel::Info => (theme.accent_b, "[INFO] "),
                    LogLevel::Debug => (theme.text_dim, "[DEBUG]"),
                    LogLevel::Trace => (theme.text_muted, "[TRACE]"),
                };

                ui.horizontal(&mut |ui| {
                    ui.colored_label(color, prefix);
                    ui.colored_label(theme.primary, &format!("[{}]", entry.target));
                    ui.colored_label(theme.text, &entry.message);
                });
            }
        });
    }
}
