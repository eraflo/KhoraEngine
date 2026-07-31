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
//! Phase J: atomic modal — backdrop + box are painted absolutely, all
//! interactive controls live inside a single `region_at` so the layout
//! stays in sync regardless of zoom or window size.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::KeyCode;

use crate::widgets::brand::paint_diamond_filled;
use crate::widgets::chrome::paint_kbd_chip;
use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

/// A single palette command.
#[derive(Debug, Clone)]
struct Command {
    label: &'static str,
    description: &'static str,
    action: &'static str,
    icon: Icon,
}

#[derive(Debug, Clone)]
struct Section {
    title: &'static str,
    items: &'static [Command],
}

const QUICK_ACTIONS: &[Command] = &[
    Command {
        label: "Save Scene",
        description: "Save the current scene file",
        action: "save",
        icon: Icon::Cube,
    },
    Command {
        label: "Play",
        description: "Run scene in editor",
        action: "play",
        icon: Icon::Play,
    },
    Command {
        label: "Pause",
        description: "Pause the running simulation",
        action: "pause",
        icon: Icon::Pause,
    },
    Command {
        label: "Stop",
        description: "Stop simulation, restore scene",
        action: "stop",
        icon: Icon::Stop,
    },
];

const CREATE_ACTIONS: &[Command] = &[
    Command {
        label: "New Scene",
        description: "Discard current scene and start fresh",
        action: "new_scene",
        icon: Icon::Plus,
    },
    Command {
        label: "New Empty Entity",
        description: "Spawn an empty entity in the scene",
        action: "spawn_empty",
        icon: Icon::Plus,
    },
    Command {
        label: "Save Scene As…",
        description: "Save under a new filename",
        action: "save_as",
        icon: Icon::Cube,
    },
];

const NAVIGATE_ACTIONS: &[Command] = &[
    Command {
        label: "Open Scene…",
        description: "Open a .kscene file",
        action: "open",
        icon: Icon::Folder,
    },
    Command {
        label: "Documentation",
        description: "Open Khora Engine docs",
        action: "documentation",
        icon: Icon::Code,
    },
];

const VIEW_ACTIONS: &[Command] = &[Command {
    label: "Toggle Wireframe",
    description: "Show scene meshes as edge lines (debug)",
    action: "toggle_wireframe",
    icon: Icon::Cube,
}];

const SECTIONS: &[Section] = &[
    Section {
        title: "Quick Actions",
        items: QUICK_ACTIONS,
    },
    Section {
        title: "Create",
        items: CREATE_ACTIONS,
    },
    Section {
        title: "Navigate",
        items: NAVIGATE_ACTIONS,
    },
    Section {
        title: "View",
        items: VIEW_ACTIONS,
    },
];

/// Floating modal command palette.
pub struct CommandPalettePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    query: String,
    active: usize,
    /// Whether the palette was already open last frame, so focus is requested
    /// exactly once per opening rather than every frame (which would trap it).
    was_open: bool,
    /// Points the result list is scrolled down by, tracking the active row.
    list_scroll: f32,
}

impl CommandPalettePanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            query: String::new(),
            active: 0,
            was_open: false,
            list_scroll: 0.0,
        }
    }
}

fn matches(cmd: &Command, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    cmd.label.to_lowercase().contains(query_lower)
        || cmd.description.to_lowercase().contains(query_lower)
}

impl EditorPanel for CommandPalettePanel {
    fn id(&self) -> &str {
        "khora.editor.command_palette"
    }

    fn title(&self) -> &str {
        "Command Palette"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Bail out without painting if not open.
        let is_open = self
            .state
            .lock()
            .ok()
            .map(|s| s.command_palette_open)
            .unwrap_or(false);
        if !is_open {
            // Reset the one-shot focus latch so the next open takes focus
            // again, and clear the query so the palette doesn't reopen showing
            // the last search.
            self.was_open = false;
            self.list_scroll = 0.0;
            self.query.clear();
            self.active = 0;
            return;
        }

        let theme = self.theme.clone();
        let [sx, sy, sw, sh] = ui.screen_rect();

        // ── Backdrop (full screen, semi-opaque) ───────
        ui.paint_rect_filled([sx, sy], [sw, sh], with_alpha(theme.background, 0.65), 0.0);

        // ── Modal box ────────────────────────────────
        let modal_w = 640.0_f32.min(sw - 64.0);
        let modal_x = sx + (sw - modal_w) * 0.5;
        let modal_y = sy + sh * 0.14;
        let modal_h = 480.0_f32.min(sh - modal_y - 32.0);

        // Detect click on backdrop EXCLUDING the modal area, so clicking
        // inside the modal doesn't close the palette (Bug C.5 in v3 plan).
        let backdrop_int = ui.interact_rect("cmdk-backdrop", [sx, sy, sw, sh]);
        let modal_int = ui.interact_rect("cmdk-modal-eat", [modal_x, modal_y, modal_w, modal_h]);
        if backdrop_int.clicked && !modal_int.hovered {
            if let Ok(mut s) = self.state.lock() {
                s.command_palette_open = false;
            }
        }

        ui.paint_rect_filled(
            [modal_x, modal_y],
            [modal_w, modal_h],
            theme.surface_elevated,
            theme.radius_xl,
        );
        // Brand glow border
        ui.paint_rect_stroke(
            [modal_x, modal_y],
            [modal_w, modal_h],
            with_alpha(theme.primary, 0.25),
            theme.radius_xl,
            1.0,
        );
        // Internal soft glow
        ui.paint_rect_stroke(
            [modal_x + 1.0, modal_y + 1.0],
            [modal_w - 2.0, modal_h - 2.0],
            with_alpha(theme.primary, 0.05),
            theme.radius_xl - 1.0,
            1.0,
        );

        // (Backdrop / modal interaction handled above before painting the
        // modal box — `modal_int.hovered` gates the close-on-outside.)

        // ── Header (input row) ───────────────────────
        let header_h = 56.0;
        let pad = 18.0;
        let input_y = modal_y + (header_h - 28.0) * 0.5;

        // Diamond
        paint_diamond_filled(
            ui,
            modal_x + pad + 9.0,
            modal_y + header_h * 0.5,
            7.0,
            theme.primary,
        );

        // Esc kbd chip on right
        let esc_x = modal_x + modal_w - pad - 30.0;
        paint_kbd_chip(ui, [esc_x, input_y + 6.0], "esc", &theme);

        // Hairline below header
        ui.paint_line(
            [modal_x + pad - 6.0, modal_y + header_h],
            [modal_x + modal_w - pad + 6.0, modal_y + header_h],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        // ── Filter pre-compute ────────────────────────
        let q_lower = self.query.to_lowercase();
        let mut visible: Vec<(&Section, Vec<&Command>)> = Vec::new();
        for section in SECTIONS {
            let items: Vec<&Command> = section
                .items
                .iter()
                .filter(|c| matches(c, &q_lower))
                .collect();
            if !items.is_empty() {
                visible.push((section, items));
            }
        }
        let total: usize = visible.iter().map(|(_, items)| items.len()).sum();
        if self.active >= total.max(1) {
            self.active = 0;
        }

        // ── Body region (input + list) ───────────────
        let body_rect = [
            modal_x,
            modal_y + header_h + 4.0,
            modal_w,
            modal_h - header_h - 56.0,
        ];

        // ── Keyboard navigation ──────────────────────
        // Read before the field is drawn: the arrows must move the selection
        // even while the query field holds focus, which is the whole point of
        // a palette. `key_pressed` would refuse (it suppresses shortcuts while
        // a field is focused), so the arrows go through the raw key state.
        if ui.raw_key_pressed(KeyCode::ArrowDown) && total > 0 {
            self.active = (self.active + 1) % total;
        }
        if ui.raw_key_pressed(KeyCode::ArrowUp) && total > 0 {
            self.active = (self.active + total - 1) % total;
        }

        // Focus the field on the frame the palette opens, once. Requesting it
        // every frame would trap focus; never requesting it — the old
        // behaviour — meant the user had to click the box before typing, and
        // meanwhile every keystroke fell through to the editor's shortcuts.
        let take_focus = !self.was_open;
        self.was_open = true;

        // Snapshot before the field borrows it: the row loop below may move the
        // selection on hover, but the scroll must follow where it is *now*.
        let active_now = self.active;
        let list_scroll_prev = self.list_scroll;
        let query_ref = &mut self.query;
        let active_ref = &mut self.active;
        let mut to_dispatch: Option<&'static str> = None;
        let mut close_after = false;
        let theme_for_closure = theme.clone();

        // Input field
        let input_rect = [
            modal_x + pad + 28.0,
            input_y,
            modal_w - pad * 2.0 - 80.0,
            28.0,
        ];
        ui.region_at("cmd-palette-input", input_rect, &mut |ui_inner| {
            ui_inner.text_edit_singleline(query_ref);
            if take_focus {
                ui_inner.focus_last_item();
            }
            if ui_inner.is_last_item_escape_pressed() {
                close_after = true;
            }
            if ui_inner.is_last_item_enter_pressed() {
                // pick currently-active item
                let mut idx = 0usize;
                'outer: for (_, items) in &visible {
                    for cmd in items {
                        if idx == *active_ref {
                            to_dispatch = Some(cmd.action);
                            close_after = true;
                            break 'outer;
                        }
                        idx += 1;
                    }
                }
            }
        });

        // List — clipped to the modal body and scrolled to keep the active row
        // in view.
        //
        // It used to run past the modal's bottom edge: the last rows painted
        // over the footer and then straight onto the workspace behind, so the
        // dialog looked like it had burst. The list is short today, but it is a
        // list — its length is not a constant.
        let row_pitch = 36.0 + 2.0;
        let content_h = visible
            .iter()
            .map(|(_, items)| 18.0 + items.len() as f32 * row_pitch)
            .sum::<f32>()
            + 8.0;
        // Follow the keyboard selection rather than the wheel: the palette is
        // driven from the keyboard, so the view must track the active row.
        let active_top = {
            let mut y = 8.0;
            let mut seen = 0usize;
            for (_, items) in &visible {
                y += 18.0;
                for _ in items.iter() {
                    if seen == active_now {
                        break;
                    }
                    seen += 1;
                    y += row_pitch;
                }
                if seen == active_now {
                    break;
                }
            }
            y
        };
        let view_h = body_rect[3];
        let scroll = if content_h <= view_h {
            0.0
        } else {
            // Keep the active row inside the viewport, nudging only as much as
            // needed so the list doesn't jump on every arrow press.
            let want_min = (active_top + row_pitch - view_h).max(0.0);
            let want_max = active_top;
            list_scroll_prev
                .clamp(want_min, want_max)
                .min(content_h - view_h)
        };
        self.list_scroll = scroll;

        ui.push_clip_rect(body_rect);
        let mut idx = 0usize;
        let mut row_y = body_rect[1] + 8.0 - scroll;
        for (section, items) in &visible {
            // Section header
            ui.paint_text_styled(
                [modal_x + pad, row_y],
                section.title,
                10.0,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            row_y += 18.0;

            for cmd in items {
                let active = idx == *active_ref;
                let row_x = modal_x + pad - 4.0;
                let row_w = modal_w - pad * 2.0 + 8.0;
                let row_h = 36.0;

                // The highlighted row carries the gold selection bar, exactly
                // like a selected entity or asset — one selection language
                // across the whole editor. The icon sits bare: a filled plate
                // per row turns the list into a grid of buttons.
                if active {
                    ui.paint_rect_filled(
                        [row_x, row_y],
                        [row_w, row_h],
                        theme.surface_active,
                        theme.radius_md,
                    );
                    khora_tool_ui::widgets::paint::selection_bar(
                        ui,
                        [row_x, row_y, row_w, row_h],
                        theme.accent_c,
                    );
                }

                paint_icon(
                    ui,
                    [row_x + 15.0, row_y + 11.0],
                    cmd.icon,
                    14.0,
                    if active {
                        theme.accent_c
                    } else {
                        theme.primary_dim
                    },
                );

                // Label + desc
                paint_text_size(ui, [row_x + 44.0, row_y + 8.0], cmd.label, 13.0, theme.text);
                ui.paint_text_styled(
                    [row_x + row_w - 8.0, row_y + 11.0],
                    cmd.description,
                    11.0,
                    theme.text_muted,
                    FontFamilyHint::Proportional,
                    TextAlign::Right,
                );

                let interaction =
                    ui.interact_rect(&format!("cmdk-row-{}", idx), [row_x, row_y, row_w, row_h]);
                if interaction.hovered {
                    *active_ref = idx;
                }
                if interaction.clicked {
                    to_dispatch = Some(cmd.action);
                    close_after = true;
                }

                row_y += row_h + 1.0;
                idx += 1;
            }
            row_y += 6.0;
        }
        if visible.is_empty() {
            ui.paint_text_styled(
                [modal_x + modal_w * 0.5, row_y + 30.0],
                "No commands match.",
                12.0,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
        }
        ui.pop_clip_rect();

        // ── Footer ───────────────────────────────────
        let footer_y = modal_y + modal_h - 40.0;
        ui.paint_line(
            [modal_x, footer_y],
            [modal_x + modal_w, footer_y],
            with_alpha(theme.separator, 0.55),
            1.0,
        );
        let mut fx = modal_x + pad;
        for (chip, text) in [("↑↓", " Navigate"), ("↵", " Select"), ("esc", " Close")] {
            let after = paint_kbd_chip(ui, [fx, footer_y + 14.0], chip, &theme);
            paint_text_size(
                ui,
                [after + 4.0, footer_y + 14.0],
                text,
                10.5,
                theme.text_muted,
            );
            fx = after + 64.0;
        }
        let engine_version = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_engine_version.clone())
            .unwrap_or_else(|| "dev".to_owned());
        let version_label = format!("khora · v{}", engine_version);
        ui.paint_text_styled(
            [modal_x + modal_w - pad, footer_y + 14.0],
            &version_label,
            10.0,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );

        // Apply effects
        let _ = theme_for_closure;
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
