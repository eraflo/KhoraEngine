// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Inspector header — composite widget used by the Properties panel.

use khora_sdk::editor_ui::{UiTheme, FontFamilyHint, Icon, TextAlign, UiBuilder};

use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

/// Paints the Inspector's rich header — large icon tile + name + meta row
/// (tag + status pill + id). Returns the bottom edge in screen-space.
#[allow(clippy::too_many_arguments)]
pub fn paint_inspector_header(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    icon: Icon,
    name: &str,
    type_tag: &str,
    status_label: &str,
    status_color: [f32; 4],
    id_label: Option<&str>,
    theme: &UiTheme,
) -> f32 {
    let h = 64.0;
    let pad = 12.0;

    // Background
    ui.paint_rect_filled(origin, [width, h], theme.surface, 0.0);
    ui.paint_line(
        [origin[0], origin[1] + h],
        [origin[0] + width, origin[1] + h],
        with_alpha(theme.separator, 0.55),
        1.0,
    );

    // Icon tile
    let tile_x = origin[0] + pad;
    let tile_y = origin[1] + 14.0;
    ui.paint_rect_filled([tile_x, tile_y], [36.0, 36.0], theme.surface_active, 8.0);
    ui.paint_rect_stroke([tile_x, tile_y], [36.0, 36.0], theme.border, 8.0, 1.0);
    paint_icon(
        ui,
        [tile_x + 10.0, tile_y + 10.0],
        icon,
        16.0,
        theme.primary,
    );

    // Name
    let name_x = tile_x + 48.0;
    paint_text_size(ui, [name_x, origin[1] + 14.0], name, 14.5, theme.text);

    // Meta row
    let meta_y = origin[1] + 36.0;
    // Type tag
    let tag_w = ui.measure_text(type_tag, 10.0, FontFamilyHint::Monospace)[0] + 14.0;
    ui.paint_rect_filled([name_x, meta_y], [tag_w, 16.0], theme.surface_active, 3.0);
    ui.paint_text_styled(
        [name_x + tag_w * 0.5, meta_y + 2.5],
        type_tag,
        10.0,
        theme.text_dim,
        FontFamilyHint::Monospace,
        TextAlign::Center,
    );

    // Status pill
    let pill_x = name_x + tag_w + 8.0;
    let label_w = ui.measure_text(status_label, 10.5, FontFamilyHint::Proportional)[0] + 22.0;
    ui.paint_rect_filled(
        [pill_x, meta_y],
        [label_w, 16.0],
        with_alpha(status_color, 0.18),
        999.0,
    );
    ui.paint_circle_filled([pill_x + 8.0, meta_y + 8.0], 2.5, status_color);
    paint_text_size(
        ui,
        [pill_x + 14.0, meta_y + 3.0],
        status_label,
        10.5,
        status_color,
    );

    // Id
    if let Some(id) = id_label {
        ui.paint_text_styled(
            [pill_x + label_w + 10.0, meta_y + 3.5],
            id,
            10.0,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
    }

    origin[1] + h
}
