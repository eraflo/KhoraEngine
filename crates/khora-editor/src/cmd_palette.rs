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

//! Command palette — Cmd+K modal that runs editor actions.
//!
//! Registered as a [`Floating`] panel (the shell's modal layer). The panel
//! visibility is driven by [`EditorState::command_palette_open`]; the actual
//! Cmd+K / Ctrl+K key binding lives in `main.rs` so it can also fire from
//! deep inside `update()`.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::chrome::widgets::with_alpha;

/// A single palette command — a label and the action token it dispatches into
/// [`EditorState::pending_menu_action`].
#[derive(Debug, Clone)]
struct Command {
    label: &'static str,
    description: &'static str,
    action: &'static str,
}

const COMMANDS: &[Command] = &[
    Command {
        label: "Save Scene",
        description: "Save the current scene file",
        action: "save",
    },
    Command {
        label: "Save Scene As…",
        description: "Save under a new filename",
        action: "save_as",
    },
    Command {
        label: "Open Scene…",
        description: "Open a .kscene file",
        action: "open",
    },
    Command {
        label: "New Scene",
        description: "Discard current scene and start fresh",
        action: "new_scene",
    },
    Command {
        label: "Play",
        description: "Run the scene in editor",
        action: "play",
    },
    Command {
        label: "Pause",
        description: "Pause the running simulation",
        action: "pause",
    },
    Command {
        label: "Stop",
        description: "Stop simulation and restore scene",
        action: "stop",
    },
    Command {
        label: "Undo",
        description: "Revert last edit (Ctrl+Z)",
        action: "undo",
    },
    Command {
        label: "Redo",
        description: "Re-apply last undone edit (Ctrl+Y)",
        action: "redo",
    },
    Command {
        label: "Delete Selection",
        description: "Remove selected entities",
        action: "delete",
    },
    Command {
        label: "Documentation",
        description: "Open Khora Engine documentation",
        action: "documentation",
    },
];

/// Floating modal command palette.
pub struct CommandPalettePanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
    query: String,
    active: usize,
}

impl CommandPalettePanel {
    /// Creates a new palette.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self {
            state,
            theme,
            query: String::new(),
            active: 0,
        }
    }
}

fn matches(cmd: &Command, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    cmd.label.to_lowercase().contains(&q) || cmd.description.to_lowercase().contains(&q)
}

impl EditorPanel for CommandPalettePanel {
    fn id(&self) -> &str {
        "khora.editor.command_palette"
    }

    fn title(&self) -> &str {
        "Command Palette"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Read open flag and bail out without painting if closed.
        let is_open = self
            .state
            .lock()
            .ok()
            .map(|s| s.command_palette_open)
            .unwrap_or(false);
        if !is_open {
            return;
        }

        // ── Backdrop (full screen, semi-opaque) ───────
        let [sx, sy, sw, sh] = ui.screen_rect();
        ui.paint_rect_filled(
            [sx, sy],
            [sw, sh],
            with_alpha(self.theme.background, 0.65),
            0.0,
        );

        // ── Modal box centered horizontally, ~14vh ────
        let modal_w = 640.0_f32.min(sw - 64.0);
        let modal_x = sx + (sw - modal_w) * 0.5;
        let modal_y = sy + sh * 0.14;
        let modal_h = 420.0_f32.min(sh - modal_y - 32.0);

        // Modal background
        ui.paint_rect_filled(
            [modal_x, modal_y],
            [modal_w, modal_h],
            self.theme.surface_elevated,
            self.theme.radius_xl,
        );
        // Brand glow border (overlay with primary + low alpha)
        ui.paint_rect_filled(
            [modal_x, modal_y],
            [modal_w, modal_h],
            with_alpha(self.theme.primary, 0.06),
            self.theme.radius_xl,
        );
        // Bottom hairline under input
        ui.paint_line(
            [modal_x + 16.0, modal_y + 56.0],
            [modal_x + modal_w - 16.0, modal_y + 56.0],
            with_alpha(self.theme.separator, 0.55),
            1.0,
        );

        // ── Compute visible matches BEFORE painting rows ──
        let filtered: Vec<&Command> = COMMANDS.iter().filter(|c| matches(c, &self.query)).collect();
        if self.active >= filtered.len() {
            self.active = 0;
        }

        // Selected command (used after the closure, since we may dispatch).
        let mut to_dispatch: Option<&'static str> = None;
        let mut close_after = false;

        let theme = self.theme.clone();
        let query_ref = &mut self.query;
        let active_ref = &mut self.active;

        ui.vertical(&mut |ui| {
            // Spacer to position content inside the modal box (since we draw
            // the bg as absolute rects, layout offsets are added via spacing).
            ui.spacing(modal_y + 14.0);
            ui.horizontal(&mut |ui| {
                ui.spacing(modal_x + 18.0);
                ui.colored_label(theme.primary, "◆");
                ui.text_edit_singleline(query_ref);
                if ui.is_last_item_escape_pressed() {
                    close_after = true;
                }
                if ui.is_last_item_enter_pressed() {
                    if let Some(c) = filtered.get(*active_ref) {
                        to_dispatch = Some(c.action);
                        close_after = true;
                    }
                }
            });

            // Spacer past the hairline
            ui.spacing(16.0);

            ui.scroll_area("cmdk_list", &mut |ui| {
                if filtered.is_empty() {
                    ui.horizontal(&mut |ui| {
                        ui.spacing(modal_x + 24.0);
                        ui.colored_label(theme.text_muted, "No commands match.");
                    });
                    return;
                }
                for (i, cmd) in filtered.iter().enumerate() {
                    let is_active = i == *active_ref;
                    ui.horizontal(&mut |ui| {
                        ui.spacing(modal_x + 18.0);
                        let label = if is_active {
                            format!("▸  {}", cmd.label)
                        } else {
                            format!("    {}", cmd.label)
                        };
                        if ui.selectable_label(is_active, &label) {
                            *active_ref = i;
                            to_dispatch = Some(cmd.action);
                            close_after = true;
                        }
                        ui.spacing(8.0);
                        ui.colored_label(theme.text_muted, cmd.description);
                    });
                }
            });
        });

        // ── Apply effects (close + dispatch) ──────────
        if let Some(action) = to_dispatch {
            if let Ok(mut s) = self.state.lock() {
                s.pending_menu_action = Some(action.to_owned());
            }
        }
        if close_after {
            if let Ok(mut s) = self.state.lock() {
                s.command_palette_open = false;
            }
            self.query.clear();
            self.active = 0;
        }
    }
}
