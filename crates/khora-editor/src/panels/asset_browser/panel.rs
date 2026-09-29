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
use crate::widgets::tile::{paint_asset_tile, AssetTileKind};
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

use super::naming::ensure_extension;
use super::{
    AssetBrowserPanel, FlatAsset, HEADER_HEIGHT, SIDEBAR_WIDTH, TILE_GAP, TILE_SIZE, TOOLBAR_HEIGHT,
};
use crate::drag_payload::{pack_asset_drag, payload_is_entity, unpack_entity};

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

        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, ph] = panel_rect;

        // ── Header ────────────────────────────────────
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);
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

        // Header action buttons. Trash deletes the current selection; Filter
        // and More open small context menus. Effects are collected here and
        // applied after the loop so the closures don't need `&mut self`.
        let mut header_delete_selected = false;
        let mut header_filter_pick: Option<Option<AssetTileKind>> = None;
        let mut header_new_folder = false;
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
            match self.selected_index.and_then(|i| self.flat.get(i)) {
                Some(asset) => {
                    self.confirm_delete = Some((asset.rel_path.clone(), false));
                }
                None => log::info!("Asset browser: nothing selected to delete"),
            }
        }
        if header_new_folder {
            let parent = self.current_folder.clone().unwrap_or_default();
            self.queue_new_folder(&parent);
        }
        if header_reveal_project {
            self.reveal_project();
        }

        // ── Layout: sidebar | grid ────────────────────
        let body_y = py + HEADER_HEIGHT;
        let body_h = ph - HEADER_HEIGHT;
        let sidebar_w = SIDEBAR_WIDTH;

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

        // Type-category filter rows (compact, top of sidebar).
        let mut row_y = body_y + 8.0;
        let row_h = 22.0;
        let mut new_filter = self.selected_filter;
        for (kind, label, icon) in SIDEBAR_CATEGORIES {
            let count = if *kind == AssetTileKind::Unknown {
                self.flat.len()
            } else {
                self.flat.iter().filter(|a| a.asset_type == *kind).count()
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

        // Tree separator label.
        row_y += 8.0;
        paint::text(ui, [px + 8.0, row_y], "FOLDERS", 10.0, theme.text_muted);
        row_y += 16.0;

        // Folder tree (real VFS hierarchy).
        let _ = self.render_folder_tree(ui, px, row_y, sidebar_w, &theme);

        // ── Grid area ─────────────────────────────────
        let grid_x = px + sidebar_w;
        let grid_w = pw - sidebar_w;

        let crumb_y = body_y;
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
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone())
            .unwrap_or_default();

        let search_w = 200.0_f32.min(grid_w * 0.4);
        let search_x = grid_x + grid_w - search_w - 12.0;
        let search_y = crumb_y + 4.0;

        // Breadcrumb on the left, capped to leave room for the search.
        let crumb_max_w = (search_x - grid_x - 24.0).max(0.0);
        self.render_breadcrumb(
            ui,
            grid_x + 12.0,
            crumb_y + 7.0,
            crumb_max_w,
            &theme,
            &project_folder,
        );

        // Search affordance icon.
        paint::icon(
            ui,
            [search_x + 4.0, search_y + 5.0],
            Icon::Search,
            12.0,
            theme.text_muted,
        );

        // ── Tile grid ─────────────────────────────────
        let grid_inner_x = grid_x + 12.0;
        let grid_inner_y = crumb_y + TOOLBAR_HEIGHT + 12.0;
        let grid_inner_w = grid_w - 24.0;
        let cols = ((grid_inner_w + TILE_GAP) / (TILE_SIZE + TILE_GAP))
            .max(1.0)
            .floor() as usize;

        let filter_text = self.search_filter.to_lowercase();
        let current_folder = self.current_folder.clone().unwrap_or_default();
        let visible: Vec<(usize, &FlatAsset)> = self
            .flat
            .iter()
            .enumerate()
            .filter(|(_, a)| match self.selected_filter {
                Some(k) => a.asset_type == k,
                None => true,
            })
            .filter(|(_, a)| {
                // When a folder is selected, only show assets that live
                // *within* it (recursive — `assets/props` matches
                // `assets/props/crates/a.png`).
                if current_folder.is_empty() {
                    true
                } else {
                    a.folder == current_folder
                        || a.folder.starts_with(&format!("{}/", current_folder))
                }
            })
            .filter(|(_, a)| filter_text.is_empty() || a.name.to_lowercase().contains(&filter_text))
            .collect();

        // Drop target for entity drags from the scene tree. Registered
        // *before* the tile loop so the per-tile `interact_rect` calls
        // come later and take click priority — without this ordering
        // the grid-wide rect overlays the tiles and absorbs single
        // clicks (regression: tiles unselectable). The drop check
        // happens immediately, while `last_response` still points at
        // the grid rect; tiles registering after won't change the
        // already-read drop status.
        let drop_y = grid_inner_y;
        let drop_h = (body_y + body_h - grid_inner_y).max(0.0);
        let _drop_int =
            ui.interact_rect("ab-grid-drop", [grid_inner_x, drop_y, grid_inner_w, drop_h]);
        if let Some(payload) = ui.dnd_take_drop_payload() {
            if payload_is_entity(payload) {
                let entity = unpack_entity(payload);
                self.queue_save_entity_as_prefab(entity);
            }
        }

        // Right-click on the empty grid background. Bound to the same grid-wide
        // response as the drop target above; per-tile menus (registered later,
        // on top) take precedence when the cursor is over a tile.
        let mut bg_new_folder = false;
        let mut bg_reveal_current = false;
        ui.context_menu_last(&mut |menu| {
            if menu.button("New Folder") {
                bg_new_folder = true;
                menu.close_menu();
            }
            if menu.button("Reveal Current Folder") {
                bg_reveal_current = true;
                menu.close_menu();
            }
        });
        if bg_new_folder || bg_reveal_current {
            let cur = self.current_folder.clone().unwrap_or_default();
            if bg_new_folder {
                self.queue_new_folder(&cur);
            }
            if bg_reveal_current {
                self.reveal_in_explorer(&cur, true);
            }
        }

        let tile_h = TILE_SIZE + 22.0;
        let mut to_select: Option<usize> = None;
        let mut to_activate: Option<usize> = None;
        let mut to_assign_material: Option<String> = None;
        let mut to_reveal: Option<usize> = None;
        let mut to_rename: Option<usize> = None;
        let mut to_duplicate: Option<String> = None;
        let mut to_delete: Option<String> = None;
        // Name + kind of the tile currently being dragged, so we can paint a
        // cursor-following ghost after the grid (a visible "I'm carrying this").
        let mut dragging_ghost: Option<(String, AssetTileKind)> = None;
        // A scene-tree entity dropped onto a tile → save it as a prefab here.
        let mut entity_drop: Option<khora_sdk::prelude::ecs::EntityId> = None;
        // Tiles are placed at computed rects, so the grid scrolls by offsetting
        // its own origin and clipping — same shape as the Console and the
        // Hierarchy.
        let grid_view = [grid_inner_x, grid_inner_y, grid_inner_w, drop_h];
        let rows_total = visible.len().div_ceil(cols.max(1)) as f32;
        let grid_content_h = rows_total * (tile_h + TILE_GAP);
        self.grid_scroll.update(ui, grid_view, grid_content_h);
        ui.push_clip_rect(grid_view);
        let grid_origin_y = grid_inner_y - self.grid_scroll.offset();

        for (i, (orig_idx, asset)) in visible.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let tx = grid_inner_x + col as f32 * (TILE_SIZE + TILE_GAP);
            let ty = grid_origin_y + row as f32 * (tile_h + TILE_GAP);
            // Skip whole rows scrolled out of view rather than painting them
            // under the clip.
            if ty + tile_h < grid_view[1] || ty > grid_view[1] + grid_view[3] {
                continue;
            }
            let selected = self.selected_index == Some(*orig_idx);
            let interaction = paint_asset_tile(
                ui,
                &format!("tile-{}", orig_idx),
                [tx, ty],
                [TILE_SIZE, tile_h],
                &asset.name,
                asset.asset_type,
                selected,
                &theme,
            );
            // Every tile is a drag source — the low 32 bits carry the index
            // into `EditorState::asset_entries`. The drop sink (viewport /
            // folder tree) dispatches by the asset's type.
            // Stamped with the epoch `flat` was built for — that is the epoch
            // `orig_idx` is an index into.
            ui.dnd_attach_drag_payload(pack_asset_drag(
                *orig_idx as u32,
                self.last_epoch.unwrap_or(0),
            ));
            if ui.is_last_item_dragged() {
                dragging_ghost = Some((asset.name.clone(), asset.asset_type));
            }
            // A scene-tree entity released ONTO this tile saves it as a prefab.
            // Tiles blanket the populated grid, so without this the entity would
            // land on a tile (top hovered) and the grid-wide drop rect below
            // would never see it.
            if let Some(payload) = ui.dnd_take_drop_payload() {
                if payload_is_entity(payload) {
                    entity_drop = Some(unpack_entity(payload));
                }
            }
            // Generic per-tile context menu (same idiom the scene tree uses
            // for entity actions). Material tiles keep their "Assign to
            // selected" entry on top of the shared actions.
            let idx = *orig_idx;
            let rel = asset.rel_path.clone();
            let is_material = asset.type_name == "material";
            ui.context_menu_last(&mut |menu| {
                if menu.button("Open") {
                    to_activate = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Reveal in Explorer") {
                    to_reveal = Some(idx);
                    menu.close_menu();
                }
                if is_material && menu.button("Assign to selected") {
                    to_assign_material = Some(rel.clone());
                    menu.close_menu();
                }
                menu.separator();
                if menu.button("Rename") {
                    to_rename = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Duplicate") {
                    to_duplicate = Some(rel.clone());
                    menu.close_menu();
                }
                if menu.button("Delete") {
                    to_delete = Some(rel.clone());
                    menu.close_menu();
                }
            });
            if interaction.double_clicked {
                to_activate = Some(*orig_idx);
            } else if interaction.clicked {
                to_select = Some(*orig_idx);
            }
        }
        ui.pop_clip_rect();
        khora_tool_ui::widgets::scrollbar(
            ui,
            &theme,
            grid_view,
            grid_content_h,
            &mut self.grid_scroll,
            "ab-grid-scroll",
        );

        // Cursor-following drag ghost — painted after the clip is popped so it
        // can follow the cursor outside the grid.
        if let Some((name, kind)) = dragging_ghost {
            if let Some(pos) = ui.pointer_position() {
                self.paint_drag_ghost(ui, pos, &name, kind, &theme);
            }
        }
        if let Some(entity) = entity_drop {
            self.queue_save_entity_as_prefab(entity);
        }
        if let Some(rel) = to_assign_material {
            if let Ok(mut state) = self.state.lock() {
                if state.selection.is_empty() {
                    log::warn!(
                        "Asset browser: assign material '{}' ignored — no entity selected",
                        rel
                    );
                } else {
                    state.pending_assign_material = Some(rel.clone());
                    log::info!("Asset browser: assigning material '{}' to selection", rel);
                }
            }
        }
        if let Some(idx) = to_reveal {
            if let Some(rel) = self.flat.get(idx).map(|a| a.rel_path.clone()) {
                self.reveal_in_explorer(&rel, false);
            }
        }
        if let Some(idx) = to_rename {
            // Seed the shared buffer with the current file name and switch this
            // tile into inline-edit mode; the overlay is drawn near the search
            // box handling at the end of `ui()`.
            if let Some(asset) = self.flat.get(idx) {
                self.rename_buffer = asset.name.clone();
                self.renaming_index = Some(idx);
                self.rename_focus_pending = true;
            }
        }
        if let Some(rel) = to_duplicate {
            if let Ok(mut state) = self.state.lock() {
                state.pending_duplicate_asset = Some(rel.clone());
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
                    // Single-click switches the inspector to asset
                    // metadata mode (Phase 5). Clearing entity
                    // selection prevents the entity inspector from
                    // ghosting under the asset metadata pane.
                    state.inspected_asset_path = Some(asset.rel_path.clone());
                    state.selected_asset = Some(i);
                    state.clear_selection();
                    state.inspected = None;
                }
                log::info!(
                    "Asset selected: {} ({:?}) — {}",
                    asset.name,
                    asset.asset_type,
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

        if visible.is_empty() {
            ui.paint_text_styled(
                [grid_inner_x + grid_inner_w * 0.5, grid_inner_y + 60.0],
                "No assets match the current filter.",
                12.0,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
        }

        // Inline tile-rename overlay: a text field painted over the renaming
        // tile's label. Committed on Enter, cancelled on Escape (the builder
        // doesn't surface focus-loss, so those two keys are the commit path).
        if let Some(rename_idx) = self.renaming_index {
            let slot = visible
                .iter()
                .position(|(oi, _)| *oi == rename_idx)
                .map(|pos| {
                    let col = pos % cols;
                    let row = pos / cols;
                    let tx = grid_inner_x + col as f32 * (TILE_SIZE + TILE_GAP);
                    let ty = grid_inner_y + row as f32 * (tile_h + TILE_GAP);
                    [tx + 3.0, ty + TILE_SIZE - 2.0, TILE_SIZE - 6.0, 20.0]
                });
            // Snapshot the asset before taking the `&mut` buffer borrow.
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
                                        state.pending_rename_asset =
                                            Some((asset.rel_path.clone(), new_rel));
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

        let search_filter_ref = &mut self.search_filter;
        ui.region_at(
            "asset-browser-search",
            [search_x + 20.0, search_y, search_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );

        // Painted last so it sits above the grid it is asking about.
        self.render_delete_confirmation(ui, panel_rect, &theme);
    }
}
