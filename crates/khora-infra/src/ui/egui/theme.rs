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

//! Convert [`EditorTheme`] to egui [`Visuals`] and apply to an egui context.

use khora_core::ui::editor::EditorTheme;

fn c(color: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
        (color[3] * 255.0) as u8,
    )
}

/// Applies an [`EditorTheme`] to the given egui context.
pub fn apply_theme(ctx: &egui::Context, theme: &EditorTheme) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = c(theme.surface);
    visuals.window_fill = c(theme.surface);
    visuals.faint_bg_color = c(theme.surface_highlight);
    visuals.extreme_bg_color = c(theme.background);
    visuals.override_text_color = Some(c(theme.text));
    visuals.selection.bg_fill = c(theme.primary).gamma_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, c(theme.primary));
    visuals.hyperlink_color = c(theme.accent);
    visuals.warn_fg_color = c(theme.warning);
    visuals.error_fg_color = c(theme.error);

    // Window chrome
    visuals.window_shadow = egui::Shadow {
        offset: [0, 4].into(),
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(80),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, c(theme.border));
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 2].into(),
        blur: 8,
        spread: 0,
        color: egui::Color32::from_black_alpha(60),
    };

    // Widget styles
    let corner_radius = egui::CornerRadius::same(3);

    visuals.widgets.noninteractive.bg_fill = c(theme.surface);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, c(theme.text_dim));
    visuals.widgets.noninteractive.corner_radius = corner_radius;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, c(theme.border));

    visuals.widgets.inactive.bg_fill = c(theme.surface_highlight);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.inactive.corner_radius = corner_radius;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;

    visuals.widgets.hovered.bg_fill = c(theme.primary).gamma_multiply(0.2);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.hovered.corner_radius = corner_radius;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, c(theme.primary).gamma_multiply(0.5));

    visuals.widgets.active.bg_fill = c(theme.primary).gamma_multiply(0.4);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = corner_radius;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, c(theme.primary));

    visuals.widgets.open.bg_fill = c(theme.surface_highlight);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.open.corner_radius = corner_radius;

    // Scroll bar
    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    ctx.set_visuals(visuals);

    // Spacing
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.spacing.indent = 16.0;
    ctx.set_style(style);
}
