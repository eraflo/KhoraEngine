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

//! Chips and pills — small status carriers.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction};
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill_stroke, mono, tint};
use super::{text_y, Color, Rect};

/// The semantic tone of a chip. Colour is never decorative here — the tone
/// *is* the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// No signal — counts, versions, inert metadata.
    Neutral,
    /// Healthy, connected, done.
    Success,
    /// Needs attention, pre-release, degraded.
    Warning,
    /// Failed, destructive, offline.
    Error,
    /// Informational.
    Info,
    /// Brand-tinted (selection-adjacent, "current").
    Brand,
}

impl Tone {
    /// The chip's foreground (also its dot) for a theme.
    pub fn fg(self, theme: &UiTheme) -> Color {
        match self {
            Tone::Neutral => theme.text_muted,
            Tone::Success => theme.success,
            Tone::Warning => theme.warning,
            Tone::Error => theme.error,
            Tone::Info => theme.accent_b,
            Tone::Brand => theme.primary,
        }
    }

    /// Fill + border derive from the foreground so a new tone can never fall
    /// out of the system: 14% fill, 35% border, exactly as the mockups.
    fn plate(self, theme: &UiTheme) -> (Color, Color) {
        match self {
            Tone::Neutral => (theme.surface_elevated, theme.border),
            other => {
                let c = other.fg(theme);
                (tint(c, 0.14), tint(c, 0.35))
            }
        }
    }
}

/// Height every chip renders at — they sit in dense rows and must line up.
const CHIP_H: f32 = 18.0;

/// A status chip: an optional dot plus a short monospace label.
///
/// Chips carry a state, so the label reads as data — `connected`, `0.7.0`,
/// `pre-release` — and renders in the monospace face.
pub fn chip(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    label: &str,
    tone: Tone,
    with_dot: bool,
) {
    let (bg, border) = tone.plate(theme);
    let fg = tone.fg(theme);
    let r = [rect[0], rect[1], rect[2], CHIP_H.min(rect[3])];
    fill_stroke(ui, r, bg, border, r[3] * 0.5);

    let size = theme.font_size_caption;
    let mut x = r[0] + 7.0;
    if with_dot {
        ui.paint_circle_filled([x + 3.0, r[1] + r[3] * 0.5], 3.0, fg);
        x += 11.0;
    }
    mono(ui, [x, text_y(r, size)], label, size, fg);
}

/// A keyboard hint chip — `Ctrl`, `↵`, `esc`. Always neutral: a shortcut is
/// never a status.
pub fn kbd_chip(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, key: &str) {
    fill_stroke(
        ui,
        rect,
        theme.surface_elevated,
        theme.border,
        theme.radius_sm,
    );
    let size = theme.font_size_caption - 1.0;
    let c = super::center(rect);
    ui.paint_text_styled(
        [c[0], text_y(rect, size)],
        key,
        size,
        theme.text_muted,
        FontFamilyHint::Monospace,
        khora_core::ui::editor::ui_builder::TextAlign::Center,
    );
}

/// What a filter pill shows and whether it is engaged.
#[derive(Debug, Clone, Copy)]
pub struct Pill<'a> {
    /// The label ("Info", "Warn"…).
    pub label: &'a str,
    /// A live count, if the filter has one.
    pub count: Option<usize>,
    /// The tone of the thing being filtered.
    pub tone: Tone,
    /// Whether the filter is currently engaged.
    pub on: bool,
}

impl<'a> Pill<'a> {
    /// An engaged pill with no count.
    pub fn new(label: &'a str, tone: Tone) -> Self {
        Self {
            label,
            count: None,
            tone,
            on: true,
        }
    }

    /// Attaches a live count.
    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }

    /// Sets whether the filter is engaged.
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// A toggleable filter pill with a live count — the console's log-level
/// filters.
///
/// `on == false` fades the whole pill rather than recolouring it, so the tone
/// still reads at a glance and the control keeps one visual identity in both
/// states. Returns the [`Interaction`] so the caller can flip its own state.
pub fn filter_pill(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    spec: Pill<'_>,
) -> Interaction {
    let Pill {
        label,
        count,
        tone,
        on,
    } = spec;

    let out = ui.interact_rect(id_salt, rect);

    let fade = if on { 1.0 } else { 0.4 };
    let (bg, border) = (tint(theme.surface_elevated, fade), tint(theme.border, fade));
    let fg_dot = tint(tone.fg(theme), fade);
    let fg_label = tint(theme.text_muted, fade);
    let fg_count = tint(theme.text, fade);

    fill_stroke(ui, rect, bg, border, rect[3] * 0.5);

    let size = theme.font_size_caption;
    let mut x = rect[0] + 8.0;
    ui.paint_circle_filled([x + 3.0, rect[1] + rect[3] * 0.5], 3.0, fg_dot);
    x += 11.0;

    super::paint::text(ui, [x, text_y(rect, size)], label, size, fg_label);

    if let Some(n) = count {
        let label_w = ui.measure_text(label, size, FontFamilyHint::Proportional)[0];
        mono(
            ui,
            [x + label_w + 6.0, text_y(rect, size)],
            &n.to_string(),
            size,
            fg_count,
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn tone_maps_to_the_matching_status_slot() {
        let t = khora_dark();
        assert_eq!(Tone::Success.fg(&t), t.success);
        assert_eq!(Tone::Warning.fg(&t), t.warning);
        assert_eq!(Tone::Error.fg(&t), t.error);
        assert_eq!(Tone::Brand.fg(&t), t.primary);
    }

    #[test]
    fn chip_label_is_monospace_and_tone_colored() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(120.0, 24.0);
        chip(
            &mut ui,
            &t,
            [0.0, 0.0, 100.0, 18.0],
            "connected",
            Tone::Success,
            true,
        );

        assert_eq!(ui.text_color("connected"), Some(t.success));
        assert_eq!(ui.text_family("connected"), Some(FontFamilyHint::Monospace));
        assert_eq!(
            ui.circles_filled().len(),
            1,
            "dot requested → one dot painted"
        );
    }

    #[test]
    fn chip_without_dot_paints_no_circle() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(120.0, 24.0);
        chip(
            &mut ui,
            &t,
            [0.0, 0.0, 100.0, 18.0],
            "0.7.0",
            Tone::Neutral,
            false,
        );
        assert!(ui.circles_filled().is_empty());
    }

    #[test]
    fn filter_pill_off_fades_every_layer() {
        let t = khora_dark();
        let spec = Pill::new("Warn", Tone::Warning).count(3);

        let mut on = RecordingUiBuilder::new(120.0, 24.0);
        filter_pill(&mut on, &t, [0.0, 0.0, 90.0, 18.0], "warn", spec);

        let mut off = RecordingUiBuilder::new(120.0, 24.0);
        filter_pill(&mut off, &t, [0.0, 0.0, 90.0, 18.0], "warn", spec.on(false));

        let on_label = on.text_color("Warn").unwrap();
        let off_label = off.text_color("Warn").unwrap();
        assert!(
            off_label[3] < on_label[3],
            "an off pill must be faded, not recolored ({} !< {})",
            off_label[3],
            on_label[3]
        );

        // …and it must still be the same hue, so the tone stays readable.
        assert_eq!(
            [off_label[0], off_label[1], off_label[2]],
            [on_label[0], on_label[1], on_label[2]]
        );
    }

    #[test]
    fn filter_pill_renders_its_count_in_monospace() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(120.0, 24.0);
        filter_pill(
            &mut ui,
            &t,
            [0.0, 0.0, 90.0, 18.0],
            "info",
            Pill::new("Info", Tone::Info).count(128),
        );
        assert!(ui.painted_text("128"));
        assert_eq!(ui.text_family("128"), Some(FontFamilyHint::Monospace));
    }

    #[test]
    fn filter_pill_reports_clicks_so_the_caller_can_toggle() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(120.0, 24.0).with_click("debug");
        let out = filter_pill(
            &mut ui,
            &t,
            [0.0, 0.0, 90.0, 18.0],
            "debug",
            Pill::new("Debug", Tone::Neutral).on(false),
        );
        assert!(out.clicked);
    }
}
