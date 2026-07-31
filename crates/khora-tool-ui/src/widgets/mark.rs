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

//! The brand mark — Khora's diamond, and the wordmark lockup.

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{mono, text};
use super::{text_y, Color, Rect};

/// Paints the Khora diamond — a square rotated 45°, centred on `center`.
///
/// It is drawn as a path rather than a rotated rect because the paint API has
/// no rotation, and a 4-point polygon is exactly what a diamond is.
pub fn diamond(ui: &mut dyn UiBuilder, center: [f32; 2], size: f32, color: Color) {
    let h = size * 0.5;
    ui.paint_path_filled(
        &[
            [center[0], center[1] - h],
            [center[0] + h, center[1]],
            [center[0], center[1] + h],
            [center[0] - h, center[1]],
        ],
        color,
    );
}

/// The brand lockup: diamond, "Khora", and an optional monospace context chip
/// separated by a hairline — the top-left of every Khora tool window.
///
/// Returns the x coordinate just past the lockup, so the caller can continue
/// laying out the title bar.
pub fn brand_pill(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, context: &str) -> f32 {
    let cy = rect[1] + rect[3] * 0.5;
    let mut x = rect[0];

    diamond(ui, [x + 6.0, cy], 11.0, theme.primary);
    x += 6.0 + 8.0;

    let size = theme.font_size_body + 0.5;
    text(ui, [x, text_y(rect, size)], "Khora", size, theme.text);
    x += ui.measure_text("Khora", size, FontFamilyHint::Proportional)[0];

    if !context.is_empty() {
        x += 9.0;
        ui.paint_line(
            [x, rect[1] + rect[3] * 0.25],
            [x, rect[1] + rect[3] * 0.75],
            theme.separator,
            1.0,
        );
        x += 9.0;

        let csize = theme.font_size_caption;
        mono(
            ui,
            [x, text_y(rect, csize)],
            context,
            csize,
            theme.text_muted,
        );
        x += ui.measure_text(context, csize, FontFamilyHint::Monospace)[0];
    }

    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::{PaintCall, RecordingUiBuilder};

    #[test]
    fn diamond_is_a_four_point_polygon() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(40.0, 40.0);
        diamond(&mut ui, [20.0, 20.0], 12.0, t.primary);

        let paths: Vec<_> = ui
            .calls()
            .iter()
            .filter_map(|c| match c {
                PaintCall::Path { points, color } => Some((points, color)),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0.len(), 4, "a diamond has four corners");
        assert_eq!(*paths[0].1, t.primary);
    }

    #[test]
    fn diamond_points_are_symmetric_about_its_center() {
        let mut ui = RecordingUiBuilder::new(40.0, 40.0);
        diamond(&mut ui, [20.0, 20.0], 10.0, [1.0; 4]);
        if let PaintCall::Path { points, .. } = &ui.calls()[0] {
            // top/bottom share x with the centre; left/right share y.
            assert_eq!(points[0][0], 20.0);
            assert_eq!(points[2][0], 20.0);
            assert_eq!(points[1][1], 20.0);
            assert_eq!(points[3][1], 20.0);
            assert_eq!(points[0][1], 15.0);
            assert_eq!(points[2][1], 25.0);
        } else {
            panic!("expected a path");
        }
    }

    #[test]
    fn brand_pill_writes_the_wordmark_and_a_mono_context() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 44.0);
        let end = brand_pill(&mut ui, &t, [0.0, 0.0, 280.0, 44.0], "nightfall.kscene");

        assert!(ui.painted_text("Khora"));
        assert_eq!(
            ui.text_family("nightfall.kscene"),
            Some(FontFamilyHint::Monospace),
            "the project context is data — monospace"
        );
        assert!(end > 0.0, "returns the x cursor past the lockup");
    }

    #[test]
    fn brand_pill_without_context_paints_no_separator() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 44.0);
        brand_pill(&mut ui, &t, [0.0, 0.0, 280.0, 44.0], "");
        assert!(
            !ui.calls()
                .iter()
                .any(|c| matches!(c, PaintCall::Line { .. })),
            "no context → no divider"
        );
    }
}
