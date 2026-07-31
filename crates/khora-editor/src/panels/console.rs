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

//! Console — the engine talking.
//!
//! Rows are a fixed grid (time · level · source · message) so the eye can scan
//! one column without reading the others. Warn and error rows are tinted, but
//! the level is *also* an icon: colour alone would leave the severity invisible
//! to a colour-blind reader.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_tool_ui::widgets::{
    self, empty_state, filter_pill,
    paint::{fill, icon, mono, text},
    search_field, Pill, Tone,
};

const HEADER_H: f32 = 34.0;
const FILTER_H: f32 = 30.0;
const ROW_H: f32 = 17.0;

/// Column geometry. The message column takes whatever is left and *wraps*;
/// it never widens the row, because a console that scrolls sideways is a
/// console you cannot read.
const COL_TIME: f32 = 58.0;
const COL_LEVEL: f32 = 16.0;
const COL_SOURCE: f32 = 122.0;
const GAP: f32 = 9.0;

/// Which log levels the console is currently showing.
///
/// A plain value type, separate from the panel, so the toggle logic can be
/// tested without a UI at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelFilter {
    /// Show `Info` entries.
    pub info: bool,
    /// Show `Warn` entries.
    pub warn: bool,
    /// Show `Error` entries.
    pub error: bool,
    /// Show `Debug` and `Trace` entries.
    pub debug: bool,
}

impl Default for LevelFilter {
    /// Errors, warnings and info on; debug off. Debug is noise until you go
    /// looking for it.
    fn default() -> Self {
        Self {
            info: true,
            warn: true,
            error: true,
            debug: false,
        }
    }
}

impl LevelFilter {
    /// Whether an entry of this level passes the filter.
    pub fn admits(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Error => self.error,
            LogLevel::Warn => self.warn,
            LogLevel::Info => self.info,
            LogLevel::Debug | LogLevel::Trace => self.debug,
        }
    }

    /// Flips one level on or off.
    pub fn toggle(&mut self, level: LogLevel) {
        let slot = match level {
            LogLevel::Error => &mut self.error,
            LogLevel::Warn => &mut self.warn,
            LogLevel::Info => &mut self.info,
            LogLevel::Debug | LogLevel::Trace => &mut self.debug,
        };
        *slot = !*slot;
    }
}

/// Counts per level, for the pill badges.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    info: usize,
    warn: usize,
    error: usize,
    debug: usize,
}

impl Counts {
    fn of(entries: &[LogEntry]) -> Self {
        let mut c = Self::default();
        for e in entries {
            match e.level {
                LogLevel::Error => c.error += 1,
                LogLevel::Warn => c.warn += 1,
                LogLevel::Info => c.info += 1,
                LogLevel::Debug | LogLevel::Trace => c.debug += 1,
            }
        }
        c
    }
}

/// The console panel.
pub struct ConsolePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    filter: LevelFilter,
    search: String,
    /// How far the log is scrolled. Owned by the panel because the rows are
    /// painted in absolute coordinates — see `khora_tool_ui::widgets::scroll`.
    scroll: widgets::ScrollState,
}

impl ConsolePanel {
    /// Creates a new console.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            filter: LevelFilter::default(),
            search: String::new(),
            scroll: widgets::ScrollState::default(),
        }
    }
}

/// The colour and glyph that stand for a level.
fn level_style(level: LogLevel, t: &UiTheme) -> ([f32; 4], Icon) {
    match level {
        LogLevel::Error => (t.error, Icon::Error),
        LogLevel::Warn => (t.warning, Icon::Warn),
        LogLevel::Info => (t.accent_b, Icon::Info),
        LogLevel::Debug | LogLevel::Trace => (t.text_muted, Icon::Code),
    }
}

impl EditorPanel for ConsolePanel {
    fn id(&self) -> &str {
        "khora.editor.console"
    }

    fn title(&self) -> &str {
        "Console"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let t = self.theme.clone();
        let r = ui.panel_rect();

        let entries = self
            .state
            .lock()
            .ok()
            .map(|s| s.log_entries.clone())
            .unwrap_or_default();
        let counts = Counts::of(&entries);

        // ── Header: the dock tabs ──
        ui.paint_rect_filled([r[0], r[1]], [r[2], HEADER_H], t.surface, 0.0);
        ui.paint_line(
            [r[0], r[1] + HEADER_H],
            [widgets::right(r), r[1] + HEADER_H],
            t.separator,
            1.0,
        );
        // No title chip: the dock tab above already names this panel.

        // ── Filters ──
        let fy = r[1] + HEADER_H + 5.0;
        let mut x = r[0] + 10.0;

        let pills: [(&str, Tone, usize, LogLevel, &str); 4] = [
            ("Info", Tone::Info, counts.info, LogLevel::Info, "c-f-info"),
            (
                "Warn",
                Tone::Warning,
                counts.warn,
                LogLevel::Warn,
                "c-f-warn",
            ),
            (
                "Error",
                Tone::Error,
                counts.error,
                LogLevel::Error,
                "c-f-err",
            ),
            (
                "Debug",
                Tone::Neutral,
                counts.debug,
                LogLevel::Debug,
                "c-f-dbg",
            ),
        ];

        let mut toggled: Option<LogLevel> = None;
        for (label, tone, count, level, salt) in pills {
            let w = 28.0
                + ui.measure_text(label, t.font_size_caption, FontFamilyHint::Proportional)[0]
                + ui.measure_text(
                    &count.to_string(),
                    t.font_size_caption,
                    FontFamilyHint::Monospace,
                )[0]
                + 12.0;

            if filter_pill(
                ui,
                &t,
                [x, fy, w, 20.0],
                salt,
                Pill::new(label, tone)
                    .count(count)
                    .on(self.filter.admits(level)),
            )
            .clicked
            {
                toggled = Some(level);
            }
            x += w + 6.0;
        }
        if let Some(level) = toggled {
            self.filter.toggle(level);
        }

        // Search, right-aligned.
        let sw = 160.0_f32.min(r[2] * 0.32);
        let sx = widgets::right(r) - sw - 10.0;
        if sx > x {
            search_field(
                ui,
                &t,
                [sx, fy, sw, 20.0],
                "c-search",
                &self.search,
                "Filter…",
            );
            let sref = &mut self.search;
            ui.region_at(
                "console-search",
                [sx + 22.0, fy + 2.0, sw - 30.0, 16.0],
                &mut |ui| {
                    ui.text_edit_singleline(sref);
                },
            );
        }

        // ── Rows ──
        let body_y = r[1] + HEADER_H + FILTER_H + 2.0;
        let body = [r[0], body_y, r[2], (widgets::bottom(r) - body_y).max(0.0)];

        let needle = self.search.to_lowercase();
        let visible: Vec<&LogEntry> = entries
            .iter()
            .rev()
            .filter(|e| self.filter.admits(e.level))
            .filter(|e| {
                needle.is_empty()
                    || e.message.to_lowercase().contains(&needle)
                    || e.target.to_lowercase().contains(&needle)
            })
            .take(500)
            .collect();

        if visible.is_empty() {
            let w = 260.0_f32.min(body[2] - 24.0);
            let h = 110.0_f32.min(body[3] - 12.0);
            if w > 0.0 && h > 0.0 {
                empty_state(
                    ui,
                    &t,
                    [body[0] + (body[2] - w) * 0.5, body[1] + 8.0, w, h],
                    Icon::Terminal,
                    if entries.is_empty() {
                        "Console is clear"
                    } else {
                        "Nothing matches"
                    },
                    if entries.is_empty() {
                        "Engine logs will appear here."
                    } else {
                        "Try a different filter."
                    },
                );
            }
            return;
        }

        let msg_x = body[0] + 12.0 + COL_TIME + GAP + COL_LEVEL + GAP + COL_SOURCE + GAP;
        let msg_w = (widgets::right(body) - 12.0 - msg_x).max(40.0);
        let size = t.font_size_caption;

        // Scrolling, the whole of it: take the wheel, offset the cursor, clip.
        // The rows below still paint in absolute coordinates — they just start
        // higher up, and anything outside `body` is clipped away.
        let content_h = visible.len() as f32 * ROW_H + 4.0;
        self.scroll.update(ui, body, content_h);
        ui.push_clip_rect(body);

        let mut y = body[1] + 2.0 - self.scroll.offset();
        for (row_index, e) in visible.iter().enumerate() {
            // Skip rows scrolled off either edge instead of painting them under
            // the clip: a 2000-line buffer would otherwise cost 2000 paint
            // calls a frame to show forty.
            if y + ROW_H < body[1] {
                y += ROW_H;
                continue;
            }
            if y > widgets::bottom(body) {
                break;
            }
            let row = [body[0], y, body[2], ROW_H];
            let (color, glyph) = level_style(e.level, &t);

            // Tint the row for the two levels that mean "look at me".
            match e.level {
                LogLevel::Error => fill(ui, row, widgets::tint(t.error, 0.09), 0.0),
                LogLevel::Warn => fill(ui, row, widgets::tint(t.warning, 0.07), 0.0),
                _ => {}
            }

            let ty = widgets::text_y(row, size);
            let mut cx = body[0] + 12.0;

            mono(ui, [cx, ty], &e.time, size, t.text_disabled);
            cx += COL_TIME + GAP;

            icon(ui, [cx, ty], glyph, size + 1.0, color);
            cx += COL_LEVEL + GAP;

            mono(ui, [cx, ty], &truncate(&e.target, 18), size, t.text_muted);
            cx += COL_SOURCE + GAP;

            // The message is clipped, never wrapped into a taller row: rows
            // must stay on the grid for the columns to mean anything.
            ui.region_at(
                &format!("console-msg-{row_index}"),
                [cx, y, msg_w, ROW_H],
                &mut |ui| {
                    text(ui, [cx, ty], &e.message, size + 1.0, t.text_dim);
                },
            );

            y += ROW_H;
        }

        ui.pop_clip_rect();
        widgets::scrollbar(ui, &t, body, content_h, &mut self.scroll, "console-scroll");
    }
}

/// Clips a source path to fit its column, keeping the tail (the part that
/// actually distinguishes `khora_control` from `khora_data`).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let tail: String = s.chars().skip(s.chars().count() - (max - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: LogLevel, target: &str, message: &str) -> LogEntry {
        LogEntry {
            level,
            message: message.to_owned(),
            target: target.to_owned(),
            time: "12:00:00".to_owned(),
        }
    }

    #[test]
    fn default_filter_hides_debug_but_shows_the_rest() {
        let f = LevelFilter::default();
        assert!(f.admits(LogLevel::Error));
        assert!(f.admits(LogLevel::Warn));
        assert!(f.admits(LogLevel::Info));
        assert!(!f.admits(LogLevel::Debug));
        assert!(!f.admits(LogLevel::Trace));
    }

    #[test]
    fn toggling_a_level_flips_only_that_level() {
        let mut f = LevelFilter::default();
        f.toggle(LogLevel::Warn);
        assert!(!f.admits(LogLevel::Warn));
        assert!(f.admits(LogLevel::Error), "error must be untouched");
        assert!(f.admits(LogLevel::Info), "info must be untouched");

        f.toggle(LogLevel::Warn);
        assert!(f.admits(LogLevel::Warn), "toggling twice restores it");
    }

    /// Debug and Trace share one pill, so toggling either must move both.
    #[test]
    fn debug_and_trace_share_a_single_toggle() {
        let mut f = LevelFilter::default();
        f.toggle(LogLevel::Trace);
        assert!(f.admits(LogLevel::Debug));
        assert!(f.admits(LogLevel::Trace));
    }

    #[test]
    fn counts_bucket_every_level_and_fold_trace_into_debug() {
        let entries = vec![
            entry(LogLevel::Info, "a", "1"),
            entry(LogLevel::Info, "b", "2"),
            entry(LogLevel::Warn, "c", "3"),
            entry(LogLevel::Error, "d", "4"),
            entry(LogLevel::Debug, "e", "5"),
            entry(LogLevel::Trace, "f", "6"),
        ];
        let c = Counts::of(&entries);
        assert_eq!(c.info, 2);
        assert_eq!(c.warn, 1);
        assert_eq!(c.error, 1);
        assert_eq!(c.debug, 2, "trace counts toward the debug pill");
    }

    #[test]
    fn counts_of_nothing_are_zero() {
        assert_eq!(Counts::of(&[]), Counts::default());
    }

    #[test]
    fn truncate_keeps_the_distinguishing_tail() {
        assert_eq!(truncate("khora_control", 18), "khora_control");
        let long = truncate("khora_infra::graphics::wgpu::device", 18);
        assert_eq!(long.chars().count(), 18);
        assert!(long.starts_with('…'));
        assert!(
            long.ends_with("device"),
            "the tail is what tells them apart"
        );
    }
}
