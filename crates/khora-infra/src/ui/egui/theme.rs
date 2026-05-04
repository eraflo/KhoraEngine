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

    // ── Surfaces ─────────────────────────────────────
    visuals.panel_fill = c(theme.surface);
    visuals.window_fill = c(theme.surface);
    visuals.faint_bg_color = c(theme.surface_elevated);
    visuals.extreme_bg_color = c(theme.background);
    visuals.code_bg_color = c(theme.surface_elevated);

    // ── Text ─────────────────────────────────────────
    visuals.override_text_color = Some(c(theme.text));
    visuals.warn_fg_color = c(theme.warning);
    visuals.error_fg_color = c(theme.error);

    // ── Selection / hyperlinks ───────────────────────
    visuals.selection.bg_fill = c(theme.primary).gamma_multiply(0.20);
    visuals.selection.stroke = egui::Stroke::new(1.0, c(theme.primary));
    visuals.hyperlink_color = c(theme.accent_b);

    // ── Window chrome ────────────────────────────────
    visuals.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: egui::Color32::from_black_alpha(140),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, c(theme.border));
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(110),
    };

    // ── Widget radii ─────────────────────────────────
    let radius_md = egui::CornerRadius::same(theme.radius_md.round().clamp(0.0, 30.0) as u8);
    let radius_sm = egui::CornerRadius::same(theme.radius_sm.round().clamp(0.0, 30.0) as u8);

    // ── Widget states ────────────────────────────────
    visuals.widgets.noninteractive.bg_fill = c(theme.surface);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, c(theme.text_dim));
    visuals.widgets.noninteractive.corner_radius = radius_md;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, c(theme.separator));

    visuals.widgets.inactive.bg_fill = c(theme.surface_interactive);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.inactive.corner_radius = radius_md;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, c(theme.border));

    visuals.widgets.hovered.bg_fill = c(theme.surface_active);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.hovered.corner_radius = radius_md;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, c(theme.border_strong));

    visuals.widgets.active.bg_fill = c(theme.primary).gamma_multiply(0.32);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, c(theme.text));
    visuals.widgets.active.corner_radius = radius_md;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, c(theme.primary));

    visuals.widgets.open.bg_fill = c(theme.surface_elevated);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, c(theme.text));
    visuals.widgets.open.corner_radius = radius_md;

    // Menu / popup backgrounds use the elevated surface so they pop above
    // panels without looking out of place.
    visuals.menu_corner_radius = radius_sm;

    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    ctx.set_visuals(visuals);

    // ── Spacing & sizing ─────────────────────────────
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(theme.pad_row * 0.75, theme.pad_row * 0.5);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.indent = 14.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.menu_margin = egui::Margin::symmetric(8, 6);
    // Make panel resize handles much easier to grab. The default 4px hot
    // zone is hard to hit and our panel content paints right up to the
    // edge, so users were reporting the resize "didn't work".
    style.interaction.resize_grab_radius_side = 8.0;
    style.interaction.resize_grab_radius_corner = 10.0;

    // Default font sizes per text style — drives any RichText that does not
    // override its size manually.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(theme.font_size_display, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(theme.font_size_body, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(theme.font_size_body, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(theme.font_size_caption, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(theme.font_size_body - 0.5, FontFamily::Monospace),
    );

    ctx.set_style(style);
}
