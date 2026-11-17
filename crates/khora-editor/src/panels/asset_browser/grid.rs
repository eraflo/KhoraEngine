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

//! The asset browser's file grid: its toolbar, its tiles and what they ask for.

use std::collections::HashSet;

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::asset::{AssetType, AssetUUID};
use khora_sdk::khora_core::problems::Problem;

use crate::drag_payload::{pack_asset_drag, payload_is_entity, unpack_entity};
use crate::widgets::tile::{paint_asset_tile, AssetTile, AssetTileKind};
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

use super::naming::ensure_extension;
use super::new_script::paint_new_script_tile;
use super::{AssetBrowserPanel, FlatAsset, TILE_GAP, TILE_SIZE, TOOLBAR_HEIGHT};

impl AssetBrowserPanel {
    /// Shows the file grid: its toolbar, the tiles, a rename in progress and
    /// the search field.
    pub(super) fn show_grid(&mut self, ui: &mut dyn UiBuilder, theme: &UiTheme, grid: [f32; 4]) {
        let [grid_x, body_y, grid_w, body_h] = grid;
        let crumb_y = body_y;
        let (project_folder, problems, quarantined) = self
            .state
            .lock()
            .ok()
            .map(|s| {
                (
                    s.project_folder.clone().unwrap_or_default(),
                    s.problems.clone(),
                    (s.quarantined.iter())
                        .map(|asset| asset.rel_path.clone())
                        .collect::<std::collections::HashSet<_>>(),
                )
            })
            .unwrap_or_default();
        let [search_x, search_y, search_w] =
            self.show_toolbar(ui, theme, [grid_x, crumb_y, grid_w], &project_folder);

        let grid_inner_x = grid_x + 12.0;
        let grid_inner_y = crumb_y + TOOLBAR_HEIGHT + 12.0;
        let grid_inner_w = grid_w - 24.0;
        let cols = ((grid_inner_w + TILE_GAP) / (TILE_SIZE + TILE_GAP))
            .max(1.0)
            .floor() as usize;

        let visible: Vec<(usize, FlatAsset)> = self
            .grid_tiles()
            .into_iter()
            .map(|idx| (idx, self.flat[idx].clone()))
            .collect();

        // The grid-wide drop target for scene-tree entities is registered
        // before the tiles, so their own rects take the clicks; otherwise it
        // would absorb them. Its drop is read now, while `last_response` is it.
        let drop_y = grid_inner_y;
        let drop_h = (body_y + body_h - grid_inner_y).max(0.0);
        let start_script_from =
            self.show_grid_background(ui, [grid_inner_x, drop_y, grid_inner_w, drop_h]);

        let tile_h = TILE_SIZE + 22.0;
        // Tiles sit at computed rects: the grid scrolls by offsetting its
        // origin and clipping.
        let grid_view = [grid_inner_x, grid_inner_y, grid_inner_w, drop_h];
        // A script being named takes the first slot; the tiles follow it.
        let lead = usize::from(self.new_script.is_some());
        let rows_total = (visible.len() + lead).div_ceil(cols.max(1)) as f32;
        let grid_content_h = rows_total * (tile_h + TILE_GAP);
        self.grid_scroll.update(ui, grid_view, grid_content_h);
        ui.push_clip_rect(grid_view);
        let grid_origin_y = grid_inner_y - self.grid_scroll.offset();
        let layout = GridLayout {
            x: grid_inner_x,
            origin_y: grid_origin_y,
            view: grid_view,
            cols,
            lead,
            tile_h,
        };
        // An open file and its parts sit on one band, behind their tiles.
        self.paint_part_bands(
            ui,
            &visible,
            |i| {
                let [tx, ty] = layout.slot(i);
                [tx, ty, TILE_SIZE, tile_h]
            },
            theme,
        );

        let new_script_file = self.new_script_file_name();
        let new_script_event = paint_new_script_tile(
            ui,
            [grid_inner_x, grid_origin_y],
            [TILE_SIZE, tile_h],
            theme,
            self.new_script.as_mut(),
            &mut self.rename_focus_pending,
            new_script_file.as_deref(),
        );

        let clicks = self.paint_tiles(ui, theme, layout, &visible, &problems, &quarantined);
        ui.pop_clip_rect();
        khora_tool_ui::widgets::scrollbar(
            ui,
            theme,
            grid_view,
            grid_content_h,
            &mut self.grid_scroll,
            "ab-grid-scroll",
        );
        self.apply_tile_clicks(ui, theme, clicks);

        if visible.is_empty() && self.new_script.is_none() {
            ui.paint_text_styled(
                [grid_inner_x + grid_inner_w * 0.5, grid_inner_y + 60.0],
                "No assets match the current filter.",
                12.0,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
        }

        self.show_tile_rename(ui, layout, &visible);
        match new_script_event {
            InlineEditEvent::Committed => self.commit_new_script(),
            InlineEditEvent::Cancelled => self.cancel_new_script(),
            _ => {}
        }
        if let Some(from) = start_script_from {
            self.start_new_script(&from);
        }

        let search_filter_ref = &mut self.search_filter;
        ui.region_at(
            "asset-browser-search",
            [search_x + 20.0, search_y, search_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );
    }

    /// Paints the toolbar above the grid: the breadcrumb and the search icon;
    /// returns the search field's left, top and width.
    fn show_toolbar(
        &mut self,
        ui: &mut dyn UiBuilder,
        theme: &UiTheme,
        [grid_x, crumb_y, grid_w]: [f32; 3],
        project_folder: &str,
    ) -> [f32; 3] {
        ui.paint_rect_filled(
            [grid_x, crumb_y],
            [grid_w, TOOLBAR_HEIGHT],
            theme.surface,
            0.0,
        );
        ui.paint_line(
            [grid_x, crumb_y + TOOLBAR_HEIGHT],
            [grid_x + grid_w, crumb_y + TOOLBAR_HEIGHT],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        let search_w = 200.0_f32.min(grid_w * 0.4);
        let search_x = grid_x + grid_w - search_w - 12.0;
        let search_y = crumb_y + 4.0;

        let crumb_max_w = (search_x - grid_x - 24.0).max(0.0);
        self.render_breadcrumb(
            ui,
            grid_x + 12.0,
            crumb_y + 7.0,
            crumb_max_w,
            theme,
            project_folder,
        );

        paint::icon(
            ui,
            [search_x + 4.0, search_y + 5.0],
            Icon::Search,
            12.0,
            theme.text_muted,
        );
        [search_x, search_y, search_w]
    }

    /// Takes an entity dropped on the grid and offers the grid's background
    /// menu; returns the folder a new script starts from, when one was asked for.
    fn show_grid_background(
        &mut self,
        ui: &mut dyn UiBuilder,
        drop_rect: [f32; 4],
    ) -> Option<String> {
        let _drop_int = ui.interact_rect("ab-grid-drop", drop_rect);
        if let Some(payload) = ui.dnd_take_drop_payload() {
            if payload_is_entity(payload) {
                let entity = unpack_entity(payload);
                self.queue_save_entity_as_prefab(entity);
            }
        }

        // The empty background's menu, on the grid-wide response: over a tile,
        // the tile's own menu, registered later, wins.
        let mut bg_new_folder = false;
        let mut bg_new_script = false;
        let mut bg_reveal_current = false;
        ui.context_menu_last(&mut |menu| {
            if menu.button("New Folder") {
                bg_new_folder = true;
                menu.close_menu();
            }
            if menu.button("New Script") {
                bg_new_script = true;
                menu.close_menu();
            }
            if menu.button("Reveal Current Folder") {
                bg_reveal_current = true;
                menu.close_menu();
            }
        });
        // Started once the grid has released the borrowed asset list.
        let start_script_from =
            bg_new_script.then(|| self.current_folder.clone().unwrap_or_default());
        if bg_new_folder || bg_reveal_current {
            let cur = self.current_folder.clone().unwrap_or_default();
            if bg_new_folder {
                self.queue_new_folder(&cur);
            }
            if bg_reveal_current {
                self.reveal_in_explorer(&cur, true);
            }
        }
        start_script_from
    }

    /// Paints the visible tiles with their drag, drop and menu; returns what
    /// was asked of them.
    fn paint_tiles(
        &self,
        ui: &mut dyn UiBuilder,
        theme: &UiTheme,
        layout: GridLayout,
        visible: &[(usize, FlatAsset)],
        problems: &[Problem],
        quarantined: &HashSet<String>,
    ) -> TileClicks {
        // A misplaced `.erg`'s error counts in its badge too.
        let counts_of =
            |rel_path: &str| crate::panels::problems::view::script_tile_counts(rel_path, problems);
        let mut clicks = TileClicks::default();
        for (i, (orig_idx, asset)) in visible.iter().enumerate() {
            let [tx, ty] = layout.slot(i);
            let tile_h = layout.tile_h;
            let grid_view = layout.view;
            // Rows out of view are skipped, not painted under the clip.
            if ty + tile_h < grid_view[1] || ty > grid_view[1] + grid_view[3] {
                continue;
            }
            let selected = self.selected_index == Some(*orig_idx);
            let id_salt = format!("tile-{orig_idx}");
            let tile = AssetTile {
                id_salt: &id_salt,
                origin: [tx, ty],
                size: [TILE_SIZE, tile_h],
                name: &asset.name,
                kind: asset.tile,
                selected,
                badge: counts_of(&asset.rel_path).and_then(crate::widgets::tile_badge::tile_badge),
                quarantined: asset.container.is_none() && quarantined.contains(&asset.rel_path),
            };
            let interaction = paint_asset_tile(ui, tile, theme);
            // Stamped with the epoch `flat` was built at, the one `orig_idx`
            // indexes into.
            ui.dnd_attach_drag_payload(pack_asset_drag(
                *orig_idx as u32,
                self.last_epoch.unwrap_or(0),
            ));
            if ui.is_last_item_dragged() {
                clicks.dragging_ghost = Some((asset.name.clone(), asset.tile));
            }
            // Tiles cover the grid, so the grid-wide drop target never sees an
            // entity released onto a tile.
            if let Some(payload) = ui.dnd_take_drop_payload() {
                if payload_is_entity(payload) {
                    clicks.entity_drop = Some(unpack_entity(payload));
                }
            }
            let idx = *orig_idx;
            let rel = asset.rel_path.clone();
            let is_material = asset.asset_type == AssetType::Material;
            let (uuid, name) = (asset.uuid, asset.name.clone());
            let file_ops = asset.container.is_none();
            ui.context_menu_last(&mut |menu| {
                if menu.button("Open") {
                    clicks.to_activate = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Reveal in Explorer") {
                    clicks.to_reveal = Some(idx);
                    menu.close_menu();
                }
                if is_material && menu.button("Assign to selected") {
                    clicks.to_assign_material = Some((uuid, name.clone()));
                    menu.close_menu();
                }
                if !file_ops {
                    return;
                }
                menu.separator();
                if menu.button("Rename") {
                    clicks.to_rename = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Duplicate") {
                    clicks.to_duplicate = Some(rel.clone());
                    menu.close_menu();
                }
                if menu.button("Delete") {
                    clicks.to_delete = Some(rel.clone());
                    menu.close_menu();
                }
            });
            if self.paint_part_chip(ui, *orig_idx, [tx, ty], theme) {
                clicks.to_toggle = Some(*orig_idx);
            }
            if interaction.double_clicked {
                clicks.to_activate = Some(*orig_idx);
            } else if interaction.clicked {
                clicks.to_select = Some(*orig_idx);
            }
        }
        clicks
    }

    /// Carries out what the tiles asked for: the drag ghost, a selection, an
    /// opening and the menu's actions.
    fn apply_tile_clicks(&mut self, ui: &mut dyn UiBuilder, theme: &UiTheme, clicks: TileClicks) {
        let TileClicks {
            to_select,
            to_activate,
            to_assign_material,
            to_reveal,
            to_rename,
            to_duplicate,
            to_delete,
            dragging_ghost,
            entity_drop,
            to_toggle,
        } = clicks;
        // After the clip is popped, so the ghost follows the cursor anywhere.
        if let Some((name, kind)) = dragging_ghost {
            if let Some(pos) = ui.pointer_position() {
                self.paint_drag_ghost(ui, pos, &name, kind, theme);
            }
        }
        if let Some(entity) = entity_drop {
            self.queue_save_entity_as_prefab(entity);
        }
        if let Some(idx) = to_toggle {
            self.toggle_container(idx);
        }
        if let Some((uuid, name)) = to_assign_material {
            if let Ok(mut state) = self.state.lock() {
                if state.selection.is_empty() {
                    log::warn!(
                        "Asset browser: assign material '{name}' ignored — no entity selected"
                    );
                } else {
                    state.push_request(EditorRequest::AssignMaterial { material: uuid });
                    log::info!("Asset browser: assigning material '{name}' to selection");
                }
            }
        }
        if let Some(idx) = to_reveal {
            if let Some(rel) = self.flat.get(idx).map(|a| a.rel_path.clone()) {
                self.reveal_in_explorer(&rel, false);
            }
        }
        if let Some(idx) = to_rename {
            // The rename field is drawn over the tile further down.
            if let Some(asset) = self.flat.get(idx) {
                self.rename_buffer = asset.name.clone();
                self.renaming_index = Some(idx);
                self.rename_focus_pending = true;
            }
        }
        if let Some(rel) = to_duplicate {
            if let Ok(mut state) = self.state.lock() {
                state.push_request(EditorRequest::AssetFile(AssetFileOp::Duplicate {
                    path: rel.clone(),
                }));
            }
            log::info!("Asset browser: duplicating '{rel}'");
        }
        if let Some(rel) = to_delete {
            self.confirm_delete = Some((rel, false));
        }
        if let Some(i) = to_select {
            self.selected_index = Some(i);
            if let Some(asset) = self.flat.get(i) {
                if let Ok(mut state) = self.state.lock() {
                    // The entity selection is cleared, or its inspector would
                    // show under the asset's.
                    state.inspected_asset_path = Some(asset.rel_path.clone());
                    state.selected_asset = Some(i);
                    state.clear_selection();
                    state.inspected = None;
                }
                log::info!(
                    "Asset selected: {} ({:?}) — {}",
                    asset.name,
                    asset.tile,
                    asset.rel_path
                );
            }
        }
        if let Some(i) = to_activate {
            self.selected_index = Some(i);
            if let Some(asset) = self.flat.get(i).cloned() {
                self.activate_asset(&asset);
            }
        }
    }

    /// Shows the rename field over the tile being renamed; Enter commits and
    /// Escape cancels, as the builder reports no focus loss.
    fn show_tile_rename(
        &mut self,
        ui: &mut dyn UiBuilder,
        layout: GridLayout,
        visible: &[(usize, FlatAsset)],
    ) {
        let Some(rename_idx) = self.renaming_index else {
            return;
        };
        let slot = visible
            .iter()
            .position(|(oi, _)| *oi == rename_idx)
            .map(|pos| {
                let [tx, ty] = layout.slot(pos);
                [tx + 3.0, ty + TILE_SIZE - 2.0, TILE_SIZE - 6.0, 20.0]
            });
        // Cloned before the buffer is borrowed mutably.
        let asset = self.flat.get(rename_idx).cloned();
        match (slot, asset) {
            (Some(rect), Some(asset)) => {
                let focus = self.rename_focus_pending;
                let salt = format!("tile-rename-{rename_idx}");
                let buf = &mut self.rename_buffer;
                let event = ui.inline_text_field(rect, &salt, buf, focus);
                self.rename_focus_pending = false;
                match event {
                    InlineEditEvent::Committed => {
                        let edited = self.rename_buffer.trim().to_string();
                        if !edited.is_empty() {
                            let final_name = ensure_extension(&edited, &asset.name);
                            let new_rel = if asset.folder.is_empty() {
                                final_name
                            } else {
                                format!("{}/{}", asset.folder, final_name)
                            };
                            if new_rel != asset.rel_path {
                                if let Ok(mut state) = self.state.lock() {
                                    state.push_request(EditorRequest::AssetFile(
                                        AssetFileOp::Rename {
                                            from: asset.rel_path.clone(),
                                            to: new_rel,
                                        },
                                    ));
                                }
                                log::info!("Asset browser: renaming '{}'", asset.rel_path);
                            }
                        }
                        self.renaming_index = None;
                    }
                    InlineEditEvent::Cancelled => self.renaming_index = None,
                    _ => {}
                }
            }
            // The tile was filtered out or removed — abandon the rename.
            _ => self.renaming_index = None,
        }
    }
}

/// Where the grid's tiles sit this frame.
#[derive(Clone, Copy)]
struct GridLayout {
    /// Left of the first column.
    x: f32,
    /// Top of the first row, scrolled.
    origin_y: f32,
    /// The visible part of the grid, which the tiles are clipped to.
    view: [f32; 4],
    cols: usize,
    /// Slots taken before the first tile: one while a script is being named.
    lead: usize,
    tile_h: f32,
}

impl GridLayout {
    /// Returns the top-left corner of the `i`-th visible tile.
    fn slot(&self, i: usize) -> [f32; 2] {
        let col = (i + self.lead) % self.cols;
        let row = (i + self.lead) / self.cols;
        [
            self.x + col as f32 * (TILE_SIZE + TILE_GAP),
            self.origin_y + row as f32 * (self.tile_h + TILE_GAP),
        ]
    }
}

/// What the tiles asked for this frame, carried out once the grid is drawn.
#[derive(Default)]
struct TileClicks {
    to_select: Option<usize>,
    to_activate: Option<usize>,
    to_assign_material: Option<(AssetUUID, String)>,
    to_reveal: Option<usize>,
    to_rename: Option<usize>,
    to_duplicate: Option<String>,
    to_delete: Option<String>,
    dragging_ghost: Option<(String, AssetTileKind)>,
    entity_drop: Option<khora_sdk::prelude::ecs::EntityId>,
    to_toggle: Option<usize>,
}
