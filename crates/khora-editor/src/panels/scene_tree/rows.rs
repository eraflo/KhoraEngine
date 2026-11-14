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

//! Rows of the tree: filtering, counting, drawing one node and its children.

use khora_sdk::editor_ui::*;

use super::{EditorAction, ROW_HEIGHT, ROW_PAD_X};
use crate::drag_payload::{pack_entity, payload_is_entity, unpack_asset_drag, unpack_entity};
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

/// Returns `node` filtered by a lowercase `needle`, `None` when nothing in it
/// matches.
///
/// A node whose name matches keeps its whole subtree; otherwise only its
/// matching descendants are kept.
pub(super) fn filter_scene_node(node: &SceneNode, needle: &str) -> Option<SceneNode> {
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

/// Returns how many nodes the forest holds, folded or not.
pub(super) fn count_scene_nodes(nodes: &[SceneNode]) -> usize {
    nodes
        .iter()
        .map(|n| 1 + count_scene_nodes(&n.children))
        .sum()
}

/// Returns an entity's display name in a `SceneNode` forest.
pub(super) fn find_node_name(
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

fn entity_icon(kind: EntityIcon) -> Icon {
    match kind {
        EntityIcon::Camera => Icon::Camera,
        EntityIcon::Light => Icon::Light,
        EntityIcon::Mesh => Icon::Cube,
        EntityIcon::Audio => Icon::Music,
        EntityIcon::Empty => Icon::Folder,
    }
}

/// Paints the row of `node` at `y` and, unless folded, its children's; returns
/// the y under the last row.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_node(
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
    // Refuses an asset drag whose index was read before a rescan.
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

    // The last 10px stay unclickable, so the dock splitter's grab band stays
    // reachable.
    let row_click_w = (row_w - 10.0).max(0.0);

    let interaction = ui.interact_rect(
        &format!("hier-row-{}", node.entity.index),
        [row_x, y, row_click_w, ROW_HEIGHT],
    );

    // Drag source and drop target share the row's one response, so clicks,
    // drags and drops do not steal each other's pointer. The payload carries
    // the generation, never a stale slot; `GameWorld::set_parent` refuses a cycle.
    ui.dnd_attach_drag_payload(pack_entity(node.entity));
    if let Some(packed) = ui.dnd_take_drop_payload() {
        if payload_is_entity(packed) {
            let dropped = unpack_entity(packed);
            if dropped != node.entity {
                pending.set(Some(EditorAction::Reparent {
                    child: dropped,
                    new_parent: Some(node.entity),
                }));
            }
        } else if let Some(idx) = unpack_asset_drag(packed, asset_epoch) {
            pending.set(Some(EditorAction::DropAsset {
                idx: idx as usize,
                target: Some(node.entity),
            }));
        }
    }

    // Selection is gold, as everywhere in the editor. See the book:
    // design/editor.md, "Accents — three signals only".
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

    let indent_px = ROW_PAD_X + depth as f32 * 14.0;
    let mut cx = row_x + indent_px;

    // A hit target of its own: folding does not also select the row.
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
        paint::icon(ui, [cx, y + 7.0], glyph, 11.0, colour);
    }
    cx += 14.0;

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
    paint::icon(ui, [cx, y + 6.0], icon, 13.0, icon_color);
    cx += 18.0;

    // The tags themselves are in the inspector.
    if node.tag_count > 0 {
        paint::icon(ui, [cx, y + 6.0], Icon::Tag, 12.0, icon_color);
        cx += 16.0;
    }

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
    // The caller draws the rename field here, so the recursion carries no
    // `&mut String`.
    if renaming == Some(node.entity) {
        let field_w = (row_x + row_click_w - cx - 6.0).max(40.0);
        rename_rect.set(Some([cx - 2.0, y + 3.0, field_w, ROW_HEIGHT - 6.0]));
    } else {
        paint::text(ui, [cx, y + 7.0], &node.name, 12.0, label_color);
    }

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

/// Returns how many rows a forest shows, folded subtrees left out, which is
/// the scroll extent.
pub(super) fn count_visible_nodes(
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
