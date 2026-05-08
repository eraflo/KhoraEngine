// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Component card frame — chevron + icon + title + (toggle) + (trash) +
//! collapsible body. Used by the Properties tab to wrap each component
//! the inspected entity carries.

use khora_sdk::editor_ui::{EditorState, UiTheme, Icon, PropertyEdit, UiBuilder};
use khora_sdk::prelude::ecs::EntityId;

use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

const CARD_HEADER_H: f32 = 30.0;

/// Render one component as a card and call `body` for its content. The
/// card persists its open / closed state through
/// `EditorState::inspector_card_open` keyed by entity-index + title.
#[allow(clippy::too_many_arguments)]
pub fn render_card(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    title: &str,
    icon: Icon,
    enabled: Option<bool>,
    removable: bool,
    card_x: f32,
    card_w: f32,
    theme: &UiTheme,
    state: &mut EditorState,
    body: &mut dyn FnMut(&mut dyn UiBuilder),
) {
    let card_id = format!("{}::{}", entity.index, title);
    let open = *state
        .inspector_card_open
        .entry(card_id.clone())
        .or_insert(true);

    let cursor_y = ui.cursor_pos()[1];
    let header_rect = [card_x, cursor_y, card_w, CARD_HEADER_H];

    ui.paint_rect_filled(
        [card_x, cursor_y],
        [card_w, CARD_HEADER_H],
        theme.surface_elevated,
        theme.radius_md,
    );
    ui.paint_rect_stroke(
        [card_x, cursor_y],
        [card_w, CARD_HEADER_H],
        with_alpha(theme.separator, 0.55),
        theme.radius_md,
        1.0,
    );

    let chev = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    paint_icon(
        ui,
        [card_x + 8.0, cursor_y + 9.0],
        chev,
        12.0,
        theme.text_muted,
    );
    paint_icon(
        ui,
        [card_x + 26.0, cursor_y + 8.0],
        icon,
        14.0,
        theme.primary_dim,
    );
    paint_text_size(
        ui,
        [card_x + 46.0, cursor_y + 9.0],
        title,
        12.0,
        theme.text,
    );

    // Trailing edge: optional toggle, then optional trash button.
    let mut right_edge = card_x + card_w - 10.0;

    if let Some(en) = enabled {
        let toggle_w = 26.0;
        let toggle_h = 14.0;
        let tx = right_edge - toggle_w;
        let ty = cursor_y + (CARD_HEADER_H - toggle_h) * 0.5;
        let track_color = if en {
            theme.primary
        } else {
            theme.surface_active
        };
        ui.paint_rect_filled([tx, ty], [toggle_w, toggle_h], track_color, 999.0);
        let knob_x = if en { tx + 13.0 } else { tx + 1.0 };
        ui.paint_circle_filled([knob_x + 6.0, ty + toggle_h * 0.5], 5.5, theme.text);
        right_edge = tx - 6.0;
    }

    if removable {
        let trash_w = 22.0;
        let trash_h = 22.0;
        let tx = right_edge - trash_w;
        let ty = cursor_y + (CARD_HEADER_H - trash_h) * 0.5;
        let trash_int =
            ui.interact_rect(&format!("card-rm-{}", card_id), [tx, ty, trash_w, trash_h]);
        let trash_color = if trash_int.hovered {
            theme.error
        } else {
            theme.text_muted
        };
        if trash_int.hovered {
            ui.paint_rect_filled(
                [tx, ty],
                [trash_w, trash_h],
                with_alpha(theme.error, 0.12),
                4.0,
            );
        }
        paint_icon(ui, [tx + 5.0, ty + 5.0], Icon::Trash, 12.0, trash_color);
        if trash_int.clicked {
            state.pending_edits.push(PropertyEdit::RemoveComponent {
                entity,
                type_name: title.to_string(),
            });
        }
    }

    let header_int = ui.interact_rect(&format!("card-hdr-{}", card_id), header_rect);
    if header_int.clicked {
        state.inspector_card_open.insert(card_id, !open);
    }

    ui.spacing(CARD_HEADER_H + 4.0);

    if open {
        ui.indent("card-body", &mut |ui_b| {
            body(ui_b);
        });
        ui.spacing(10.0);
    } else {
        ui.spacing(4.0);
    }
}
