// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Khora Hub palette and theme application.

use eframe::egui;

/// Linear-inspired dark palette, matching Khora brand.
pub mod pal {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(10, 10, 14);
    pub const SURFACE: Color32 = Color32::from_rgb(17, 18, 23);
    pub const SURFACE2: Color32 = Color32::from_rgb(24, 26, 33);
    pub const SURFACE3: Color32 = Color32::from_rgb(34, 37, 48);
    pub const BORDER: Color32 = Color32::from_rgb(38, 42, 56);
    pub const BORDER_LIGHT: Color32 = Color32::from_rgb(55, 60, 78);
    pub const PRIMARY: Color32 = Color32::from_rgb(58, 135, 240);
    pub const PRIMARY_DIM: Color32 = Color32::from_rgb(25, 65, 130);
    pub const ACCENT: Color32 = Color32::from_rgb(124, 92, 222);
    pub const TEXT: Color32 = Color32::from_rgb(226, 232, 240);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(136, 146, 164);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(80, 88, 106);
    pub const SUCCESS: Color32 = Color32::from_rgb(58, 184, 122);
    pub const WARNING: Color32 = Color32::from_rgb(240, 160, 58);
    pub const ERROR: Color32 = Color32::from_rgb(240, 90, 58);
}

/// Apply the Khora dark theme to the egui context.
pub fn apply_hub_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = pal::SURFACE;
    visuals.window_fill = pal::SURFACE;
    visuals.faint_bg_color = pal::SURFACE2;
    visuals.extreme_bg_color = pal::BG;
    visuals.override_text_color = Some(pal::TEXT);
    visuals.selection.bg_fill = pal::PRIMARY.gamma_multiply(0.25);
    visuals.selection.stroke = egui::Stroke::new(1.0, pal::PRIMARY);
    visuals.hyperlink_color = pal::ACCENT;
    visuals.warn_fg_color = pal::WARNING;
    visuals.error_fg_color = pal::ERROR;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::Vec2::new(0.0, 4.0),
        blur: 16.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(100),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, pal::BORDER);
    let cr = egui::Rounding::same(4_f32);
    visuals.widgets.inactive.bg_fill = pal::SURFACE3;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, pal::TEXT);
    visuals.widgets.inactive.rounding = cr;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, pal::BORDER);
    visuals.widgets.hovered.bg_fill = pal::PRIMARY.gamma_multiply(0.15);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, pal::TEXT);
    visuals.widgets.hovered.rounding = cr;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, pal::PRIMARY.gamma_multiply(0.4));
    visuals.widgets.active.bg_fill = pal::PRIMARY.gamma_multiply(0.35);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    visuals.widgets.active.rounding = cr;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, pal::PRIMARY);
    visuals.widgets.noninteractive.bg_fill = pal::SURFACE;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, pal::TEXT_DIM);
    visuals.widgets.noninteractive.rounding = cr;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, pal::BORDER);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_style(style);
}
