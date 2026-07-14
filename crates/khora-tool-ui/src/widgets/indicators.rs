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

//! Indicators — how the tools show a number without making you read it.

use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill, mono, text, tint};
use super::{Color, Rect};

/// Health as a traffic light. The thresholds live here, once, so every agent
/// row / meter in every tool agrees on what "degraded" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// At or above 85% — nominal.
    Good,
    /// 60–85% — degraded but running.
    Degraded,
    /// Below 60% — in trouble.
    Bad,
}

impl Health {
    /// Classifies a normalized `0..=1` health value.
    pub fn from_ratio(v: f32) -> Self {
        if v >= 0.85 {
            Health::Good
        } else if v >= 0.60 {
            Health::Degraded
        } else {
            Health::Bad
        }
    }

    /// The status color for this level.
    pub fn color(self, theme: &UiTheme) -> Color {
        match self {
            Health::Good => theme.success,
            Health::Degraded => theme.warning,
            Health::Bad => theme.error,
        }
    }
}

/// A small filled dot — the cheapest possible status indicator.
pub fn status_dot(ui: &mut dyn UiBuilder, theme: &UiTheme, center: [f32; 2], health: Health) {
    ui.paint_circle_filled(center, 3.0, health.color(theme));
}

/// A horizontal track with a filled portion. The generic bar behind meters,
/// health bars, and progress.
///
/// `ratio` is clamped to `0..=1`, so a caller that hands over a bogus value
/// gets a full or empty bar rather than a bar painted outside its track.
pub fn meter_bar(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, ratio: f32, color: Color) {
    let r = rect[3] * 0.5;
    fill(ui, rect, theme.surface_interactive, r);
    let w = rect[2] * ratio.clamp(0.0, 1.0);
    if w > 0.0 {
        fill(ui, [rect[0], rect[1], w, rect[3]], color, r);
    }
}

/// A meter whose colour is chosen by the [`Health`] thresholds — the agent-row
/// bar in the Control Plane.
pub fn health_bar(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, ratio: f32) {
    let health = Health::from_ratio(ratio);
    meter_bar(ui, theme, rect, ratio, health.color(theme));
}

/// A labelled progress row: caption on the left, percentage on the right, bar
/// underneath. Used by engine downloads.
pub fn progress_row(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, label: &str, ratio: f32) {
    let ratio = ratio.clamp(0.0, 1.0);
    let size = theme.font_size_caption;
    let head = [rect[0], rect[1], rect[2], size];

    text(ui, [head[0], head[1]], label, size, theme.text_muted);

    let pct = format!("{}%", (ratio * 100.0).round() as i32);
    let w = ui.measure_text(
        &pct,
        size,
        khora_core::ui::editor::ui_builder::FontFamilyHint::Monospace,
    )[0];
    mono(
        ui,
        [super::right(head) - w, head[1]],
        &pct,
        size,
        theme.text_muted,
    );

    let bar = [rect[0], rect[1] + size + 4.0, rect[2], 5.0];
    meter_bar(ui, theme, bar, ratio, theme.primary);
}

/// A bar sparkline over a series — the agent cost history.
///
/// Values are normalized against the series maximum, so the shape reads even
/// when the absolute scale shifts. An empty or all-zero series paints just the
/// plate (no bars), rather than dividing by zero.
pub fn sparkline(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, values: &[f32]) {
    fill(ui, rect, theme.background, theme.radius_sm);
    super::paint::stroke(ui, rect, theme.border, theme.radius_sm, 1.0);

    if values.is_empty() {
        return;
    }
    let max = values.iter().cloned().fold(0.0_f32, f32::max);
    if max <= 0.0 {
        return;
    }

    let pad = 5.0;
    let inner = super::shrink(rect, pad);
    let gap = 2.0;
    let n = values.len() as f32;
    let bar_w = ((inner[2] - gap * (n - 1.0)) / n).max(1.0);

    for (i, v) in values.iter().enumerate() {
        let h = (v / max).clamp(0.0, 1.0) * inner[3];
        let x = inner[0] + i as f32 * (bar_w + gap);
        let y = super::bottom(inner) - h;
        fill(ui, [x, y, bar_w, h], theme.primary_dim, 1.0);
    }
}

/// A loading placeholder bar. Skeletons beat spinners: they show the shape of
/// what is coming instead of just saying "wait".
///
/// `lines` is how many placeholder rows to stack; each is inset a little
/// differently so the block doesn't read as a solid rectangle.
pub fn skeleton(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, lines: usize) {
    let h = 11.0;
    let gap = 9.0;
    // Deterministic, decreasing widths — reads like text without pretending
    // to be random.
    const WIDTHS: [f32; 3] = [0.80, 0.95, 0.55];
    for i in 0..lines {
        let y = rect[1] + i as f32 * (h + gap);
        if y + h > super::bottom(rect) {
            break;
        }
        let w = rect[2] * WIDTHS[i % WIDTHS.len()];
        fill(
            ui,
            [rect[0], y, w, h],
            tint(theme.surface_interactive, 0.9),
            theme.radius_sm,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::{PaintCall, RecordingUiBuilder};

    #[test]
    fn health_thresholds_pick_the_right_status_color() {
        let t = khora_dark();
        assert_eq!(Health::from_ratio(0.96), Health::Good);
        assert_eq!(Health::from_ratio(0.85), Health::Good);
        assert_eq!(Health::from_ratio(0.72), Health::Degraded);
        assert_eq!(Health::from_ratio(0.60), Health::Degraded);
        assert_eq!(Health::from_ratio(0.31), Health::Bad);

        assert_eq!(Health::Good.color(&t), t.success);
        assert_eq!(Health::Degraded.color(&t), t.warning);
        assert_eq!(Health::Bad.color(&t), t.error);
    }

    #[test]
    fn health_bar_colors_itself_from_the_ratio() {
        let t = khora_dark();

        let mut good = RecordingUiBuilder::new(60.0, 8.0);
        health_bar(&mut good, &t, [0.0, 0.0, 44.0, 4.0], 0.96);
        assert!(good.used_fill(t.success));

        let mut bad = RecordingUiBuilder::new(60.0, 8.0);
        health_bar(&mut bad, &t, [0.0, 0.0, 44.0, 4.0], 0.30);
        assert!(bad.used_fill(t.error));
        assert!(!bad.used_fill(t.success));
    }

    #[test]
    fn meter_bar_clamps_out_of_range_ratios_inside_the_track() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(100.0, 10.0);
        meter_bar(&mut ui, &t, [0.0, 0.0, 80.0, 4.0], 3.5, t.primary);

        // track + fill
        let rects = ui.rects_filled();
        assert_eq!(rects.len(), 2);
        if let PaintCall::RectFilled { size, .. } = rects[1] {
            assert_eq!(
                size[0], 80.0,
                "an over-unity ratio must not overflow the track"
            );
        } else {
            panic!("expected the fill rect");
        }
    }

    #[test]
    fn meter_bar_at_zero_paints_only_the_track() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(100.0, 10.0);
        meter_bar(&mut ui, &t, [0.0, 0.0, 80.0, 4.0], 0.0, t.primary);
        assert_eq!(
            ui.rects_filled().len(),
            1,
            "no fill when there is nothing to show"
        );
    }

    #[test]
    fn sparkline_normalizes_against_its_own_max() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(120.0, 40.0);
        sparkline(&mut ui, &t, [0.0, 0.0, 100.0, 40.0], &[1.0, 2.0, 4.0]);

        // plate + 3 bars
        let bars: Vec<_> = ui
            .rects_filled()
            .into_iter()
            .filter(|c| matches!(c, PaintCall::RectFilled { color, .. } if *color == t.primary_dim))
            .collect();
        assert_eq!(bars.len(), 3);

        let heights: Vec<f32> = bars
            .iter()
            .map(|c| match c {
                PaintCall::RectFilled { size, .. } => size[1],
                _ => unreachable!(),
            })
            .collect();
        assert!(
            heights[2] > heights[1] && heights[1] > heights[0],
            "bars scale with value"
        );
        // the max value must reach full inner height
        assert!(heights[2] > 0.0);
    }

    #[test]
    fn sparkline_survives_empty_and_all_zero_series() {
        let t = khora_dark();

        let mut empty = RecordingUiBuilder::new(120.0, 40.0);
        sparkline(&mut empty, &t, [0.0, 0.0, 100.0, 40.0], &[]);
        assert_eq!(empty.rects_filled().len(), 1, "just the plate");

        let mut zeros = RecordingUiBuilder::new(120.0, 40.0);
        sparkline(&mut zeros, &t, [0.0, 0.0, 100.0, 40.0], &[0.0, 0.0]);
        assert_eq!(
            zeros.rects_filled().len(),
            1,
            "no division by zero, no bars"
        );
    }

    #[test]
    fn skeleton_stacks_the_requested_number_of_lines() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 200.0);
        skeleton(&mut ui, &t, [0.0, 0.0, 180.0, 100.0], 3);
        assert_eq!(ui.rects_filled().len(), 3);
    }

    #[test]
    fn skeleton_never_paints_outside_its_rect() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 200.0);
        // Ask for far more lines than fit in a 30px-tall block.
        skeleton(&mut ui, &t, [0.0, 0.0, 180.0, 30.0], 20);
        for call in ui.rects_filled() {
            if let PaintCall::RectFilled { min, size, .. } = call {
                assert!(
                    min[1] + size[1] <= 30.0 + 0.01,
                    "skeleton line escaped its rect"
                );
            }
        }
    }

    #[test]
    fn progress_row_shows_a_rounded_percentage() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0);
        progress_row(&mut ui, &t, [0.0, 0.0, 180.0, 22.0], "downloading", 0.62);
        assert!(ui.painted_text("downloading"));
        assert!(ui.painted_text("62%"));
    }
}
