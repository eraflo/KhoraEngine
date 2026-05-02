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

//! Chrome widgets — pieces shared between title bar, status bar and panel
//! headers. Everything below paints to absolute screen-space coordinates and
//! tracks its own click hit-test via [`UiBuilder::interact_rect`].

use khora_sdk::editor_ui::{
    EditorTheme, FontFamilyHint, Icon, Interaction, TextAlign, UiBuilder,
};
// `Icon` is consumed inside `paint_search_pill` via `Icon::Search`.
// `Interaction` is the return type of `paint_search_pill` and `panel_tab` (for
// the click + width tuple). Kept explicit to make the public surface readable.

use super::paint::{paint_icon, paint_text_size, with_alpha};

/// A keyboard-shortcut chip ("⌘", "K", "Esc"). Returns the right edge in
/// screen-space.
pub fn paint_kbd_chip(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    label: &str,
    theme: &EditorTheme,
) -> f32 {
    let measured = ui.measure_text(label, 10.5, FontFamilyHint::Monospace)[0];
    let w = measured.max(14.0) + 10.0;
    let h = 16.0;
    ui.paint_rect_filled(
        origin,
        [w, h],
        theme.surface_active,
        3.0,
    );
    ui.paint_rect_stroke(
        origin,
        [w, h],
        theme.border,
        3.0,
        1.0,
    );
    // Bottom shadow line for the "depth"
    ui.paint_line(
        [origin[0] + 1.0, origin[1] + h - 0.5],
        [origin[0] + w - 1.0, origin[1] + h - 0.5],
        with_alpha(theme.border, 0.5),
        1.0,
    );
    ui.paint_text_styled(
        [origin[0] + w * 0.5, origin[1] + 2.0],
        label,
        10.5,
        theme.text_dim,
        FontFamilyHint::Monospace,
        TextAlign::Center,
    );
    origin[0] + w
}

/// A circular dot indicator (used in status bars and pill chips).
pub fn paint_status_dot(ui: &mut dyn UiBuilder, center: [f32; 2], color: [f32; 4]) {
    ui.paint_circle_filled(center, 3.5, color);
    ui.paint_circle_filled(center, 5.5, with_alpha(color, 0.18));
}

/// The wide "search commands" pill in the title bar. Returns the click
/// interaction (whether the user wants to open the command palette) plus the
/// right edge in screen-space.
pub fn paint_search_pill(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    height: f32,
    placeholder: &str,
    theme: &EditorTheme,
) -> (Interaction, f32) {
    ui.paint_rect_filled(origin, [width, height], theme.surface_elevated, 6.0);
    ui.paint_rect_stroke(
        origin,
        [width, height],
        with_alpha(theme.separator, 0.6),
        6.0,
        1.0,
    );

    let cy = origin[1] + height * 0.5;
    paint_icon(
        ui,
        [origin[0] + 10.0, cy - 6.5],
        Icon::Search,
        13.0,
        theme.text_dim,
    );

    paint_text_size(
        ui,
        [origin[0] + 30.0, origin[1] + (height - theme.font_size_body) * 0.5],
        placeholder,
        theme.font_size_body - 0.5,
        theme.text_muted,
    );

    // Kbd chips on the right ⌘ K
    let chip_y = origin[1] + (height - 16.0) * 0.5;
    let mut x = origin[0] + width - 12.0;
    for ch in ["K", "⌘"] {
        let measured = ui.measure_text(ch, 10.5, FontFamilyHint::Monospace)[0];
        let w = measured.max(14.0) + 10.0;
        x -= w + 2.0;
        paint_kbd_chip(ui, [x, chip_y], ch, theme);
    }

    let interaction = ui.interact_rect("titlebar-search", [origin[0], origin[1], width, height]);
    (interaction, origin[0] + width)
}

/// A panel header tab. Returns whether it was clicked plus its width.
pub fn panel_tab(
    ui: &mut dyn UiBuilder,
    id_salt: &str,
    origin: [f32; 2],
    label: &str,
    badge: Option<&str>,
    active: bool,
    theme: &EditorTheme,
) -> (bool, f32) {
    let label_w =
        ui.measure_text(label, theme.font_size_body - 1.0, FontFamilyHint::Proportional)[0];
    let badge_w = badge
        .map(|b| ui.measure_text(b, 9.5, FontFamilyHint::Monospace)[0] + 10.0)
        .unwrap_or(0.0);
    let pad_x = 10.0;
    let w = pad_x * 2.0 + label_w + if badge.is_some() { badge_w + 6.0 } else { 0.0 };
    let h = 22.0;

    let bg = if active { theme.surface_active } else { theme.surface };
    if active {
        ui.paint_rect_filled(origin, [w, h], bg, 4.0);
    }

    let text_color = if active { theme.text } else { theme.text_dim };
    paint_text_size(
        ui,
        [origin[0] + pad_x, origin[1] + (h - theme.font_size_body) * 0.5],
        label,
        theme.font_size_body - 1.0,
        text_color,
    );

    if let Some(badge) = badge {
        let badge_x = origin[0] + pad_x + label_w + 6.0;
        let badge_y = origin[1] + 4.0;
        ui.paint_rect_filled(
            [badge_x, badge_y],
            [badge_w - 4.0, 14.0],
            if active {
                with_alpha(theme.primary, 0.20)
            } else {
                theme.surface_elevated
            },
            999.0,
        );
        let badge_text_color = if active { theme.primary } else { theme.text_dim };
        ui.paint_text_styled(
            [badge_x + (badge_w - 4.0) * 0.5, badge_y + 1.0],
            badge,
            9.5,
            badge_text_color,
            FontFamilyHint::Monospace,
            TextAlign::Center,
        );
    }

    let interaction = ui.interact_rect(id_salt, [origin[0], origin[1], w, h]);
    (interaction.clicked, w)
}

/// Paints a panel-header strip: subtle gradient bg, hairline at bottom, then
/// a row of tabs returned through `tabs_clicked` and a row of action icons on
/// the right. Returns the y-coordinate where panel content can begin.
pub fn paint_panel_header(
    ui: &mut dyn UiBuilder,
    panel_rect: [f32; 4],
    height: f32,
    theme: &EditorTheme,
) {
    let [x, y, w, _] = panel_rect;
    super::paint::paint_vertical_gradient(
        ui,
        [x, y, w, height],
        theme.surface_elevated,
        theme.surface,
        4,
    );
    ui.paint_line(
        [x, y + height],
        [x + w, y + height],
        with_alpha(theme.separator, 0.55),
        1.0,
    );
}
