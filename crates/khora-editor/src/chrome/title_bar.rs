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

//! Title bar — branded top strip with brand pill, native menus and Cmd+K
//! search.
//!
//! v3 cleanup: only menus / actions that have actual handlers are exposed.
//! - Menus: `File`, `Edit`, `Help` (each entry routes through
//!   `EditorState::pending_menu_action` which is dispatched in
//!   `EditorApp::process_menu_actions`).
//! - Search pill: opens the Cmd+K command palette.
//! - The `Object / View / Build / Window` menus, the right-side icon
//!   buttons (branch / build / bell / share) and the "EF" avatar were
//!   removed — they had no backing handler.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::{
    brand::paint_brand_pill,
    chrome::paint_search_pill,
    paint::{paint_vertical_gradient, with_alpha},
};

const TITLE_BAR_HEIGHT: f32 = 44.0;
const SEARCH_MIN_W: f32 = 180.0;
const SEARCH_PREFERRED_W: f32 = 320.0;
const SEARCH_HEIGHT: f32 = 28.0;
/// Below this width we collapse the pill into a single search icon.
const SEARCH_COLLAPSE_THRESHOLD: f32 = 120.0;
const MENU_REGION_WIDTH: f32 = 240.0;

/// Top-bar branded strip.
pub struct TitleBarPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl TitleBarPanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self { state, theme }
    }

    fn open_command_palette(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.command_palette_open = true;
        }
    }
}

impl EditorPanel for TitleBarPanel {
    fn id(&self) -> &str {
        "khora.editor.title_bar"
    }

    fn title(&self) -> &str {
        "Title Bar"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(TITLE_BAR_HEIGHT)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = self.theme.clone();
        let project_name = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_name.clone())
            .unwrap_or_else(|| "untitled".to_owned());

        let rect = ui.panel_rect();
        let [x, y, w, h] = rect;

        // ── Background gradient + bottom hairline ────
        paint_vertical_gradient(ui, rect, theme.surface_elevated, theme.surface, 6);
        ui.paint_line(
            [x, y + h],
            [x + w, y + h],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        // ── Brand pill ───────────────────────────────
        let brand_x = x + 14.0;
        let pill_right =
            paint_brand_pill(ui, [brand_x, y], h, "KhoraEngine", &project_name, &theme);

        // ── Native egui menus (File / Edit / Help) ───
        let menus_x = pill_right + 12.0;
        let menus_y = y + (h - 24.0) * 0.5;
        let menu_region = [menus_x, menus_y, MENU_REGION_WIDTH, 24.0];

        // Capture closures for dispatch tokens, then route via region_at →
        // egui menu_button. The inner closures push dispatch tokens by
        // calling self.dispatch.
        let state_for_menus = self.state.clone();
        let dispatch = move |action: &str| {
            if let Ok(mut s) = state_for_menus.lock() {
                s.pending_menu_action = Some(action.to_owned());
            }
        };

        ui.region_at(menu_region, &mut |ui_inner| {
            ui_inner.horizontal(&mut |ui_inner| {
                ui_inner.menu_button("File", &mut |m| {
                    if m.button("New Scene") {
                        dispatch("new_scene");
                        m.close_menu();
                    }
                    if m.button("Open…") {
                        dispatch("open");
                        m.close_menu();
                    }
                    m.separator();
                    if m.button("Save") {
                        dispatch("save");
                        m.close_menu();
                    }
                    if m.button("Save As…") {
                        dispatch("save_as");
                        m.close_menu();
                    }
                    if m.button("Export Scene as RON…") {
                        dispatch("export_ron");
                        m.close_menu();
                    }
                    m.separator();
                    if m.button("Quit") {
                        dispatch("quit");
                        m.close_menu();
                    }
                });
                ui_inner.menu_button("Edit", &mut |m| {
                    if m.button("Undo  Ctrl+Z") {
                        dispatch("undo");
                        m.close_menu();
                    }
                    if m.button("Redo  Ctrl+Y") {
                        dispatch("redo");
                        m.close_menu();
                    }
                    m.separator();
                    if m.button("Delete  Del") {
                        dispatch("delete");
                        m.close_menu();
                    }
                });
                ui_inner.menu_button("Build", &mut |m| {
                    if m.button("Build Game…") {
                        dispatch("build_game");
                        m.close_menu();
                    }
                });
                ui_inner.menu_button("Help", &mut |m| {
                    if m.button("Documentation") {
                        dispatch("documentation");
                        m.close_menu();
                    }
                    if m.button("About Khora Engine") {
                        dispatch("about");
                        m.close_menu();
                    }
                });
            });
        });

        // ── Search pill (right-aligned with overflow guard) ──
        let right_pad = 14.0;
        let search_right = x + w - right_pad;
        let search_left_min = menus_x + MENU_REGION_WIDTH + 12.0;
        let available = (search_right - search_left_min).max(0.0);

        if available >= SEARCH_MIN_W {
            // Full pill
            let search_w = SEARCH_PREFERRED_W.min(available);
            let search_x = search_right - search_w;
            let search_y = y + (h - SEARCH_HEIGHT) * 0.5;
            let (search_int, _) = paint_search_pill(
                ui,
                [search_x, search_y],
                search_w,
                SEARCH_HEIGHT,
                "Search commands, assets, entities…",
                &theme,
            );
            if search_int.clicked {
                self.open_command_palette();
            }
        } else if available >= SEARCH_COLLAPSE_THRESHOLD {
            // Collapsed icon-only button (no pill, just hit area)
            let icon_x = search_right - 28.0;
            let icon_y = y + (h - 28.0) * 0.5;
            let int = ui.interact_rect("titlebar-search-collapsed", [icon_x, icon_y, 28.0, 28.0]);
            if int.hovered {
                ui.paint_rect_filled([icon_x, icon_y], [28.0, 28.0], theme.surface_active, 6.0);
            }
            crate::widgets::paint::paint_icon(
                ui,
                [icon_x + 7.0, icon_y + 7.0],
                Icon::Search,
                14.0,
                theme.text_dim,
            );
            if int.clicked {
                self.open_command_palette();
            }
        }
        // else: not enough room even for the icon — hide entirely.
    }
}
