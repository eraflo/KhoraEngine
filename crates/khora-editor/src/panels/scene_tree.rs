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

//! Scene Tree panel — the entity hierarchy: search, foldable rows with a
//! chevron and a type icon, and the branded gold selection bar.
//!
//! `EditorState::hidden_entities` still dims the rows it names, but nothing
//! populates it: the visibility eye was removed because it only greyed the row
//! while the object kept rendering. The set is left in place as the seam a
//! component-activation model would plug into.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::chrome::paint_panel_header;
use crate::widgets::paint::{paint_hairline_h, paint_icon, paint_text_size, with_alpha};

/// Filters a `SceneNode` against a lowercase needle. Returns `Some(node)`
/// when the node's own name contains the needle (subtree kept intact) or
/// when any descendant matches (only matching descendants kept). Returns
/// `None` when neither the node nor any descendant matches.
fn filter_scene_node(node: &SceneNode, needle: &str) -> Option<SceneNode> {
    if node.name.to_lowercase().contains(needle) {
        return Some(node.clone());
    }
    let kept: Vec<SceneNode> = node
        .children
        .iter()
        .filter_map(|c| filter_scene_node(c, needle))
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(SceneNode {
            children: kept,
            ..node.clone()
        })
    }
}

/// Recursively counts every visible node in a subtree (self + children).
fn count_scene_nodes(nodes: &[SceneNode]) -> usize {
    nodes
        .iter()
        .map(|n| 1 + count_scene_nodes(&n.children))
        .sum()
}

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 32.0;
const ROW_PAD_X: f32 = 8.0;

pub struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// How far the entity list is scrolled.
    scroll: khora_tool_ui::widgets::ScrollState,
    /// Entities whose subtree is folded away.
    ///
    /// Stored as the *exception* — collapsed rather than expanded — so a
    /// freshly loaded scene shows its whole hierarchy, and a newly spawned
    /// child appears without the user having to open anything.
    collapsed: std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    /// Whether the rename field already took focus for the current rename, so
    /// it is requested once rather than every frame (which would trap it).
    rename_focused: bool,
}

/// Finds an entity's display name in a `SceneNode` forest.
fn find_node_name(
    nodes: &[SceneNode],
    entity: khora_sdk::prelude::ecs::EntityId,
) -> Option<String> {
    for node in nodes {
        if node.entity == entity {
            return Some(node.name.clone());
        }
        if let Some(found) = find_node_name(&node.children, entity) {
            return Some(found);
        }
    }
    None
}

impl SceneTreePanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            scroll: khora_tool_ui::widgets::ScrollState::default(),
            collapsed: std::collections::HashSet::new(),
            rename_focused: false,
        }
    }
}

fn entity_icon(kind: EntityIcon) -> Icon {
    match kind {
        EntityIcon::Camera => Icon::Camera,
        EntityIcon::Light => Icon::Light,
        EntityIcon::Mesh => Icon::Cube,
        EntityIcon::Audio => Icon::Music,
        EntityIcon::Empty => Icon::Folder,
    }
}

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

/// Routes an asset dropped onto the hierarchy to the right `pending_*` action,
/// mirroring the viewport's drop dispatch. Meshes spawn at the world origin
/// (a tree has no 3D drop point), prefabs instantiate, scenes load, and a
/// texture/material assigns to the `target` row (or the selection).
fn dispatch_asset_drop(
    state: &mut EditorState,
    idx: usize,
    target: Option<khora_sdk::prelude::ecs::EntityId>,
) {
    let Some(entry) = state.asset_entries.get(idx).cloned() else {
        return;
    };
    let rel = entry.source_path.clone();
    match entry.asset_type.as_str() {
        "mesh" => {
            // Dropped on a row → parent the new entity under it (Unity/Godot
            // convention); dropped on empty space → spawn at scene root.
            state.pending_spawn_mesh_asset = Some((rel, [0.0, 0.0, 0.0], target));
            log::info!(
                "Hierarchy: mesh '{}' dropped — spawning{}",
                entry.name,
                if target.is_some() {
                    " as child"
                } else {
                    " at root"
                }
            );
        }
        "prefab" => {
            state.pending_prefab_spawn = Some((rel, target));
            log::info!(
                "Hierarchy: prefab '{}' dropped — instantiating{}",
                entry.name,
                if target.is_some() {
                    " as child"
                } else {
                    " at root"
                }
            );
        }
        "scene" => {
            if let Some(pf) = state.project_folder.clone() {
                let abs = std::path::Path::new(&pf)
                    .join("assets")
                    .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                state.pending_scene_load = Some(abs.to_string_lossy().to_string());
                log::info!("Hierarchy: scene '{}' dropped — loading", entry.name);
            } else {
                log::warn!("Hierarchy: cannot load scene '{}' — no project folder", rel);
            }
        }
        "texture" | "material" => match target.or_else(|| state.selection.iter().copied().next()) {
            Some(entity) => {
                state.pending_assign_texture = Some((rel, entity));
                log::info!("Hierarchy: '{}' dropped — assigning to entity", entry.name);
            }
            None => log::warn!("Hierarchy: drop '{}' on an entity to assign it", entry.name),
        },
        other => log::info!("Hierarchy: asset type '{other}' is not droppable here"),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_node(
    ui: &mut dyn UiBuilder,
    node: &SceneNode,
    depth: u32,
    px: f32,
    pw: f32,
    y: f32,
    selection: &std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    hidden: &std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    theme: &UiTheme,
    pending: &std::cell::Cell<Option<EditorAction>>,
    // Current `EditorState::asset_epoch` — rejects an asset drag whose index
    // was read before a rescan.
    asset_epoch: u64,
    collapsed: &std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    // The entity being renamed, and where its field should go once found.
    renaming: Option<khora_sdk::prelude::ecs::EntityId>,
    rename_rect: &std::cell::Cell<Option<[f32; 4]>>,
) -> f32 {
    let row_x = px + 4.0;
    let row_w = pw - 8.0;
    let is_selected = selection.contains(&node.entity);
    let is_hidden = hidden.contains(&node.entity);

    // Leave the last 10px of the row unclickable so the dock splitter's grab
    // band stays reachable — a row that swallows the edge makes the panel feel
    // unresizable.
    let row_click_w = (row_w - 10.0).max(0.0);

    let interaction = ui.interact_rect(
        &format!("hier-row-{}", node.entity.index),
        [row_x, y, row_click_w, ROW_HEIGHT],
    );

    // Drag-and-drop wiring — both source and target attach to the SAME
    // interact_rect response above, so a single hit-target serves clicks,
    // drags, and drops without stealing each other's pointer events.
    // Payload is the row's `EntityId` packed into u64 (high 32 = generation,
    // low 32 = index) so reparent addresses the exact live entity, not a
    // stale slot. Cycle prevention happens in `GameWorld::set_parent`.
    ui.dnd_attach_drag_payload(pack_entity(node.entity));
    if let Some(packed) = ui.dnd_take_drop_payload() {
        // An entity drag reparents under this row; an asset-tile drag
        // instantiates into the scene (texture/material assigns to this row).
        if payload_is_entity(packed) {
            let dropped = unpack_entity(packed);
            if dropped != node.entity {
                pending.set(Some(EditorAction::Reparent {
                    child: dropped,
                    new_parent: Some(node.entity),
                }));
            }
        } else if let Some(idx) =
            crate::panels::asset_browser::unpack_asset_drag(packed, asset_epoch)
        {
            pending.set(Some(EditorAction::DropAsset {
                idx: idx as usize,
                target: Some(node.entity),
            }));
        }
    }

    // Selection is **gold** — the same mark the asset tiles, the palette and
    // the spine use. Silver is the brand colour and says "Khora"; gold says
    // "this is the thing you are acting on", and it has to mean only that for
    // the eye to find it instantly.
    if is_selected {
        ui.paint_rect_filled(
            [row_x, y],
            [row_w, ROW_HEIGHT],
            theme.surface_active,
            theme.radius_sm,
        );
        khora_tool_ui::widgets::paint::selection_bar(
            ui,
            [row_x, y, row_w, ROW_HEIGHT],
            theme.accent_c,
        );
    } else if interaction.hovered {
        ui.paint_rect_filled(
            [row_x, y],
            [row_w, ROW_HEIGHT],
            with_alpha(theme.surface_elevated, 0.6),
            theme.radius_sm,
        );
    }

    if interaction.clicked {
        pending.set(Some(EditorAction::Select(node.entity)));
    }

    // Right-click context menu on the row. We attach it to the same
    // interaction so right-clicking anywhere on the row (excluding the
    // eye target) opens the menu. Selecting the row first means actions
    // like Duplicate / Delete operate on the right entity even if it
    // wasn't already selected.
    let entity = node.entity;
    let node_name = node.name.clone();
    ui.context_menu_last(&mut |menu| {
        if menu.button("Rename") {
            pending.set(Some(EditorAction::Rename(entity)));
            menu.close_menu();
        }
        if menu.button("Duplicate") {
            pending.set(Some(EditorAction::Duplicate(entity)));
            menu.close_menu();
        }
        if menu.button("Save as Prefab…") {
            pending.set(Some(EditorAction::SaveAsPrefab(entity)));
            menu.close_menu();
        }
        if menu.button("Save Material as .kmat") {
            pending.set(Some(EditorAction::SaveAsMaterial(
                entity,
                node_name.clone(),
            )));
            menu.close_menu();
        }
        menu.separator();
        if menu.button("Delete") {
            pending.set(Some(EditorAction::Delete(entity)));
            menu.close_menu();
        }
    });

    // Indent
    let indent_px = ROW_PAD_X + depth as f32 * 14.0;
    let mut cx = row_x + indent_px;

    // Chevron — the fold control, and a hit target of its own so clicking it
    // folds without also re-selecting the row.
    if !node.children.is_empty() {
        let is_collapsed = collapsed.contains(&node.entity);
        let chev_rect = [cx - 3.0, y + 4.0, 17.0, 17.0];
        let chev = ui.interact_rect(&format!("st-chev-{}", node.entity.index), chev_rect);
        if chev.clicked {
            pending.set(Some(EditorAction::ToggleCollapse(node.entity)));
        }
        let glyph = if is_collapsed {
            Icon::ChevronRight
        } else {
            Icon::ChevronDown
        };
        let colour = if chev.hovered {
            theme.text
        } else {
            theme.text_muted
        };
        paint_icon(ui, [cx, y + 7.0], glyph, 11.0, colour);
    }
    cx += 14.0;

    // Icon
    let base_icon_color = if is_selected {
        theme.accent_c
    } else {
        theme.text_dim
    };
    let icon_color = if is_hidden {
        with_alpha(base_icon_color, 0.4)
    } else {
        base_icon_color
    };
    let icon = entity_icon(node.icon);
    paint_icon(ui, [cx, y + 6.0], icon, 13.0, icon_color);
    cx += 18.0;

    // Tag indicator — small glyph next to the type icon when the entity
    // carries any `Tag` component entries. Tooltip would show the actual
    // tag set; for now the glyph alone tells the user "this entity has
    // tags, look at the inspector for details".
    if node.tag_count > 0 {
        paint_icon(ui, [cx, y + 6.0], Icon::Tag, 12.0, icon_color);
        cx += 16.0;
    }

    // Label
    let base_label_color = if is_selected {
        theme.text
    } else {
        theme.text_dim
    };
    let label_color = if is_hidden {
        with_alpha(base_label_color, 0.45)
    } else {
        base_label_color
    };
    // While a row is being renamed its label is replaced by a field — drawn by
    // the caller after the loop, so the recursion doesn't have to carry a
    // `&mut String` down every level. The row just reports where it goes.
    if renaming == Some(node.entity) {
        let field_w = (row_x + row_click_w - cx - 6.0).max(40.0);
        rename_rect.set(Some([cx - 2.0, y + 3.0, field_w, ROW_HEIGHT - 6.0]));
    } else {
        paint_text_size(ui, [cx, y + 7.0], &node.name, 12.0, label_color);
    }

    // No visibility eye. It used to dim the row and nothing else — the object
    // kept rendering — because `pending_visibility_toggle` was never consumed.
    //
    // Hiding an object belongs to a component activation model (deactivate an
    // entity's render components), which every consuming `Flow` would have to
    // honour or the flag is decorative all over again. That is an ECS feature,
    // not an editor one; until it exists, no eye is better than a fake one.

    let mut next_y = y + ROW_HEIGHT;
    if collapsed.contains(&node.entity) {
        return next_y;
    }
    for child in &node.children {
        next_y = render_node(
            ui,
            child,
            depth + 1,
            px,
            pw,
            next_y,
            selection,
            hidden,
            theme,
            pending,
            asset_epoch,
            collapsed,
            renaming,
            rename_rect,
        );
    }
    next_y
}

/// Counts the rows a forest actually shows, stopping at folded nodes.
///
/// Used for the scroll extent: counting the whole tree would let the view
/// scroll past the end of a mostly-folded hierarchy.
fn count_visible_nodes(
    nodes: &[SceneNode],
    collapsed: &std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
) -> usize {
    nodes
        .iter()
        .map(|n| {
            1 + if collapsed.contains(&n.entity) {
                0
            } else {
                count_visible_nodes(&n.children, collapsed)
            }
        })
        .sum()
}

/// Packs an `EntityId` into a `u64` for drag-and-drop payloads. Layout:
/// high 32 = generation, low 32 = index. Also used by the asset
/// browser's drop handler to ingest entity drags from the scene tree
/// (drop = create prefab in the current folder).
pub(crate) fn pack_entity(e: khora_sdk::prelude::ecs::EntityId) -> u64 {
    ((e.generation as u64) << 32) | (e.index as u64)
}

/// Inverse of [`pack_entity`].
pub(crate) fn unpack_entity(payload: u64) -> khora_sdk::prelude::ecs::EntityId {
    khora_sdk::prelude::ecs::EntityId {
        index: payload as u32,
        generation: (payload >> 32) as u32,
    }
}

/// `true` when `payload` is *not* one of the asset-browser's
/// dedicated drag tags (the type-agnostic asset-tile tag). Lets receivers
/// disambiguate scene-tree entity payloads from asset-tile payloads on
/// the same `dnd_take_drop_payload` channel.
///
/// The check is conservative: any payload whose top 32 bits don't
/// match a known tag is assumed to be a packed `EntityId`. Entity
/// generations are tiny u32s (start at 1) so this can't collide with
/// the ASCII-encoded tag constants in
/// [`crate::panels::asset_browser`].
pub(crate) fn payload_is_entity(payload: u64) -> bool {
    !crate::panels::asset_browser::is_asset_drag(payload)
}

enum EditorAction {
    Select(khora_sdk::prelude::ecs::EntityId),
    /// Fold or unfold this node's subtree.
    ToggleCollapse(khora_sdk::prelude::ecs::EntityId),
    Rename(khora_sdk::prelude::ecs::EntityId),
    Duplicate(khora_sdk::prelude::ecs::EntityId),
    Delete(khora_sdk::prelude::ecs::EntityId),
    Spawn(String),
    /// Reparent `child` under `new_parent`, or detach it (root) when
    /// `new_parent` is `None`. Emitted by the scene tree drag-and-drop
    /// handler.
    Reparent {
        child: khora_sdk::prelude::ecs::EntityId,
        new_parent: Option<khora_sdk::prelude::ecs::EntityId>,
    },
    /// Save the entity's subtree (root + descendants) as a `.kprefab`
    /// asset. The path is picked through `rfd::FileDialog` in the
    /// command dispatcher.
    SaveAsPrefab(khora_sdk::prelude::ecs::EntityId),
    /// Save the entity's inline material to a `.kmat` asset and convert
    /// the entity to reference it. Carries the entity plus the chosen
    /// material name (file stem). No-op in the dispatcher when the entity
    /// has no inline material.
    SaveAsMaterial(khora_sdk::prelude::ecs::EntityId, String),
    /// An asset tile dropped onto the hierarchy: `idx` is the index into
    /// `EditorState::asset_entries`, `target` the entity row it landed on (if
    /// any). Dispatched by asset type — mesh/prefab/scene instantiate into the
    /// scene, texture/material assign to `target`.
    DropAsset {
        idx: usize,
        target: Option<khora_sdk::prelude::ecs::EntityId>,
    },
}
