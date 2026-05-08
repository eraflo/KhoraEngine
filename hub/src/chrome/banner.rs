// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Banner overlay + the manually-placed status chip used by the topbar.

use crate::Banner;
use crate::theme::{pal, tint};
use crate::widgets::rgba;
use khora_sdk::tool_ui::{FontFamilyHint, LinearRgba, TextAlign, UiBuilder};

/// Paint the global banner overlay near the top of the central area.
pub fn paint_banner(ui: &mut dyn UiBuilder, banner: &Banner) {
    let r = ui.panel_rect();
    let bg = if banner.is_error { pal::ERROR } else { pal::PRIMARY };
    let h = 32.0;
    let w = (r[2] - 80.0).min(680.0);
    let pos = [r[0] + (r[2] - w) * 0.5, r[1] + 56.0];
    ui.paint_rect_filled(pos, [w, h], rgba(tint(bg, 0.85)), 6.0);
    ui.paint_text_styled(
        [pos[0] + 16.0, pos[1] + 8.0],
        &banner.message,
        12.0,
        rgba(LinearRgba::WHITE),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}

/// Paints a status chip at an explicit position. Used by the topbar
/// where layout is hand-placed instead of via the auto-cursor.
pub fn paint_chip_at(ui: &mut dyn UiBuilder, pos: [f32; 2], text: &str, color: LinearRgba) {
    let pad_x = 8.0;
    let pad_y = 4.0;
    let size = ui.measure_text(text, 11.0, FontFamilyHint::Proportional);
    let dot_w = 14.0;
    let w = dot_w + size[0] + pad_x * 2.0;
    let h = size[1] + pad_y * 2.0;
    ui.paint_rect_filled(pos, [w, h], rgba(tint(color, 0.15)), 4.0);
    ui.paint_rect_stroke(pos, [w, h], rgba(tint(color, 0.4)), 4.0, 1.0);
    let cy = pos[1] + h * 0.5;
    ui.paint_circle_filled([pos[0] + pad_x + 4.0, cy], 3.5, rgba(color));
    ui.paint_circle_filled([pos[0] + pad_x + 4.0, cy], 5.5, rgba(tint(color, 0.18)));
    ui.paint_text_styled(
        [pos[0] + pad_x + dot_w, pos[1] + pad_y],
        text,
        11.0,
        rgba(color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}
