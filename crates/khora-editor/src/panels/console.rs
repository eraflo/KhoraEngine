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

use std::sync::{Arc, Mutex};

use khora_core::ui::editor::*;
use khora_sdk::prelude::*;

pub struct ConsolePanel {
    state: Arc<Mutex<EditorState>>,
    show_info: bool,
    show_warn: bool,
    show_error: bool,
    show_debug: bool,
    filter_text: String,
}

impl ConsolePanel {
    pub fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self {
            state,
            show_info: true,
            show_warn: true,
            show_error: true,
            show_debug: false,
            filter_text: String::new(),
        }
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
        ui.horizontal(&mut |ui| {
            ui.checkbox(&mut self.show_error, "\u{274C} Error");
            ui.checkbox(&mut self.show_warn, "\u{26A0} Warn");
            ui.checkbox(&mut self.show_info, "\u{2139} Info");
            ui.checkbox(&mut self.show_debug, "\u{1F41B} Debug");
        });
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut self.filter_text);
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
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "No log entries.");
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
                    LogLevel::Error => ([1.0, 0.3, 0.3, 1.0], "[ERROR]"),
                    LogLevel::Warn => ([1.0, 0.8, 0.2, 1.0], "[WARN] "),
                    LogLevel::Info => ([0.8, 0.8, 0.8, 1.0], "[INFO] "),
                    LogLevel::Debug => ([0.5, 0.7, 1.0, 1.0], "[DEBUG]"),
                    LogLevel::Trace => ([0.5, 0.5, 0.5, 1.0], "[TRACE]"),
                };

                let line = format!("{} {}: {}", prefix, entry.target, entry.message);
                ui.colored_label(color, &line);
            }
        });
    }
}
