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

//! The asset browser as an editor panel: the frame it draws each tick.

use khora_sdk::editor_ui::*;

use crate::widgets::panel_header::paint_panel_header;
use crate::widgets::tile::AssetTileKind;
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

use super::{AssetBrowserPanel, HEADER_HEIGHT, SIDEBAR_WIDTH};

const SIDEBAR_CATEGORIES: &[(AssetTileKind, &str, Icon)] = &[
    (AssetTileKind::Unknown, "All Assets", Icon::Database),
    (AssetTileKind::Scene, "Scenes", Icon::Film),
    (AssetTileKind::Mesh, "Meshes", Icon::Cube),
    (AssetTileKind::Texture, "Textures", Icon::Image),
    (AssetTileKind::Material, "Materials", Icon::Circle),
    (AssetTileKind::Audio, "Audio", Icon::Music),
    (AssetTileKind::Shader, "Shaders", Icon::Zap),
    (AssetTileKind::Script, "Scripts", Icon::Code),
];

impl EditorPanel for AssetBrowserPanel {
    fn id(&self) -> &str {
        "khora.editor.asset_browser"
    }
    fn title(&self) -> &str {
        "Assets"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        self.rescan_if_needed();
        self.follow_declared_names();
        self.follow_reveal();

        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, ph] = panel_rect;

        self.show_header(ui, &theme, panel_rect);

        let body_y = py + HEADER_HEIGHT;
        let body_h = ph - HEADER_HEIGHT;
        let sidebar_w = SIDEBAR_WIDTH;
        self.show_sidebar(ui, &theme, [px, body_y, sidebar_w, body_h]);
        self.show_grid(ui, &theme, [px + sidebar_w, body_y, pw - sidebar_w, body_h]);

        // Painted last so it sits above the grid it is asking about.
        self.render_delete_confirmation(ui, panel_rect, &theme);
    }
}

impl AssetBrowserPanel {
    /// Paints the header: the asset count and the More, Delete and Filter buttons.
    fn show_header(&mut self, ui: &mut dyn UiBuilder, theme: &UiTheme, panel_rect: [f32; 4]) {
        let [px, py, pw, _] = panel_rect;
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, theme);
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;
        let badge = format!("{}", self.flat.len());
        // Only the count: the dock tab above already says "Asset Browser".
        paint::text(
            ui,
            [px + 6.0, tab_y + 5.0],
            &badge,
            theme.font_size_caption,
            theme.text_muted,
        );

        // Collected, then applied after the loop: the menu closures cannot
        // borrow `self` mutably.
        let mut header_delete_selected = false;
        let mut header_filter_pick: Option<Option<AssetTileKind>> = None;
        let mut header_new_folder = false;
        let mut header_new_script = false;
        let mut header_reveal_project = false;
        let mut ax = px + pw - 8.0;
        for (icon, salt) in [
            (Icon::More, "ab-more"),
            (Icon::Trash, "ab-trash"),
            (Icon::Filter, "ab-filter"),
        ] {
            ax -= 22.0;
            let int = ui.interact_rect(salt, [ax, py + 6.0, 22.0, 22.0]);
            if int.hovered {
                ui.paint_rect_filled([ax, py + 6.0], [22.0, 22.0], theme.surface_active, 4.0);
            }
            paint::icon(ui, [ax + 5.0, py + 11.0], icon, 13.0, theme.text_dim);
            match salt {
                "ab-trash" if int.clicked => {
                    header_delete_selected = true;
                }
                "ab-filter" => {
                    ui.context_menu_last(&mut |menu| {
                        for (kind, label, _icon) in SIDEBAR_CATEGORIES {
                            if menu.button(label) {
                                header_filter_pick = Some(if *kind == AssetTileKind::Unknown {
                                    None
                                } else {
                                    Some(*kind)
                                });
                                menu.close_menu();
                            }
                        }
                    });
                }
                "ab-more" => {
                    ui.context_menu_last(&mut |menu| {
                        if menu.button("New Folder") {
                            header_new_folder = true;
                            menu.close_menu();
                        }
                        if menu.button("New Script") {
                            header_new_script = true;
                            menu.close_menu();
                        }
                        if menu.button("Reveal Project in Explorer") {
                            header_reveal_project = true;
                            menu.close_menu();
                        }
                    });
                }
                _ => {}
            }
        }
        if let Some(pick) = header_filter_pick {
            self.selected_filter = pick;
        }
        if header_delete_selected {
            self.ask_to_delete_selected();
        }
        if header_new_folder {
            let parent = self.current_folder.clone().unwrap_or_default();
            self.queue_new_folder(&parent);
        }
        if header_new_script {
            let from = self.current_folder.clone().unwrap_or_default();
            self.start_new_script(&from);
        }
        if header_reveal_project {
            self.reveal_project();
        }
    }

    /// Paints the sidebar: the category filters, then the folder tree.
    fn show_sidebar(&mut self, ui: &mut dyn UiBuilder, theme: &UiTheme, sidebar: [f32; 4]) {
        let [px, body_y, sidebar_w, body_h] = sidebar;
        ui.paint_rect_filled(
            [px, body_y],
            [sidebar_w, body_h],
            theme.surface_elevated,
            0.0,
        );
        ui.paint_line(
            [px + sidebar_w, body_y],
            [px + sidebar_w, body_y + body_h],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        let mut row_y = body_y + 8.0;
        let row_h = 22.0;
        let mut new_filter = self.selected_filter;
        for (kind, label, icon) in SIDEBAR_CATEGORIES {
            let count = if *kind == AssetTileKind::Unknown {
                self.flat.len()
            } else {
                self.flat.iter().filter(|a| a.tile == *kind).count()
            };
            let active = self.selected_filter == Some(*kind)
                || (self.selected_filter.is_none() && *kind == AssetTileKind::Unknown);
            let row_x = px + 4.0;
            let row_w = sidebar_w - 8.0;
            let interaction =
                ui.interact_rect(&format!("ab-side-{}", label), [row_x, row_y, row_w, row_h]);
            if active {
                ui.paint_rect_filled(
                    [row_x, row_y],
                    [row_w, row_h],
                    with_alpha(theme.primary, 0.14),
                    theme.radius_sm,
                );
            } else if interaction.hovered {
                ui.paint_rect_filled(
                    [row_x, row_y],
                    [row_w, row_h],
                    with_alpha(theme.surface_active, 0.5),
                    theme.radius_sm,
                );
            }
            paint::icon(
                ui,
                [row_x + 8.0, row_y + 5.0],
                *icon,
                12.0,
                if active {
                    theme.primary
                } else {
                    theme.text_dim
                },
            );
            paint::text(
                ui,
                [row_x + 26.0, row_y + 5.0],
                label,
                11.5,
                if active { theme.text } else { theme.text_dim },
            );
            ui.paint_text_styled(
                [row_x + row_w - 6.0, row_y + 5.0],
                &format!("{}", count),
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
            if interaction.clicked {
                new_filter = if *kind == AssetTileKind::Unknown {
                    None
                } else {
                    Some(*kind)
                };
            }
            row_y += row_h + 1.0;
        }
        self.selected_filter = new_filter;

        row_y += 8.0;
        paint::text(ui, [px + 8.0, row_y], "FOLDERS", 10.0, theme.text_muted);
        row_y += 16.0;

        let _ = self.render_folder_tree(ui, px, row_y, sidebar_w, theme);
    }
}
