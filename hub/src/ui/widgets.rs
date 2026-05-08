// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Reusable UI widgets for the Khora Hub — backend-neutral.
//!
//! Every widget takes `&mut dyn UiBuilder` and reports interaction
//! via [`Interaction`]. Painting goes through the engine's
//! [`UiBuilder`] paint primitives — no direct egui calls.

use crate::theme::pal;
use khora_sdk::tool_ui::{
    FontFamilyHint, Interaction, LinearRgba, TextAlign, UiBuilder,
};

use super::theme::tint;

/// Convert a [`LinearRgba`] to the `[f32; 4]` form taken by the
/// engine's paint primitives.
#[inline]
pub fn rgba(c: LinearRgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

// ── Brand mark (diamond) ────────────────────────────────────────────

/// Paints a filled 4-point diamond — the Khora brand mark.
pub fn paint_diamond_filled(
    ui: &mut dyn UiBuilder,
    center: [f32; 2],
    size: f32,
    color: LinearRgba,
) {
    let pts = [
        [center[0], center[1] - size],
        [center[0] + size, center[1]],
        [center[0], center[1] + size],
        [center[0] - size, center[1]],
    ];
    ui.paint_path_filled(&pts, rgba(color));
}

/// Paints the outline of the brand diamond.
pub fn paint_diamond_outline(
    ui: &mut dyn UiBuilder,
    center: [f32; 2],
    size: f32,
    color: LinearRgba,
    thickness: f32,
) {
    let top = [center[0], center[1] - size];
    let right = [center[0] + size, center[1]];
    let bottom = [center[0], center[1] + size];
    let left = [center[0] - size, center[1]];
    let c = rgba(color);
    ui.paint_line(top, right, c, thickness);
    ui.paint_line(right, bottom, c, thickness);
    ui.paint_line(bottom, left, c, thickness);
    ui.paint_line(left, top, c, thickness);
}

/// Legacy alias.
#[inline]
pub fn paint_khora_star(
    ui: &mut dyn UiBuilder,
    center: [f32; 2],
    size: f32,
    color: LinearRgba,
) {
    paint_diamond_filled(ui, center, size, color);
}

// ── Background fills ────────────────────────────────────────────────

/// Approximates a vertical gradient by stacking horizontal strips.
pub fn paint_vertical_gradient(
    ui: &mut dyn UiBuilder,
    rect: [f32; 4],
    top: LinearRgba,
    bottom: LinearRgba,
    steps: u32,
) {
    let steps = steps.max(2);
    let strip_h = rect[3] / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let color = LinearRgba::lerp(top, bottom, t);
        let y = rect[1] + strip_h * i as f32;
        ui.paint_rect_filled([rect[0], y], [rect[2], strip_h.ceil() + 0.6], rgba(color), 0.0);
    }
}

/// Draws a thin horizontal separator line across the available width.
pub fn paint_separator(ui: &mut dyn UiBuilder, color: LinearRgba) {
    let r = ui.panel_rect();
    let y = ui.cursor_pos()[1];
    ui.paint_rect_filled([r[0], y], [r[2], 1.0], rgba(color), 0.0);
    ui.spacing(2.0);
}

/// Draws a vertical hairline at `x` between `top` and `bottom`.
pub fn paint_v_hairline(
    ui: &mut dyn UiBuilder,
    x: f32,
    top: f32,
    bottom: f32,
    color: LinearRgba,
) {
    ui.paint_line([x, top], [x, bottom], rgba(color), 1.0);
}

// ── Chips & dots ────────────────────────────────────────────────────

/// A small coloured badge (e.g. version tag) at the cursor.
pub fn badge(ui: &mut dyn UiBuilder, text: &str, bg: LinearRgba, fg: LinearRgba) {
    let pad_x = 6.0;
    let pad_y = 2.0;
    let size = ui.measure_text(text, 11.0, FontFamilyHint::Monospace);
    let w = size[0] + pad_x * 2.0;
    let h = size[1] + pad_y * 2.0;
    let r = ui.allocate_size([w, h]);
    ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], rgba(bg), 4.0);
    ui.paint_text_styled(
        [r[0] + pad_x, r[1] + pad_y],
        text,
        11.0,
        rgba(fg),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
}

/// A status chip (dot + label inside a tinted pill).
pub fn status_chip(ui: &mut dyn UiBuilder, text: &str, color: LinearRgba) {
    let pad_x = 8.0;
    let pad_y = 4.0;
    let size = ui.measure_text(text, 11.0, FontFamilyHint::Proportional);
    let dot_w = 14.0;
    let w = dot_w + size[0] + pad_x * 2.0;
    let h = size[1] + pad_y * 2.0;
    let r = ui.allocate_size([w, h]);
    let pos = [r[0], r[1]];
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

/// A keyboard shortcut chip.
pub fn kbd_chip(ui: &mut dyn UiBuilder, label: &str) {
    let pad_x = 5.0;
    let pad_y = 1.0;
    let size = ui.measure_text(label, 11.0, FontFamilyHint::Monospace);
    let w = size[0] + pad_x * 2.0;
    let h = size[1] + pad_y * 2.0;
    let r = ui.allocate_size([w, h]);
    ui.paint_rect_filled([r[0], r[1]], [w, h], rgba(pal::SURFACE_ACTIVE), 3.0);
    ui.paint_rect_stroke([r[0], r[1]], [w, h], rgba(pal::BORDER), 3.0, 1.0);
    ui.paint_text_styled(
        [r[0] + pad_x, r[1] + pad_y],
        label,
        11.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
}

/// Section header label — strong silver text at body size.
pub fn section_header(ui: &mut dyn UiBuilder, text: &str) {
    ui.colored_label(rgba(pal::TEXT_DIM), text);
}

/// A form field label — secondary text colour.
pub fn field_label(ui: &mut dyn UiBuilder, text: &str) {
    ui.colored_label(rgba(pal::TEXT_DIM), text);
}

// ── Tabs & nav ──────────────────────────────────────────────────────

/// Sidebar nav button — primary-tinted bg when active.
pub fn sidebar_nav_btn(ui: &mut dyn UiBuilder, salt: &str, label: &str, active: bool) -> Interaction {
    let w = 180.0;
    let h = 30.0;
    let r = ui.allocate_size([w, h]);
    let pos = [r[0], r[1]];
    let int = ui.interact_rect(salt, [pos[0], pos[1], w, h]);

    let (fill_color, fill_alpha) = if active {
        (pal::PRIMARY, 0.18)
    } else if int.hovered {
        (pal::PRIMARY, 0.08)
    } else {
        (LinearRgba::TRANSPARENT, 0.0)
    };
    if fill_alpha > 0.0 {
        ui.paint_rect_filled(pos, [w, h], rgba(tint(fill_color, fill_alpha)), 5.0);
    }
    if active {
        ui.paint_rect_stroke(pos, [w, h], rgba(tint(pal::PRIMARY, 0.35)), 5.0, 1.0);
    }
    let text_color = if active { pal::PRIMARY } else { pal::TEXT_DIM };
    ui.paint_text_styled(
        [pos[0] + 10.0, pos[1] + 9.0],
        label,
        12.0,
        rgba(text_color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    int
}

/// Top-bar pill tab. Slim, primary-tinted background when active.
pub fn tab_pill(ui: &mut dyn UiBuilder, salt: &str, label: &str, active: bool) -> Interaction {
    let size = ui.measure_text(label, 12.0, FontFamilyHint::Proportional);
    let w = size[0] + 24.0;
    let h = 24.0;
    let r = ui.allocate_size([w, h]);
    let pos = [r[0], r[1]];
    let int = ui.interact_rect(salt, [pos[0], pos[1], w, h]);
    let fill = if active {
        Some(tint(pal::PRIMARY, 0.16))
    } else if int.hovered {
        Some(tint(pal::PRIMARY, 0.08))
    } else {
        None
    };
    if let Some(c) = fill {
        ui.paint_rect_filled(pos, [w, h], rgba(c), 5.0);
    }
    let text_color = if active { pal::TEXT } else { pal::TEXT_DIM };
    ui.paint_text_styled(
        [pos[0] + 12.0, pos[1] + 6.0],
        label,
        12.0,
        rgba(text_color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    int
}

// ── Buttons ─────────────────────────────────────────────────────────

/// Primary call-to-action button (filled silver, dark text).
pub fn primary_button(
    ui: &mut dyn UiBuilder,
    salt: &str,
    label: &str,
    size: [f32; 2],
) -> Interaction {
    paint_button(ui, salt, label, size, pal::PRIMARY, pal::BG, None, true)
}

/// Destructive confirmation button (filled with `pal::ERROR`).
pub fn danger_button(
    ui: &mut dyn UiBuilder,
    salt: &str,
    label: &str,
    size: [f32; 2],
) -> Interaction {
    paint_button(
        ui,
        salt,
        label,
        size,
        pal::ERROR,
        LinearRgba::WHITE,
        None,
        true,
    )
}

/// Subdued button for secondary actions / cancels.
pub fn ghost_button(
    ui: &mut dyn UiBuilder,
    salt: &str,
    label: &str,
    size: [f32; 2],
) -> Interaction {
    paint_button(
        ui,
        salt,
        label,
        size,
        pal::SURFACE3,
        pal::TEXT_DIM,
        Some(pal::BORDER),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn paint_button(
    ui: &mut dyn UiBuilder,
    salt: &str,
    label: &str,
    size: [f32; 2],
    fill: LinearRgba,
    text_color: LinearRgba,
    border: Option<LinearRgba>,
    bold: bool,
) -> Interaction {
    // Reserve `size` in the parent layout so the row's height
    // reflects the button (matters inside `horizontal()`). Without
    // this, the surrounding row would collapse and subsequent
    // widgets would paint on top.
    let r = ui.allocate_size(size);
    let pos = [r[0], r[1]];
    let int = ui.interact_rect(salt, [pos[0], pos[1], size[0], size[1]]);

    // Hover/press effect: slightly brighten on hover, dim on press.
    let mut effective_fill = fill;
    if int.pressed {
        effective_fill = tint(fill, 0.85);
    } else if int.hovered {
        effective_fill = LinearRgba::new(
            (fill.r * 1.08).min(1.0),
            (fill.g * 1.08).min(1.0),
            (fill.b * 1.08).min(1.0),
            fill.a,
        );
    }

    ui.paint_rect_filled(pos, size, rgba(effective_fill), 5.0);
    if let Some(b) = border {
        ui.paint_rect_stroke(pos, size, rgba(b), 5.0, 1.0);
    }

    let text_size = ui.measure_text(label, 12.0, FontFamilyHint::Proportional);
    let tx = pos[0] + (size[0] - text_size[0]) * 0.5;
    let ty = pos[1] + (size[1] - text_size[1]) * 0.5;
    let _ = bold;
    ui.paint_text_styled(
        [tx, ty],
        label,
        12.0,
        rgba(text_color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    int
}

// ── Time formatting ─────────────────────────────────────────────────

/// Format a unix timestamp into a human-readable relative string.
pub fn format_ts(ts: u64) -> String {
    if ts == 0 {
        return "Never".to_owned();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        "Just now".to_owned()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} h ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}
