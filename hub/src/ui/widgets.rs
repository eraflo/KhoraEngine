// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Reusable UI widgets for the Khora Hub.
//!
//! Visual language is intentionally aligned with the editor (`khora-editor`):
//! diamond brand mark, brand pill in the title bar, slim tabs with primary-
//! tinted active state, vertical gradients on headers, KBD chips for
//! shortcuts, and a 24 px status bar at the bottom.

use crate::theme::pal;
use eframe::egui;

// ── Color helpers ───────────────────────────────────────────────────

/// Returns `c` with its alpha multiplied. egui has its own `gamma_multiply` —
/// this one is for clarity at the call site when we want a translucent tint.
#[inline]
pub fn tint(c: egui::Color32, alpha: f32) -> egui::Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// ── Brand mark (diamond, mirrors `crates/khora-editor/src/widgets/brand.rs`) ──

/// Paints a filled 4-point diamond (rotated square) — the Khora brand mark.
pub fn paint_diamond_filled(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    use egui::Pos2;
    use egui::epaint::{PathShape, PathStroke};
    let pts = vec![
        Pos2::new(center.x, center.y - size),
        Pos2::new(center.x + size, center.y),
        Pos2::new(center.x, center.y + size),
        Pos2::new(center.x - size, center.y),
    ];
    painter.add(egui::Shape::Path(PathShape {
        points: pts,
        closed: true,
        fill: color,
        stroke: PathStroke::NONE,
    }));
}

/// Paints the outline of the brand diamond (used as a glyph in disabled
/// states and empty-state illustrations).
pub fn paint_diamond_outline(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
    thickness: f32,
) {
    let stroke = egui::Stroke::new(thickness, color);
    let top = egui::pos2(center.x, center.y - size);
    let right = egui::pos2(center.x + size, center.y);
    let bottom = egui::pos2(center.x, center.y + size);
    let left = egui::pos2(center.x - size, center.y);
    painter.line_segment([top, right], stroke);
    painter.line_segment([right, bottom], stroke);
    painter.line_segment([bottom, left], stroke);
    painter.line_segment([left, top], stroke);
}

/// Legacy alias: kept as a thin wrapper around `paint_diamond_filled` so any
/// external call site that referenced the old 8-point compass star still
/// compiles. New code should call `paint_diamond_filled` directly.
#[inline]
pub fn paint_khora_star(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    paint_diamond_filled(painter, center, size, color);
}

// ── Background fills ────────────────────────────────────────────────

/// Approximates a vertical gradient by stacking horizontal strips. egui has
/// no native gradient support — same trick the editor uses for panel headers.
pub fn paint_vertical_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    top: egui::Color32,
    bottom: egui::Color32,
    steps: u32,
) {
    let steps = steps.max(2);
    let strip_h = rect.height() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let r = lerp_u8(top.r(), bottom.r(), t);
        let g = lerp_u8(top.g(), bottom.g(), t);
        let b = lerp_u8(top.b(), bottom.b(), t);
        let a = lerp_u8(top.a(), bottom.a(), t);
        let color = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        let y = rect.top() + strip_h * i as f32;
        let strip = egui::Rect::from_min_size(
            egui::pos2(rect.left(), y),
            egui::vec2(rect.width(), strip_h.ceil() + 0.6),
        );
        painter.rect_filled(strip, 0.0, color);
    }
}

#[inline]
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 * (1.0 - t) + b as f32 * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Draws a thin horizontal separator line.
pub fn paint_separator(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// Draws a vertical hairline at `x`, between `y` ±`half_height` from the
/// strip's vertical center.
pub fn paint_v_hairline(
    painter: &egui::Painter,
    x: f32,
    top: f32,
    bottom: f32,
    color: egui::Color32,
) {
    painter.line_segment(
        [egui::pos2(x, top), egui::pos2(x, bottom)],
        egui::Stroke::new(1.0, color),
    );
}

// ── Chips & dots ────────────────────────────────────────────────────

/// A small colored badge (e.g. version tag).
pub fn badge(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).monospace().color(fg));
        });
}

/// A status chip with icon-like coloring.
pub fn status_chip(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(tint(color, 0.15))
        .stroke(egui::Stroke::new(1.0, tint(color, 0.4)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            // ● dot + label, mirroring the editor status bar.
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, color);
                ui.painter()
                    .circle_filled(rect.center(), 5.5, tint(color, 0.18));
                ui.label(egui::RichText::new(text).size(11.0).color(color));
            });
        });
}

/// A keyboard shortcut chip, e.g. ⌘ K. Mirrors the editor's `paint_kbd_chip`
/// (small monospace + double-line bottom for "depth").
pub fn kbd_chip(ui: &mut egui::Ui, label: &str) {
    egui::Frame::new()
        .fill(pal::SURFACE_ACTIVE)
        .stroke(egui::Stroke::new(1.0, pal::BORDER))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin {
            left: 5,
            right: 5,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(11.0)
                    .color(pal::TEXT_DIM),
            );
        });
}

/// Section header label styled like the editor's panel headers.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(12.0)
            .color(pal::TEXT_DIM),
    );
}

/// A form field label.
pub fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(pal::TEXT_DIM));
}

// ── Tabs & nav ──────────────────────────────────────────────────────

/// Sidebar nav button — full-width row, primary-tinted bg when active.
pub fn sidebar_nav_btn(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let fill = if active {
        tint(pal::PRIMARY, 0.18)
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active { pal::PRIMARY } else { pal::TEXT_DIM };
    let stroke = if active {
        egui::Stroke::new(1.0, tint(pal::PRIMARY, 0.35))
    } else {
        egui::Stroke::NONE
    };
    ui.add_sized(
        [180.0, 30.0],
        egui::Button::new(egui::RichText::new(label).size(12.0).color(text_color))
            .fill(fill)
            .stroke(stroke),
    )
}

/// Top-bar pill tab. Slim, with a primary-tinted background only when active.
pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let fill = if active {
        tint(pal::PRIMARY, 0.16)
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active { pal::TEXT } else { pal::TEXT_DIM };
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.0).color(text_color))
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .min_size(egui::vec2(0.0, 24.0)),
    )
}

// ── Buttons ─────────────────────────────────────────────────────────

/// Primary call-to-action button (filled primary silver, light text).
pub fn primary_button(ui: &mut egui::Ui, label: &str, size: [f32; 2]) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(
            egui::RichText::new(label)
                .size(12.0)
                .strong()
                .color(pal::BG),
        )
        .fill(pal::PRIMARY)
        .stroke(egui::Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(5)),
    )
}

/// Destructive confirmation button (filled with `pal::ERROR`).
pub fn danger_button(ui: &mut egui::Ui, label: &str, size: [f32; 2]) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(
            egui::RichText::new(label)
                .size(12.0)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(pal::ERROR)
        .stroke(egui::Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(5)),
    )
}

/// Subdued button used for secondary actions / cancels.
pub fn ghost_button(ui: &mut egui::Ui, label: &str, size: [f32; 2]) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(egui::RichText::new(label).size(12.0).color(pal::TEXT_DIM))
            .fill(pal::SURFACE3)
            .stroke(egui::Stroke::new(1.0, pal::BORDER))
            .corner_radius(egui::CornerRadius::same(5)),
    )
}

// ── Time formatting (kept here for proximity with badge use) ────────

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
