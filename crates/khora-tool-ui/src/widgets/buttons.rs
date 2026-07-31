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

//! Buttons — the three kinds, each with all their states.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill, fill_stroke, icon_centered, text_centered, tint};
use super::{Color, Rect};

/// Which of the three button roles this is. There are exactly three, on
/// purpose — a fourth would mean two of them are doing the same job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one action the screen wants you to take. Filled silver, dark ink.
    Primary,
    /// Everything else. Subtle fill, hairline border.
    Ghost,
    /// Destructive. Red text on transparent, red fill on hover.
    Danger,
}

/// Resolves `(fill, border, foreground)` for a button in a given state.
fn colors(
    theme: &UiTheme,
    kind: ButtonKind,
    enabled: bool,
    hovered: bool,
) -> (Color, Color, Color) {
    if !enabled {
        // One disabled look for all three kinds — a disabled control has no
        // role left to signal.
        return (theme.surface_elevated, theme.border, theme.text_disabled);
    }
    match kind {
        ButtonKind::Primary => {
            let f = if hovered {
                // Lift the silver slightly rather than introducing a new token.
                super::paint::lerp_color(theme.primary, [1.0, 1.0, 1.0, 1.0], 0.12)
            } else {
                theme.primary
            };
            (f, f, theme.text_inverse)
        }
        ButtonKind::Ghost => {
            let f = if hovered {
                theme.surface_active
            } else {
                theme.surface_interactive
            };
            let b = if hovered {
                theme.border_strong
            } else {
                theme.border
            };
            (f, b, theme.text)
        }
        ButtonKind::Danger => {
            let f = if hovered {
                tint(theme.error, 0.16)
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };
            (f, tint(theme.error, 0.5), theme.error)
        }
    }
}

/// What a button says and how it behaves.
///
/// Content lives in a descriptor rather than a long positional argument list so
/// that a new option (a trailing icon, a busy state) can be added here without
/// touching a single call site.
#[derive(Debug, Clone, Copy)]
pub struct Button<'a> {
    /// The label.
    pub label: &'a str,
    /// Which role this button plays.
    pub kind: ButtonKind,
    /// Disabled buttons are greyed and swallow clicks.
    pub enabled: bool,
    /// Optional icon shown before the label.
    pub leading: Option<Icon>,
}

impl<'a> Button<'a> {
    /// An enabled button with no icon.
    pub fn new(label: &'a str, kind: ButtonKind) -> Self {
        Self {
            label,
            kind,
            enabled: true,
            leading: None,
        }
    }

    /// Sets the enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Adds a leading icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.leading = Some(glyph);
        self
    }
}

/// A text button (optionally with a leading icon).
///
/// Returns the [`Interaction`]; a disabled button never reports `clicked`,
/// even if the pointer is over it.
pub fn button(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    spec: Button<'_>,
) -> Interaction {
    let Button {
        label,
        kind,
        enabled,
        leading,
    } = spec;

    let raw = ui.interact_rect(id_salt, rect);
    let hovered = enabled && raw.hovered;
    let (bg, border, fg) = colors(theme, kind, enabled, hovered);

    fill_stroke(ui, rect, bg, border, theme.radius_md);

    let size = theme.font_size_body;
    match leading {
        Some(glyph) => {
            // Icon + label as a unit, roughly centred: the icon sits one
            // icon-width left of the text block's centre.
            let gap = 6.0;
            let icon_w = size;
            let text_w = ui.measure_text(label, size, FontFamilyHint::Proportional)[0];
            let total = icon_w + gap + text_w;
            let start_x = rect[0] + (rect[2] - total) * 0.5;
            super::paint::icon(ui, [start_x, super::text_y(rect, size)], glyph, size, fg);
            super::paint::text(
                ui,
                [start_x + icon_w + gap, super::text_y(rect, size)],
                label,
                size,
                fg,
            );
        }
        None => text_centered(ui, rect, label, size, fg, FontFamilyHint::Proportional),
    }

    if enabled {
        raw
    } else {
        Interaction::default()
    }
}

/// A square icon-only button — toolbar and row-action affordance.
pub fn icon_button(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    glyph: Icon,
    enabled: bool,
) -> Interaction {
    let raw = ui.interact_rect(id_salt, rect);
    let hovered = enabled && raw.hovered;

    let fg = if !enabled {
        theme.text_disabled
    } else if hovered {
        theme.text
    } else {
        theme.text_muted
    };

    if hovered {
        fill(ui, rect, theme.surface_interactive, theme.radius_sm);
    }
    icon_centered(ui, rect, glyph, theme.font_size_title, fg);

    if enabled {
        raw
    } else {
        Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    const R: Rect = [0.0, 0.0, 120.0, 30.0];

    #[test]
    fn primary_fills_with_brand_and_writes_inverse_ink() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0);
        button(
            &mut ui,
            &theme,
            R,
            "save",
            Button::new("Save", ButtonKind::Primary),
        );
        assert!(
            ui.used_fill(theme.primary),
            "primary button must fill with the brand color"
        );
        assert_eq!(
            ui.text_color("Save"),
            Some(theme.text_inverse),
            "label on a filled silver button must use the inverse ink"
        );
    }

    #[test]
    fn ghost_uses_surface_not_brand() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0);
        button(
            &mut ui,
            &theme,
            R,
            "browse",
            Button::new("Browse…", ButtonKind::Ghost),
        );
        assert!(ui.used_fill(theme.surface_interactive));
        assert!(
            !ui.used_fill(theme.primary),
            "a ghost button must never fill with the brand color"
        );
        assert_eq!(ui.text_color("Browse…"), Some(theme.text));
    }

    #[test]
    fn danger_writes_in_the_error_color() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0);
        button(
            &mut ui,
            &theme,
            R,
            "del",
            Button::new("Delete", ButtonKind::Danger).icon(Icon::Trash),
        );
        assert_eq!(ui.text_color("Delete"), Some(theme.error));
    }

    #[test]
    fn disabled_button_swallows_the_click() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0).with_click("save");
        let out = button(
            &mut ui,
            &theme,
            R,
            "save",
            Button::new("Save", ButtonKind::Primary).enabled(false),
        );
        assert!(
            !out.clicked,
            "a disabled button must not report a click even when clicked"
        );
        assert_eq!(
            ui.text_color("Save"),
            Some(theme.text_disabled),
            "disabled label uses the disabled ink"
        );
        assert!(
            !ui.used_fill(theme.primary),
            "disabled button must not keep the brand fill"
        );
    }

    #[test]
    fn enabled_button_reports_the_click() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0).with_click("save");
        let out = button(
            &mut ui,
            &theme,
            R,
            "save",
            Button::new("Save", ButtonKind::Primary),
        );
        assert!(out.clicked);
    }

    #[test]
    fn icon_button_only_paints_a_hover_plate_when_hovered() {
        let theme = khora_dark();

        let mut idle = RecordingUiBuilder::new(40.0, 40.0);
        icon_button(
            &mut idle,
            &theme,
            [0.0, 0.0, 30.0, 30.0],
            "rm",
            Icon::Trash,
            true,
        );
        assert!(
            idle.rects_filled().is_empty(),
            "idle icon button should paint no plate"
        );

        let mut hot = RecordingUiBuilder::new(40.0, 40.0).with_hover("rm");
        icon_button(
            &mut hot,
            &theme,
            [0.0, 0.0, 30.0, 30.0],
            "rm",
            Icon::Trash,
            true,
        );
        assert_eq!(
            hot.rects_filled().len(),
            1,
            "hovered icon button gets a plate"
        );
    }
}
