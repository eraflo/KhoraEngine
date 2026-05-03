// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Khora Hub palette and theme application.
//!
//! Mirrors the editor's "Deep Navy / Silver" brand language (`khora_dark()` in
//! `crates/khora-editor/src/theme.rs`) but stays self-contained — the hub has
//! no dependency on engine crates by design, so the values are duplicated
//! intentionally. Keep them in sync when the brand evolves.

use eframe::egui;

/// Linear-RGB dark palette, matching Khora brand.
pub mod pal {
    use eframe::egui::Color32;

    // ── Surfaces (deep navy, oklch 0.16-0.32 hue 265) ──
    pub const BG: Color32 = Color32::from_rgb(7, 8, 11); // oklch(0.16 0.022 265)
    pub const SURFACE: Color32 = Color32::from_rgb(11, 13, 17); // oklch(0.20 0.025 265)
    pub const SURFACE2: Color32 = Color32::from_rgb(16, 17, 23); // oklch(0.235 0.028 265)
    pub const SURFACE3: Color32 = Color32::from_rgb(22, 23, 30); // oklch(0.27 0.032 265)
    pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(28, 30, 38); // oklch(0.32 0.036 265)

    // ── Borders / separators ──
    pub const BORDER: Color32 = Color32::from_rgba_premultiplied(38, 41, 56, 178);
    pub const BORDER_LIGHT: Color32 = Color32::from_rgb(52, 56, 78);
    pub const SEPARATOR: Color32 = Color32::from_rgba_premultiplied(28, 30, 42, 140);

    // ── Brand silver (replaces the old blue PRIMARY) ──
    pub const PRIMARY: Color32 = Color32::from_rgb(165, 173, 185); // oklch(0.84 0.04 240)
    pub const PRIMARY_DIM: Color32 = Color32::from_rgb(98, 105, 117); // oklch(0.68 0.045 240)

    // ── Accents ──
    pub const ACCENT_VIOLET: Color32 = Color32::from_rgb(140, 96, 210); // oklch(0.72 0.14 290)
    pub const ACCENT_CYAN: Color32 = Color32::from_rgb(100, 175, 218); // oklch(0.78 0.10 220)
    pub const ACCENT_GOLD: Color32 = Color32::from_rgb(218, 168, 56); // oklch(0.80 0.12 82)
    /// Legacy alias kept so the rest of the hub keeps compiling — points at
    /// the new violet accent.
    pub const ACCENT: Color32 = ACCENT_VIOLET;

    // ── Status ──
    pub const SUCCESS: Color32 = Color32::from_rgb(95, 192, 121); // oklch(0.78 0.13 150)
    pub const WARNING: Color32 = Color32::from_rgb(218, 152, 56); // oklch(0.78 0.14 70)
    pub const ERROR: Color32 = Color32::from_rgb(196, 56, 38); // oklch(0.68 0.18 25)

    // ── Text ──
    pub const TEXT: Color32 = Color32::from_rgb(238, 240, 244); // oklch(0.96 0.005 250)
    pub const TEXT_DIM: Color32 = Color32::from_rgb(190, 195, 202); // oklch(0.82 0.01 250)
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 146, 156); // oklch(0.65 0.012 250)
    pub const TEXT_DISABLED: Color32 = Color32::from_rgb(96, 102, 113); // oklch(0.50 0.015 250)
}

/// Apply the Khora dark theme to the egui context. Idempotent.
pub fn apply_hub_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // ── Surfaces ─────────────────────────────────────
    visuals.panel_fill = pal::SURFACE;
    visuals.window_fill = pal::SURFACE;
    visuals.faint_bg_color = pal::SURFACE2;
    visuals.extreme_bg_color = pal::BG;
    visuals.code_bg_color = pal::SURFACE2;

    // ── Text ─────────────────────────────────────────
    visuals.override_text_color = Some(pal::TEXT);
    visuals.warn_fg_color = pal::WARNING;
    visuals.error_fg_color = pal::ERROR;

    // ── Selection / hyperlinks ───────────────────────
    visuals.selection.bg_fill = pal::PRIMARY.gamma_multiply(0.20);
    visuals.selection.stroke = egui::Stroke::new(1.0, pal::PRIMARY);
    visuals.hyperlink_color = pal::ACCENT_CYAN;

    // ── Window chrome ────────────────────────────────
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::Vec2::new(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(140),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, pal::BORDER);
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: egui::Vec2::new(0.0, 4.0),
        blur: 12.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(110),
    };

    // ── Widget radii (matches editor `radius_md = 6.0`) ──
    let cr = egui::Rounding::same(5_f32);

    visuals.widgets.noninteractive.bg_fill = pal::SURFACE;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, pal::TEXT_DIM);
    visuals.widgets.noninteractive.rounding = cr;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, pal::SEPARATOR);

    visuals.widgets.inactive.bg_fill = pal::SURFACE3;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, pal::TEXT);
    visuals.widgets.inactive.rounding = cr;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, pal::BORDER);

    visuals.widgets.hovered.bg_fill = pal::SURFACE_ACTIVE;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, pal::TEXT);
    visuals.widgets.hovered.rounding = cr;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, pal::BORDER_LIGHT);

    visuals.widgets.active.bg_fill = pal::PRIMARY.gamma_multiply(0.32);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, pal::TEXT);
    visuals.widgets.active.rounding = cr;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, pal::PRIMARY);

    visuals.widgets.open.bg_fill = pal::SURFACE2;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, pal::TEXT);
    visuals.widgets.open.rounding = cr;

    visuals.menu_rounding = egui::Rounding::same(4_f32);
    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    ctx.set_visuals(visuals);

    // ── Spacing & sizing (tracks editor `pad_row = 8`, `pad_card = 14`) ──
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.indent = 14.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.window_margin = egui::Margin::same(8.0);
    style.spacing.menu_margin = egui::Margin::symmetric(8.0, 6.0);

    // Force every text style to an integer size — fractional sizes (e.g.
    // 12.5 px) sample glyphs at non-pixel-aligned positions on most DPI
    // scales and look soft / "smudged" on Geist.
    for (_, font) in style.text_styles.iter_mut() {
        font.size = font.size.round().max(12.0);
    }
    ctx.set_style(style);
}
