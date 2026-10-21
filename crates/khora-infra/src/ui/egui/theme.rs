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

//! Convert [`UiTheme`] to egui [`Visuals`] and apply to an egui context.

use khora_core::ui::{ColorScheme, UiTheme};

/// Key under which [`apply_theme`] stashes the X/Y/Z axis colours in the egui
/// context.
///
/// `UiBuilder::vec3_editor` is a stock widget with no access to a [`UiTheme`],
/// but it must tint its axes with the theme's own tokens — otherwise the
/// inspector's X/Y/Z and the viewport gizmo's X/Y/Z drift apart, and the two
/// stop reading as the same thing. Rather than widen the trait or hard-code a
/// second copy of the palette in this backend, the theme leaves the colours
/// here for the widget to pick up.
pub(crate) const AXIS_COLORS_KEY: &str = "khora.axis_colors";

fn c(color: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
        (color[3] * 255.0) as u8,
    )
}

/// Applies an [`UiTheme`] to the given egui context.
pub fn apply_theme(ctx: &egui::Context, theme: &UiTheme) {
    // The theme's scheme picks egui's base visuals, and pins egui to it so
    // an OS switch between light and dark cannot swap in stock visuals.
    // Shadows are lighter on a light surface, where black at full weight
    // reads as a smudge.
    let (mut visuals, egui_theme, shadow_weight) = match theme.scheme {
        ColorScheme::Dark => (egui::Visuals::dark(), egui::Theme::Dark, 1.0),
        ColorScheme::Light => (egui::Visuals::light(), egui::Theme::Light, 0.35),
    };
    ctx.set_theme(egui_theme);
    let shadow = |alpha: f32| egui::Color32::from_black_alpha((alpha * shadow_weight) as u8);

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
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, c(theme.primary));
    visuals.hyperlink_color = c(theme.accent_b);

    // ── Window chrome ────────────────────────────────
    visuals.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow(140.0),
    };
    visuals.window_stroke = egui::Stroke::new(1.0_f32, c(theme.border));
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: shadow(110.0),
    };

    // ── Widget radii ─────────────────────────────────
    let radius_md = egui::CornerRadius::same(theme.radius_md.round().clamp(0.0, 30.0) as u8);
    let radius_sm = egui::CornerRadius::same(theme.radius_sm.round().clamp(0.0, 30.0) as u8);

    // ── Widget states ────────────────────────────────
    visuals.widgets.noninteractive.bg_fill = c(theme.surface);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, c(theme.text_dim));
    visuals.widgets.noninteractive.corner_radius = radius_md;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5_f32, c(theme.separator));

    visuals.widgets.inactive.bg_fill = c(theme.surface_interactive);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, c(theme.text));
    visuals.widgets.inactive.corner_radius = radius_md;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, c(theme.border));

    visuals.widgets.hovered.bg_fill = c(theme.surface_active);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, c(theme.text));
    visuals.widgets.hovered.corner_radius = radius_md;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, c(theme.border_strong));

    visuals.widgets.active.bg_fill = c(theme.primary).gamma_multiply(0.32);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, c(theme.text));
    visuals.widgets.active.corner_radius = radius_md;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, c(theme.primary));

    visuals.widgets.open.bg_fill = c(theme.surface_elevated);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, c(theme.text));
    visuals.widgets.open.corner_radius = radius_md;

    // Menu / popup backgrounds use the elevated surface so they pop above
    // panels without looking out of place.
    visuals.menu_corner_radius = radius_sm;

    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    ctx.set_visuals(visuals);

    // Hand the axis tints to `vec3_editor` (see AXIS_COLORS_KEY).
    let axes: [egui::Color32; 3] = [c(theme.axis_x), c(theme.axis_y), c(theme.axis_z)];
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(AXIS_COLORS_KEY), axes));

    // ── Spacing & sizing ─────────────────────────────
    let mut style = (*ctx.global_style()).clone();
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

    ctx.set_global_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ui::ColorScheme;

    /// A light theme: silver surfaces, navy ink, everything else neutral.
    fn light_theme() -> UiTheme {
        UiTheme {
            scheme: ColorScheme::Light,
            background: [0.875, 0.902, 0.933, 1.0],
            surface: [0.933, 0.945, 0.961, 1.0],
            surface_elevated: [0.973, 0.980, 0.988, 1.0],
            surface_interactive: [0.890, 0.914, 0.941, 1.0],
            surface_active: [0.827, 0.863, 0.906, 1.0],
            text: [0.078, 0.106, 0.157, 1.0],
            text_dim: [0.231, 0.278, 0.349, 1.0],
            text_inverse: [0.933, 0.945, 0.961, 1.0],
            primary: [0.114, 0.165, 0.251, 1.0],
            ..UiTheme::default()
        }
    }

    fn dark_theme() -> UiTheme {
        UiTheme {
            scheme: ColorScheme::Dark,
            ..UiTheme::default()
        }
    }

    /// `actual` is `color` to within one step per channel, whichever way the
    /// backend rounds a float to a byte.
    fn assert_color_is(actual: egui::Color32, color: [f32; 4], what: &str) {
        let got = actual.to_srgba_unmultiplied();
        for (i, (g, e)) in got.iter().zip(color).enumerate() {
            let e = e * 255.0;
            assert!(
                (*g as f32 - e).abs() <= 1.0,
                "{what}: channel {i} is {g}, expected {e:.1} (got {got:?})"
            );
        }
    }

    #[test]
    fn apply_theme_with_a_light_scheme_installs_light_visuals() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, &light_theme());
        assert!(
            !ctx.global_style().visuals.dark_mode,
            "a light theme must not leave egui in dark mode"
        );
    }

    #[test]
    fn apply_theme_with_a_light_scheme_pins_the_light_egui_theme() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, &light_theme());
        assert_eq!(
            ctx.theme(),
            egui::Theme::Light,
            "the OS theme must not swap stock dark visuals in"
        );
    }

    #[test]
    fn apply_theme_with_a_light_scheme_fills_panels_with_the_theme_surface() {
        let ctx = egui::Context::default();
        let theme = light_theme();
        apply_theme(&ctx, &theme);
        let visuals = &ctx.global_style().visuals;
        assert!(!visuals.dark_mode, "the light style is the active one");
        assert_color_is(visuals.panel_fill, theme.surface, "panel_fill");
    }

    #[test]
    fn apply_theme_with_a_dark_scheme_installs_dark_visuals() {
        let ctx = egui::Context::default();
        let theme = dark_theme();
        apply_theme(&ctx, &theme);
        let visuals = &ctx.global_style().visuals;
        assert!(visuals.dark_mode);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_color_is(visuals.panel_fill, theme.surface, "panel_fill");
    }

    #[test]
    fn apply_theme_switches_from_dark_to_light_and_back() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, &dark_theme());
        apply_theme(&ctx, &light_theme());
        assert!(!ctx.global_style().visuals.dark_mode, "dark → light");
        apply_theme(&ctx, &dark_theme());
        assert!(ctx.global_style().visuals.dark_mode, "light → dark");
    }

    /// Shadows on a light field are softer than in the dark: the same black
    /// at the same alpha reads as a smudge on silver.
    #[test]
    fn apply_theme_with_a_light_scheme_softens_the_shadows() {
        let dark_ctx = egui::Context::default();
        apply_theme(&dark_ctx, &dark_theme());
        let light_ctx = egui::Context::default();
        apply_theme(&light_ctx, &light_theme());

        let dark = dark_ctx.global_style().visuals.clone();
        let light = light_ctx.global_style().visuals.clone();
        assert!(
            light.window_shadow.color.a() < dark.window_shadow.color.a(),
            "window shadow: light {} !< dark {}",
            light.window_shadow.color.a(),
            dark.window_shadow.color.a()
        );
        assert!(
            light.popup_shadow.color.a() < dark.popup_shadow.color.a(),
            "popup shadow: light {} !< dark {}",
            light.popup_shadow.color.a(),
            dark.popup_shadow.color.a()
        );
    }
}
