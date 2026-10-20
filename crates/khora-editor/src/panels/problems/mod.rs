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

//! The Problems tab: what is wrong with the open project's scripts — compile
//! errors and warnings, registration clashes and orphans, faulted and slow
//! instances — grouped by module, each row leading to its file or its entity.
//!
//! Drawn as the Console is: the same filter pills, the same grid of rows, the
//! same tint for the two severities, and the severity is an icon as well as a
//! colour.

pub(crate) mod view;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ProblemSeverity;
use khora_tool_ui::widgets::{
    self, empty_state, filter_pill,
    paint::{fill, icon, mono, text},
    Pill, Tone,
};

use view::{grouped, tab_title, ProblemAction};

const HEADER_H: f32 = 34.0;
const FILTER_H: f32 = 30.0;
const ROW_H: f32 = 17.0;
const COL_LEVEL: f32 = 16.0;
const COL_POS: f32 = 52.0;
const GAP: f32 = 9.0;

/// The Problems tab.
pub struct ProblemsPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// The tab's title, its counts included: rebuilt each frame from what it
    /// lists.
    title: String,
    show_errors: bool,
    show_warnings: bool,
    /// How far the list is scrolled — its rows paint in absolute coordinates.
    scroll: widgets::ScrollState,
}

impl ProblemsPanel {
    /// A Problems tab over the editor's state.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            title: "Problems".to_owned(),
            show_errors: true,
            show_warnings: true,
            scroll: widgets::ScrollState::default(),
        }
    }
}

/// One line of the list: a module's header, a problem, or a problem's note.
enum Line {
    Module(String, usize),
    Problem {
        severity: ProblemSeverity,
        position: String,
        message: String,
        entity: Option<String>,
        action: ProblemAction,
    },
    Note(String),
}

/// The name the hierarchy shows for `entity`.
fn name_in(nodes: &[SceneNode], entity: EntityId) -> Option<String> {
    nodes.iter().find_map(|node| {
        if node.entity == entity {
            Some(node.name.clone())
        } else {
            name_in(&node.children, entity)
        }
    })
}

/// `row_text` split into its `line:col` and its message — the position
/// empty when the problem has none.
fn split_position(row_text: &str) -> (String, String) {
    match row_text.split_once("  ") {
        Some((position, message))
            if position.contains(':')
                && position.chars().all(|c| c.is_ascii_digit() || c == ':') =>
        {
            (position.to_owned(), message.to_owned())
        }
        _ => (String::new(), row_text.to_owned()),
    }
}

impl EditorPanel for ProblemsPanel {
    fn id(&self) -> &str {
        "khora.editor.problems"
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let t = self.theme.clone();
        let r = ui.panel_rect();

        let (problems, roots, project) = self
            .state
            .lock()
            .ok()
            .map(|s| {
                (
                    s.script_problems.clone(),
                    s.scene_roots.clone(),
                    s.project_folder.clone(),
                )
            })
            .unwrap_or_default();
        self.title = tab_title(&problems);
        let errors = problems
            .iter()
            .filter(|problem| problem.severity == ProblemSeverity::Error)
            .count();
        let warnings = problems.len() - errors;

        // ── Header ──
        ui.paint_rect_filled([r[0], r[1]], [r[2], HEADER_H], t.surface, 0.0);
        ui.paint_line(
            [r[0], r[1] + HEADER_H],
            [widgets::right(r), r[1] + HEADER_H],
            t.separator,
            1.0,
        );

        // ── Filters ──
        let fy = r[1] + HEADER_H + 5.0;
        let mut x = r[0] + 10.0;
        let pills = [
            ("Errors", Tone::Error, errors, self.show_errors, "p-f-err"),
            (
                "Warnings",
                Tone::Warning,
                warnings,
                self.show_warnings,
                "p-f-warn",
            ),
        ];
        let mut toggled = None;
        for (index, (label, tone, count, on, salt)) in pills.into_iter().enumerate() {
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
                Pill::new(label, tone).count(count).on(on),
            )
            .clicked
            {
                toggled = Some(index);
            }
            x += w + 6.0;
        }
        match toggled {
            Some(0) => self.show_errors = !self.show_errors,
            Some(_) => self.show_warnings = !self.show_warnings,
            None => {}
        }

        // ── Lines ──
        let body_y = r[1] + HEADER_H + FILTER_H + 2.0;
        let body = [r[0], body_y, r[2], (widgets::bottom(r) - body_y).max(0.0)];
        let shown: Vec<_> = problems
            .iter()
            .filter(|problem| match problem.severity {
                ProblemSeverity::Error => self.show_errors,
                ProblemSeverity::Warning => self.show_warnings,
            })
            .cloned()
            .collect();

        if shown.is_empty() {
            let w = 280.0_f32.min(body[2] - 24.0);
            let h = 110.0_f32.min(body[3] - 12.0);
            if w > 0.0 && h > 0.0 {
                empty_state(
                    ui,
                    &t,
                    [body[0] + (body[2] - w) * 0.5, body[1] + 8.0, w, h],
                    Icon::Code,
                    if problems.is_empty() {
                        "No problems"
                    } else {
                        "Nothing matches"
                    },
                    if problems.is_empty() {
                        "The project's scripts compile, register and run clean."
                    } else {
                        "Turn a filter back on to see the rest."
                    },
                );
            }
            return;
        }

        let name_of = |entity: EntityId| name_in(&roots, entity);
        let mut lines = Vec::new();
        for group in grouped(&shown, &name_of) {
            lines.push(Line::Module(
                group
                    .module
                    .clone()
                    .unwrap_or_else(|| "Not in a script".to_owned()),
                group.rows.len(),
            ));
            for row in group.rows {
                let (position, message) = split_position(&row.text);
                lines.push(Line::Problem {
                    severity: row.severity,
                    position,
                    message,
                    entity: row.entity,
                    action: row.action,
                });
                if let Some(note) = row.note {
                    lines.push(Line::Note(note));
                }
            }
        }

        let size = t.font_size_caption;
        let content_h = lines.len() as f32 * ROW_H + 4.0;
        self.scroll.update(ui, body, content_h);
        ui.push_clip_rect(body);

        let mut clicked: Option<ProblemAction> = None;
        let mut y = body[1] + 2.0 - self.scroll.offset();
        let msg_x = body[0] + 12.0 + COL_LEVEL + GAP + COL_POS + GAP;
        for (index, line) in lines.iter().enumerate() {
            if y + ROW_H < body[1] {
                y += ROW_H;
                continue;
            }
            if y > widgets::bottom(body) {
                break;
            }
            let row = [body[0], y, body[2], ROW_H];
            let ty = widgets::text_y(row, size);
            match line {
                Line::Module(module, count) => {
                    fill(ui, row, widgets::with_alpha(t.surface_elevated, 0.55), 0.0);
                    icon(
                        ui,
                        [body[0] + 12.0, ty],
                        Icon::Code,
                        size + 1.0,
                        t.text_muted,
                    );
                    mono(
                        ui,
                        [body[0] + 12.0 + COL_LEVEL + GAP, ty],
                        module,
                        size,
                        t.text_dim,
                    );
                    let n = count.to_string();
                    let nw = ui.measure_text(&n, size, FontFamilyHint::Monospace)[0];
                    mono(
                        ui,
                        [widgets::right(body) - 12.0 - nw, ty],
                        &n,
                        size,
                        t.text_muted,
                    );
                }
                Line::Problem {
                    severity,
                    position,
                    message,
                    entity,
                    action,
                } => {
                    let (color, glyph, tint) = match severity {
                        ProblemSeverity::Error => (t.error, Icon::Error, 0.09),
                        ProblemSeverity::Warning => (t.warning, Icon::Warn, 0.07),
                    };
                    let hit = ui.interact_rect(&format!("problem-{index}"), row);
                    let tint = if hit.hovered { tint + 0.06 } else { tint };
                    fill(ui, row, widgets::tint(color, tint), 0.0);
                    if hit.clicked {
                        clicked = Some(action.clone());
                    }
                    icon(ui, [body[0] + 12.0, ty], glyph, size + 1.0, color);
                    mono(
                        ui,
                        [body[0] + 12.0 + COL_LEVEL + GAP, ty],
                        position,
                        size,
                        t.text_muted,
                    );
                    // The entity, right-aligned, where a running problem
                    // happened; the message takes what is left, clipped.
                    let mut msg_w = (widgets::right(body) - 12.0 - msg_x).max(40.0);
                    if let Some(entity) = entity {
                        let ew = ui.measure_text(entity, size, FontFamilyHint::Monospace)[0];
                        mono(
                            ui,
                            [widgets::right(body) - 12.0 - ew, ty],
                            entity,
                            size,
                            t.text_dim,
                        );
                        msg_w = (msg_w - ew - GAP).max(40.0);
                    }
                    ui.region_at(
                        &format!("problem-msg-{index}"),
                        [msg_x, y, msg_w, ROW_H],
                        &mut |ui| {
                            text(ui, [msg_x, ty], message, size + 1.0, t.text);
                        },
                    );
                }
                Line::Note(note) => {
                    let w = (widgets::right(body) - 12.0 - msg_x).max(40.0);
                    ui.region_at(
                        &format!("problem-note-{index}"),
                        [msg_x, y, w, ROW_H],
                        &mut |ui| {
                            text(ui, [msg_x, ty], note, size, t.text_muted);
                        },
                    );
                }
            }
            y += ROW_H;
        }
        ui.pop_clip_rect();
        widgets::scrollbar(ui, &t, body, content_h, &mut self.scroll, "problems-scroll");

        match clicked {
            Some(ProblemAction::OpenModule(module)) => {
                let Some(project) = project else { return };
                let file = view::module_file(&Path::new(&project).join("assets"), &module);
                if let Err(error) = crate::project_scripts::open_script(&file) {
                    log::warn!("could not open {}: {error}", file.display());
                }
            }
            Some(ProblemAction::SelectEntity(entity)) => {
                if let Ok(mut state) = self.state.lock() {
                    state.select(entity);
                }
            }
            Some(ProblemAction::Nothing) | None => {}
        }
    }
}
