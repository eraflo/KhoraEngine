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

//! Scene Tree panel — Hierarchy with Hierarchy/Layers/Tags tabs, search,
//! sectioned rows with chevron + icon + visibility eye, branded selection bar.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::chrome::{paint_panel_header, panel_tab};
use crate::widgets::paint::{paint_hairline_h, paint_icon, paint_text_size, with_alpha};

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 32.0;
const ROW_PAD_X: f32 = 8.0;

pub struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl SceneTreePanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self { state, theme }
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
        "scene_tree"
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

        let count = self.state.lock().ok().map(|s| s.entity_count).unwrap_or(0);
        let badge = format!("{}", count);

        // Action icons live on the right; we always keep them visible because
        // they hold the only entry point for "+" / filter. Tabs adapt around
        // the remaining space — Layers/Tags drop out first when cramped.
        let action_icons: &[(Icon, &str)] = &[
            (Icon::More, "h-act-more"),
            (Icon::Filter, "h-act-filter"),
            (Icon::Plus, "h-act-plus"),
        ];
        let icons_total_w = action_icons.len() as f32 * 22.0 + 8.0;
        let icons_left = px + pw - icons_total_w;

        let tab_x = px + 6.0;
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;
        // Single tab today — Layers and Tags were decorative and removed
        // until they're actually wired (filtered scene views per layer,
        // tag-based selection, etc.).
        let _ = panel_tab(
            ui,
            "h-tab-hierarchy",
            [tab_x, tab_y],
            "Hierarchy",
            Some(&badge),
            true,
            &theme,
        );
        let _ = icons_left;

        // ── Action icons (right) ──────────────────────
        // Inset 12px from the right edge so we don't compete with the
        // SidePanel resize-handle (8px grab zone).
        let mut ax = px + pw - 12.0;
        for (icon, salt) in action_icons {
            ax -= 22.0;
            let int = ui.interact_rect(salt, [ax, py + 6.0, 22.0, 22.0]);
            if int.hovered {
                ui.paint_rect_filled([ax, py + 6.0], [22.0, 22.0], theme.surface_active, 4.0);
            }
            paint_icon(ui, [ax + 5.0, py + 11.0], *icon, 13.0, theme.text_dim);
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
        let search_text = self
            .state
            .lock()
            .ok()
            .map(|s| s.search_filter.clone())
            .unwrap_or_default();
        let placeholder = if search_text.is_empty() {
            "Filter entities…"
        } else {
            search_text.as_str()
        };
        let placeholder_color = if search_text.is_empty() {
            theme.text_muted
        } else {
            theme.text
        };
        paint_text_size(
            ui,
            [search_x + 26.0, toolbar_y + 11.0],
            placeholder,
            11.5,
            placeholder_color,
        );
        // Only paint the "n / n" count when the pill is wide enough that it
        // can't visually collide with the placeholder text.
        if search_w >= 200.0 {
            let count_text = format!("{} / {}", count, count);
            ui.paint_text_styled(
                [search_x + search_w - 8.0, toolbar_y + 11.0],
                &count_text,
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
        }
        // Click on search → focus a hidden text input. We approximate by
        // letting the user type via Cmd+K (better UX) and relying on a real
        // egui text field below the painted region for now (kept invisible).

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
        let mut state_guard = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let roots: Vec<SceneNode> = state_guard.scene_roots.clone();
        let selected = state_guard.selection.clone();
        let hidden = state_guard.hidden_entities.clone();
        let pending: std::cell::Cell<Option<EditorAction>> = std::cell::Cell::new(None);

        let mut row_y = section_y + 18.0;
        for node in &roots {
            row_y = render_node(
                ui, node, 0, px, pw, row_y, &selected, &hidden, &theme, &pending,
            );
        }

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
                pending.set(Some(EditorAction::Reparent {
                    child: unpack_entity(packed),
                    new_parent: None,
                }));
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
                EditorAction::ToggleVisibility(eid) => {
                    state_guard.pending_visibility_toggle = Some(eid);
                    if state_guard.hidden_entities.contains(&eid) {
                        state_guard.hidden_entities.remove(&eid);
                    } else {
                        state_guard.hidden_entities.insert(eid);
                    }
                }
                EditorAction::Rename(eid) => {
                    state_guard.renaming_entity = Some(eid);
                    state_guard.rename_buffer.clear();
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
            }
        }
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
) -> f32 {
    let row_x = px + 4.0;
    let row_w = pw - 8.0;
    let is_selected = selection.contains(&node.entity);
    let is_hidden = hidden.contains(&node.entity);

    // Eye is its own hit-target on the right; the row interaction must NOT
    // overlap it, otherwise clicking the eye also selects the row (and
    // worse, both `interact_rect`s race on the same pointer event so neither
    // fires reliably). We also keep the eye 10px away from the panel edge
    // so the SidePanel's resize grab handle (8px hot zone) stays free.
    let eye_size = 22.0;
    let eye_inset_right = 10.0;
    let row_click_w = (row_w - eye_size - eye_inset_right - 4.0).max(0.0);

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
        let dropped = unpack_entity(packed);
        if dropped != node.entity {
            pending.set(Some(EditorAction::Reparent {
                child: dropped,
                new_parent: Some(node.entity),
            }));
        }
    }

    if is_selected {
        ui.paint_rect_filled(
            [row_x, y],
            [row_w, ROW_HEIGHT],
            with_alpha(theme.primary, 0.14),
            theme.radius_sm,
        );
        ui.paint_rect_filled(
            [row_x, y + 4.0],
            [2.0, ROW_HEIGHT - 8.0],
            theme.primary,
            1.0,
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
    ui.context_menu_last(&mut |menu| {
        if menu.button("Rename") {
            pending.set(Some(EditorAction::Rename(entity)));
            menu.close_menu();
        }
        if menu.button("Duplicate") {
            pending.set(Some(EditorAction::Duplicate(entity)));
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

    // Chevron (or empty space for leaves)
    if !node.children.is_empty() {
        paint_icon(ui, [cx, y + 7.0], Icon::ChevronDown, 11.0, theme.text_muted);
    }
    cx += 14.0;

    // Icon
    let base_icon_color = if is_selected {
        theme.primary
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
    paint_text_size(ui, [cx, y + 7.0], &node.name, 12.0, label_color);

    // Eye — own hit target so it can toggle visibility without selecting.
    let eye_x = row_x + row_w - eye_size - eye_inset_right;
    let eye_int = ui.interact_rect(
        &format!("hier-eye-{}", node.entity.index),
        [eye_x, y, eye_size, ROW_HEIGHT],
    );
    if eye_int.clicked {
        pending.set(Some(EditorAction::ToggleVisibility(node.entity)));
    }
    if eye_int.hovered {
        ui.paint_rect_filled(
            [eye_x, y + 2.0],
            [eye_size, ROW_HEIGHT - 4.0],
            with_alpha(theme.surface_active, 0.5),
            theme.radius_sm,
        );
    }
    let eye_color = if is_hidden {
        theme.text_muted
    } else if eye_int.hovered || interaction.hovered || is_selected {
        if is_selected {
            theme.primary
        } else {
            theme.text
        }
    } else {
        with_alpha(theme.text_muted, 0.5)
    };
    let eye_icon = if is_hidden { Icon::EyeOff } else { Icon::Eye };
    paint_icon(ui, [eye_x + 5.0, y + 7.0], eye_icon, 12.0, eye_color);

    let mut next_y = y + ROW_HEIGHT;
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
        );
    }
    next_y
}

/// Packs an `EntityId` into a `u64` for drag-and-drop payloads. Layout:
/// high 32 = generation, low 32 = index.
fn pack_entity(e: khora_sdk::prelude::ecs::EntityId) -> u64 {
    ((e.generation as u64) << 32) | (e.index as u64)
}

/// Inverse of [`pack_entity`].
fn unpack_entity(payload: u64) -> khora_sdk::prelude::ecs::EntityId {
    khora_sdk::prelude::ecs::EntityId {
        index: payload as u32,
        generation: (payload >> 32) as u32,
    }
}

enum EditorAction {
    Select(khora_sdk::prelude::ecs::EntityId),
    ToggleVisibility(khora_sdk::prelude::ecs::EntityId),
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
}
