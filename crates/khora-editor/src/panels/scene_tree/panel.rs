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

//! The scene tree as an editor panel: the frame it draws each tick.

use khora_sdk::editor_ui::*;

use super::drop::{dispatch_asset_drop, payload_is_entity, unpack_entity};
use super::rows::{
    count_scene_nodes, count_visible_nodes, filter_scene_node, find_node_name, render_node,
};
use super::{EditorAction, SceneTreePanel, HEADER_HEIGHT, ROW_HEIGHT, TOOLBAR_HEIGHT};
use crate::widgets::chrome::paint_panel_header;
use crate::widgets::paint::{paint_hairline_h, paint_icon, paint_text_size, with_alpha};

impl EditorPanel for SceneTreePanel {
    fn id(&self) -> &str {
        "khora.editor.scene_tree"
    }
    fn title(&self) -> &str {
        "Hierarchy"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(280.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, _] = panel_rect;

        // ── Header strip ──────────────────────────────
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);

        // Lock the editor state once and hold it through the whole panel.
        // The search field needs `&mut state.search_filter` for `text_edit_singleline`,
        // so the lock must outlive that closure — and it's cheaper to just
        // hold it for the full body than to reacquire it three times.
        let mut state_guard = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let total_count = state_guard.entity_count;

        // Action icons live on the right; we always keep them visible because
        // they hold the only entry point for "+" / filter. Tabs adapt around
        // the remaining space — Layers/Tags drop out first when cramped.
        // Only "+" survives, and it does something. The `More` and `Filter`
        // icons painted a hover highlight and discarded the click — the search
        // field below already filters, and `More` duplicated the row context
        // menu. An icon that lights up and does nothing costs more than it
        // saves.
        let icons_total_w = 30.0;
        let icons_left = px + pw - icons_total_w;
        let spawn_from_menu: std::cell::Cell<Option<String>> = std::cell::Cell::new(None);

        let tab_x = px + 6.0;
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;
        // Single tab today — Layers and Tags were decorative and removed
        // until they're actually wired (filtered scene views per layer,
        // tag-based selection, etc.).
        // Filter — applied to scene_roots before rendering.
        let filter_lower = state_guard.search_filter.to_lowercase();
        let filter_active = !filter_lower.is_empty();
        let filtered_roots: Vec<SceneNode> = if filter_active {
            state_guard
                .scene_roots
                .iter()
                .filter_map(|n| filter_scene_node(n, &filter_lower))
                .collect()
        } else {
            state_guard.scene_roots.clone()
        };
        let visible_count = if filter_active {
            count_scene_nodes(&filtered_roots)
        } else {
            total_count
        };

        let badge = if filter_active {
            format!("{}/{}", visible_count, total_count)
        } else {
            format!("{}", total_count)
        };

        // Only the count: the dock tab above already says "Scene Tree", and a
        // second title 20px below it was the same word twice.
        paint_text_size(
            ui,
            [tab_x, tab_y + 5.0],
            &badge,
            theme.font_size_caption,
            theme.text_muted,
        );
        let _ = icons_left;

        // ── Add entity (right) ────────────────────────
        // Inset 12px from the right edge so we don't compete with the dock
        // splitter's grab band.
        let add_rect = [px + pw - 34.0, py + 6.0, 22.0, 22.0];
        let add_int = ui.interact_rect("h-act-plus", add_rect);
        if add_int.hovered {
            ui.paint_rect_filled(
                [add_rect[0], add_rect[1]],
                [add_rect[2], add_rect[3]],
                theme.surface_active,
                4.0,
            );
        }
        paint_icon(
            ui,
            [add_rect[0] + 5.0, add_rect[1] + 5.0],
            Icon::Plus,
            13.0,
            if add_int.hovered {
                theme.text
            } else {
                theme.text_dim
            },
        );
        // Click spawns an empty entity; right-click offers the same list the
        // panel's background menu does, so the button is a shortcut rather than
        // a second, divergent way to create things.
        ui.context_menu_last(&mut |menu| {
            for kind in ["Empty", "Cube", "Sphere", "Plane", "Light", "Camera"] {
                if menu.button(kind) {
                    spawn_from_menu.set(Some(kind.to_owned()));
                    menu.close_menu();
                }
            }
        });
        if add_int.clicked {
            spawn_from_menu.set(Some("Empty".to_owned()));
        }
        if let Some(kind) = spawn_from_menu.take() {
            state_guard.pending_spawn = Some(kind);
        }

        // ── Search toolbar ────────────────────────────
        let toolbar_y = py + HEADER_HEIGHT;
        let search_x = px + 8.0;
        let search_w = pw - 16.0;
        let search_h = 24.0;
        ui.paint_rect_filled(
            [search_x, toolbar_y + 4.0],
            [search_w, search_h],
            theme.background,
            theme.radius_sm,
        );
        ui.paint_rect_stroke(
            [search_x, toolbar_y + 4.0],
            [search_w, search_h],
            with_alpha(theme.separator, 0.55),
            theme.radius_sm,
            1.0,
        );
        paint_icon(
            ui,
            [search_x + 8.0, toolbar_y + 9.0],
            Icon::Search,
            12.0,
            theme.text_muted,
        );
        // Optional "n / n" count on the right edge of the search pill —
        // only painted when there's room without overlapping the input.
        let count_w = if search_w >= 200.0 {
            let count_text = if filter_active {
                format!("{} / {}", visible_count, total_count)
            } else {
                format!("{}", total_count)
            };
            ui.paint_text_styled(
                [search_x + search_w - 8.0, toolbar_y + 11.0],
                &count_text,
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
            56.0
        } else {
            0.0
        };

        // Real text input — bound directly to `state.search_filter`.
        let input_w = (search_w - 32.0 - count_w).max(40.0);
        let search_filter_ref = &mut state_guard.search_filter;
        ui.region_at(
            "hierarchy-search",
            [search_x + 24.0, toolbar_y + 6.0, input_w, 20.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );

        // ── Section header ────────────────────────────
        let section_y = toolbar_y + TOOLBAR_HEIGHT + 4.0;
        ui.paint_text_styled(
            [px + 14.0, section_y],
            "ACTIVE SCENE",
            10.0,
            theme.text_muted,
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        paint_hairline_h(
            ui,
            px + 102.0,
            section_y + 6.0,
            pw - 110.0,
            with_alpha(theme.separator, 0.55),
        );

        // ── Rows ──────────────────────────────────────
        let selected = state_guard.selection.clone();
        let hidden = state_guard.hidden_entities.clone();
        let asset_epoch = state_guard.asset_epoch;
        let renaming = state_guard.renaming_entity;
        let rename_rect: std::cell::Cell<Option<[f32; 4]>> = std::cell::Cell::new(None);
        let pending: std::cell::Cell<Option<EditorAction>> = std::cell::Cell::new(None);

        // F2 starts a rename on the single selected entity, matching the
        // convention the context menu already advertises.
        if ui.key_pressed(khora_sdk::KeyCode::F2) {
            if let Some(entity) = state_guard.single_selected() {
                let name = find_node_name(&state_guard.scene_roots, entity).unwrap_or_default();
                state_guard.renaming_entity = Some(entity);
                state_guard.rename_buffer = name;
            }
        }

        // Rows live between the section header and the bottom of the panel.
        let rows_top = section_y + 18.0;
        let rows_area = [
            px,
            rows_top,
            pw,
            (panel_rect[1] + panel_rect[3] - rows_top).max(0.0),
        ];
        let content_h = count_visible_nodes(&filtered_roots, &self.collapsed) as f32 * ROW_HEIGHT;
        self.scroll.update(ui, rows_area, content_h);
        ui.push_clip_rect(rows_area);

        let mut row_y = rows_top - self.scroll.offset();
        for node in &filtered_roots {
            row_y = render_node(
                ui,
                node,
                0,
                px,
                pw,
                row_y,
                &selected,
                &hidden,
                &theme,
                &pending,
                asset_epoch,
                &self.collapsed,
                renaming,
                &rename_rect,
            );
        }
        // The rename field, drawn over the row that asked for it. Inside the
        // clip so a renamed row scrolled out of view takes its field with it.
        if let (Some(entity), Some(rect)) = (renaming, rename_rect.get()) {
            let take_focus = !self.rename_focused;
            self.rename_focused = true;
            let event = ui.inline_text_field(
                rect,
                "hier-rename",
                &mut state_guard.rename_buffer,
                take_focus,
            );
            match event {
                InlineEditEvent::Committed => {
                    let new_name = state_guard.rename_buffer.trim().to_owned();
                    if !new_name.is_empty() {
                        state_guard.push_edit(PropertyEdit::SetName(entity, new_name));
                    }
                    state_guard.renaming_entity = None;
                    self.rename_focused = false;
                }
                InlineEditEvent::Cancelled => {
                    state_guard.renaming_entity = None;
                    self.rename_focused = false;
                }
                _ => {}
            }
        } else if renaming.is_none() {
            self.rename_focused = false;
        }

        ui.pop_clip_rect();
        khora_tool_ui::widgets::scrollbar(
            ui,
            &theme,
            rows_area,
            content_h,
            &mut self.scroll,
            "hierarchy-scroll",
        );

        // Below this point `row_y` is a scrolled coordinate; the panel-wide
        // right-click area must sit under the *visible* rows, not the virtual
        // ones, or it would swallow clicks meant for the list.
        let row_y = row_y.max(rows_top).min(panel_rect[1] + panel_rect[3]);

        // ── Panel-wide right-click area ───────────────
        // The remaining empty space below the last row is its own hit
        // target with an "Add …" context menu — letting the user spawn
        // new entities without having to right-click an existing row.
        // It's also a drop target: dragging an entity here unparents it.
        let panel_bottom = panel_rect[1] + panel_rect[3];
        let empty_h = (panel_bottom - row_y).max(0.0);
        if empty_h > 4.0 {
            let _empty_int = ui.interact_rect("scene-tree-empty", [px, row_y, pw, empty_h]);
            if let Some(packed) = ui.dnd_take_drop_payload() {
                // Entity drags unparent to root; asset-tile drags instantiate
                // into the scene (both share this drop channel).
                if payload_is_entity(packed) {
                    pending.set(Some(EditorAction::Reparent {
                        child: unpack_entity(packed),
                        new_parent: None,
                    }));
                } else if let Some(idx) =
                    crate::panels::asset_browser::unpack_asset_drag(packed, asset_epoch)
                {
                    pending.set(Some(EditorAction::DropAsset {
                        idx: idx as usize,
                        target: None,
                    }));
                }
            }
            ui.context_menu_last(&mut |menu| {
                menu.menu_button("Add", &mut |sub| {
                    if sub.button("Empty") {
                        pending.set(Some(EditorAction::Spawn("Empty".to_owned())));
                        sub.close_menu();
                    }
                    if sub.button("Cube") {
                        pending.set(Some(EditorAction::Spawn("Cube".to_owned())));
                        sub.close_menu();
                    }
                    if sub.button("Sphere") {
                        pending.set(Some(EditorAction::Spawn("Sphere".to_owned())));
                        sub.close_menu();
                    }
                    if sub.button("Plane") {
                        pending.set(Some(EditorAction::Spawn("Plane".to_owned())));
                        sub.close_menu();
                    }
                    sub.separator();
                    if sub.button("Camera") {
                        pending.set(Some(EditorAction::Spawn("Camera".to_owned())));
                        sub.close_menu();
                    }
                    if sub.button("Light") {
                        pending.set(Some(EditorAction::Spawn("Light".to_owned())));
                        sub.close_menu();
                    }
                });
            });
        }

        if let Some(action) = pending.into_inner() {
            match action {
                EditorAction::Select(eid) => {
                    if state_guard.ctrl_held {
                        state_guard.toggle_select(eid);
                    } else {
                        state_guard.select(eid);
                    }
                }
                EditorAction::ToggleCollapse(eid) => {
                    if !self.collapsed.remove(&eid) {
                        self.collapsed.insert(eid);
                    }
                }
                EditorAction::Rename(eid) => {
                    // Seed with the current name so the field opens on it —
                    // renaming usually means editing, not retyping.
                    let current = find_node_name(&state_guard.scene_roots, eid).unwrap_or_default();
                    state_guard.renaming_entity = Some(eid);
                    state_guard.rename_buffer = current;
                    self.rename_focused = false;
                }
                EditorAction::Duplicate(eid) => {
                    state_guard.pending_duplicate = Some(eid);
                }
                EditorAction::Delete(eid) => {
                    state_guard.pending_delete = Some(eid);
                }
                EditorAction::Spawn(kind) => {
                    state_guard.pending_spawn = Some(kind);
                }
                EditorAction::Reparent { child, new_parent } => {
                    if Some(child) != new_parent {
                        state_guard.pending_reparent = Some((child, new_parent));
                    }
                }
                EditorAction::SaveAsPrefab(eid) => {
                    state_guard.pending_save_as_prefab = Some(eid);
                }
                EditorAction::SaveAsMaterial(eid, name) => {
                    state_guard.pending_save_as_material = Some((eid, name));
                }
                EditorAction::DropAsset { idx, target } => {
                    dispatch_asset_drop(&mut state_guard, idx, target);
                }
            }
        }
    }
}
