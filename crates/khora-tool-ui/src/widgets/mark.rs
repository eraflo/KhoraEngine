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

//! The brand mark — the Khora sign, and the wordmark lockup.
//!
//! The sign is a square cut along a K: one cut straight down through the
//! centre, two at 45° from the centre to the right-hand corners. The pieces
//! never move; the K is the gap between them (`docs/src/design/editor.md`,
//! "Brand & identity").

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::{GradientStop, LinearGradient};
use khora_core::ui::{ColorScheme, UiBuilder, UiTheme};

use super::paint::{mono, text};
use super::{text_y, Rect};

/// The sign's silver: `#f1f5f9` → `#bccbdb` → `#7d94ad`, sRGB, opaque, from
/// the square's top-left corner to its bottom-right corner.
pub const SIGN_SILVER: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: [241.0 / 255.0, 245.0 / 255.0, 249.0 / 255.0, 1.0],
    },
    GradientStop {
        offset: 0.5,
        color: [188.0 / 255.0, 203.0 / 255.0, 219.0 / 255.0, 1.0],
    },
    GradientStop {
        offset: 1.0,
        color: [125.0 / 255.0, 148.0 / 255.0, 173.0 / 255.0, 1.0],
    },
];

/// The sign's navy: `#141b28`, sRGB, opaque — the finish on a light field.
pub const SIGN_NAVY: [GradientStop; 1] = [GradientStop {
    offset: 0.0,
    color: [20.0 / 255.0, 27.0 / 255.0, 40.0 / 255.0, 1.0],
}];

/// Paints the Khora sign — a square cut along a K — in the square spanning
/// `center ± size / 2`, at `opacity` (clamped to `0..=1`).
///
/// The finish follows `theme`: [`SIGN_SILVER`] on a dark scheme,
/// [`SIGN_NAVY`] on a light one.
///
/// The cut is 14 on a side of 400 at every size, never thickened: below about
/// 28 px it falls under a pixel and only anti-aliasing shows it. A small size
/// that must read crisply needs the sign redrawn on the pixel grid by hand.
pub fn sign(ui: &mut dyn UiBuilder, theme: &UiTheme, center: [f32; 2], size: f32, opacity: f32) {
    let half = size * 0.5;
    // Half the cut: 7 on a side of 400.
    let h = size * 7.0 / 400.0;
    let a = h * std::f32::consts::SQRT_2;
    let b = h + a;
    let at = |x: f32, y: f32| [center[0] + x, center[1] + y];

    // Silver on a dark surface, navy on a light one.
    let finish: &[GradientStop] = match theme.scheme {
        ColorScheme::Dark => &SIGN_SILVER,
        ColorScheme::Light => &SIGN_NAVY,
    };
    let alpha = opacity.clamp(0.0, 1.0);
    let mut stops = [GradientStop {
        offset: 0.0,
        color: [0.0; 4],
    }; SIGN_SILVER.len()];
    for (stop, s) in stops.iter_mut().zip(finish) {
        *stop = GradientStop {
            offset: s.offset,
            color: [s.color[0], s.color[1], s.color[2], s.color[3] * alpha],
        };
    }
    let gradient = LinearGradient {
        from: at(-half, -half),
        to: at(half, half),
        stops: &stops[..finish.len()],
    };

    let left = [
        at(-half, -half),
        at(-h, -half),
        at(-h, half),
        at(-half, half),
    ];
    let top = [at(h, -half), at(half - a, -half), at(h, -b)];
    let right = [at(a, 0.0), at(half, -half + a), at(half, half - a)];
    let bottom = [at(h, half), at(h, b), at(half - a, half)];
    ui.paint_path_filled_gradient(&left, gradient);
    ui.paint_path_filled_gradient(&top, gradient);
    ui.paint_path_filled_gradient(&right, gradient);
    ui.paint_path_filled_gradient(&bottom, gradient);
}

/// The brand lockup: the sign, "Khora", and an optional monospace context chip
/// separated by a hairline — the top-left of every Khora tool window.
///
/// Returns the x coordinate just past the lockup, so the caller can continue
/// laying out the title bar.
pub fn brand_pill(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, context: &str) -> f32 {
    let cy = rect[1] + rect[3] * 0.5;
    let mut x = rect[0];

    sign(ui, theme, [x + 6.0, cy], 12.0, 1.0);
    x += 12.0 + 8.0;

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

    #[test]
    fn brand_pill_paints_the_sign_and_no_flat_path() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 44.0);
        brand_pill(&mut ui, &t, [0.0, 0.0, 280.0, 44.0], "nightfall.kscene");
        assert_eq!(gradient_paths(&ui).len(), 4, "the sign is four pieces");
        assert!(
            !ui.calls()
                .iter()
                .any(|c| matches!(c, PaintCall::Path { .. })),
            "no flat polygon — the diamond is gone"
        );
    }

    // ── The sign ──────────────────────────────────────

    const EPS: f32 = 1e-3;
    const SQRT_2: f32 = std::f32::consts::SQRT_2;

    /// One recorded gradient piece.
    struct Piece {
        points: Vec<[f32; 2]>,
        from: [f32; 2],
        to: [f32; 2],
        stops: Vec<GradientStop>,
    }

    fn gradient_paths(ui: &RecordingUiBuilder) -> Vec<Piece> {
        ui.calls()
            .iter()
            .filter_map(|c| match c {
                PaintCall::PathGradient {
                    points,
                    from,
                    to,
                    stops,
                } => Some(Piece {
                    points: points.clone(),
                    from: *from,
                    to: *to,
                    stops: stops.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The sign painted at `center` / `size` / `opacity`, as its four pieces.
    fn pieces(center: [f32; 2], size: f32, opacity: f32) -> Vec<Piece> {
        let mut ui = RecordingUiBuilder::new(800.0, 800.0);
        sign(&mut ui, &khora_dark(), center, size, opacity);
        let pieces = gradient_paths(&ui);
        assert_eq!(pieces.len(), 4, "the sign is four gradient pieces");
        pieces
    }

    fn assert_near(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < EPS,
            "{what}: got {actual}, expected {expected}"
        );
    }

    fn assert_points(actual: &[[f32; 2]], expected: &[[f32; 2]], piece: &str) {
        assert_eq!(actual.len(), expected.len(), "{piece}: vertex count");
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert_near(a[0], e[0], &format!("{piece}[{i}].x"));
            assert_near(a[1], e[1], &format!("{piece}[{i}].y"));
        }
    }

    /// Perpendicular distance from `p` to the line through `a` and `b`.
    fn distance_to_line(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / len
    }

    /// The expected cut width at `size`.
    fn cut_at(size: f32) -> f32 {
        size * 14.0 / 400.0
    }

    #[test]
    fn sign_paints_left_top_right_bottom_with_four_three_three_three_points() {
        let mut ui = RecordingUiBuilder::new(800.0, 800.0);
        sign(&mut ui, &khora_dark(), [0.0, 0.0], 400.0, 1.0);
        let counts: Vec<usize> = gradient_paths(&ui).iter().map(|p| p.points.len()).collect();
        assert_eq!(counts, vec![4, 3, 3, 3]);
        assert_eq!(
            ui.calls().len(),
            4,
            "the sign paints its four pieces and nothing else"
        );
    }

    #[test]
    fn sign_vertices_match_the_spec_at_size_400() {
        let p = pieces([0.0, 0.0], 400.0, 1.0);
        let h = 7.0;
        let a = h * SQRT_2;
        let b = h * (1.0 + SQRT_2);
        assert_points(
            &p[0].points,
            &[[-200.0, -200.0], [-h, -200.0], [-h, 200.0], [-200.0, 200.0]],
            "L",
        );
        assert_points(
            &p[1].points,
            &[[h, -200.0], [200.0 - a, -200.0], [h, -b]],
            "T",
        );
        assert_points(
            &p[2].points,
            &[[a, 0.0], [200.0, -200.0 + a], [200.0, 200.0 - a]],
            "R",
        );
        assert_points(&p[3].points, &[[h, 200.0], [h, b], [200.0 - a, 200.0]], "B");
    }

    #[test]
    fn sign_scales_with_its_size_and_follows_its_center() {
        let center = [50.0, 30.0];
        let small = pieces(center, 100.0, 1.0);
        let reference = pieces([0.0, 0.0], 400.0, 1.0);
        for (name, (s, r)) in ["L", "T", "R", "B"]
            .iter()
            .zip(small.iter().zip(&reference))
        {
            let expected: Vec<[f32; 2]> = r
                .points
                .iter()
                .map(|q| [center[0] + q[0] * 0.25, center[1] + q[1] * 0.25])
                .collect();
            assert_points(&s.points, &expected, name);
        }
    }

    #[test]
    fn sign_stays_inside_its_square() {
        for (center, size) in [
            ([0.0, 0.0], 400.0),
            ([120.0, 80.0], 64.0),
            ([10.0, 10.0], 14.0),
            ([5.0, 5.0], 8.0),
        ] {
            let half = size * 0.5;
            for piece in pieces(center, size, 1.0) {
                for q in &piece.points {
                    assert!(
                        (q[0] - center[0]).abs() <= half + 1e-4
                            && (q[1] - center[1]).abs() <= half + 1e-4,
                        "{q:?} leaves the square of size {size} around {center:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn sign_vertical_cut_is_14_wide_at_size_400() {
        let p = pieces([0.0, 0.0], 400.0, 1.0);
        let left_edge = p[0].points[1][0];
        assert_near(p[1].points[0][0] - left_edge, 14.0, "cut beside T");
        assert_near(p[3].points[0][0] - left_edge, 14.0, "cut beside B");
    }

    #[test]
    fn sign_cut_stays_proportional_at_small_sizes() {
        for size in [14.0, 8.0] {
            let p = pieces([20.0, 20.0], size, 1.0);
            let left_edge = p[0].points[1][0];
            assert_near(
                p[1].points[0][0] - left_edge,
                cut_at(size),
                &format!("cut beside T at size {size}"),
            );
            assert_near(
                p[3].points[0][0] - left_edge,
                cut_at(size),
                &format!("cut beside B at size {size}"),
            );
        }
    }

    #[test]
    fn sign_diagonal_cuts_are_as_wide_as_the_vertical_cut() {
        for size in [400.0, 100.0, 14.0] {
            let p = pieces([0.0, 0.0], size, 1.0);
            let cut = cut_at(size);
            let (t, r, b) = (&p[1].points, &p[2].points, &p[3].points);
            // Upper diagonal: T's lower edge against R's upper edge.
            for (i, q) in [t[1], t[2]].into_iter().enumerate() {
                assert_near(
                    distance_to_line(q, r[0], r[1]),
                    cut,
                    &format!("upper diagonal, T vertex {i}, size {size}"),
                );
            }
            // Lower diagonal: B's upper edge against R's lower edge.
            for (i, q) in [b[1], b[2]].into_iter().enumerate() {
                assert_near(
                    distance_to_line(q, r[0], r[2]),
                    cut,
                    &format!("lower diagonal, B vertex {i}, size {size}"),
                );
            }
        }
    }

    #[test]
    fn sign_gradient_runs_from_the_top_left_to_the_bottom_right_corner() {
        for piece in pieces([50.0, 30.0], 100.0, 1.0) {
            assert_points(&[piece.from], &[[0.0, -20.0]], "gradient from");
            assert_points(&[piece.to], &[[100.0, 80.0]], "gradient to");
        }
    }

    #[test]
    fn sign_uses_the_silver_stops_at_full_opacity() {
        for piece in pieces([0.0, 0.0], 400.0, 1.0) {
            assert_eq!(piece.stops, SIGN_SILVER.to_vec());
        }
    }

    #[test]
    fn sign_opacity_scales_every_stop_alpha() {
        for piece in pieces([0.0, 0.0], 400.0, 0.5) {
            assert_eq!(piece.stops.len(), SIGN_SILVER.len());
            for (got, silver) in piece.stops.iter().zip(&SIGN_SILVER) {
                assert_eq!(got.offset, silver.offset);
                assert_eq!(got.color[..3], silver.color[..3], "rgb untouched");
                assert_near(got.color[3], 0.5, "alpha at half opacity");
            }
        }
    }

    #[test]
    fn sign_opacity_is_clamped_to_zero_and_one() {
        for piece in pieces([0.0, 0.0], 400.0, 2.0) {
            for stop in &piece.stops {
                assert_near(stop.color[3], 1.0, "alpha above full opacity");
            }
        }
        for piece in pieces([0.0, 0.0], 400.0, -1.0) {
            for stop in &piece.stops {
                assert_near(stop.color[3], 0.0, "alpha below zero opacity");
            }
        }
    }

    // ── The finish follows the theme ──────────────────

    /// A light-scheme theme that differs from the dark one only by its
    /// scheme, so the sign's finish is the one thing under test.
    fn light_scheme() -> UiTheme {
        UiTheme {
            scheme: khora_core::ui::ColorScheme::Light,
            ..khora_dark()
        }
    }

    fn pieces_on(theme: &UiTheme, opacity: f32) -> Vec<Piece> {
        let mut ui = RecordingUiBuilder::new(800.0, 800.0);
        sign(&mut ui, theme, [0.0, 0.0], 400.0, opacity);
        let pieces = gradient_paths(&ui);
        assert_eq!(pieces.len(), 4, "the sign is four gradient pieces");
        pieces
    }

    #[test]
    fn sign_on_a_light_theme_uses_the_navy_finish() {
        for piece in pieces_on(&light_scheme(), 1.0) {
            assert_eq!(piece.stops, SIGN_NAVY.to_vec());
        }
    }

    #[test]
    fn sign_on_a_light_theme_scales_the_navy_alpha_by_its_opacity() {
        for piece in pieces_on(&light_scheme(), 0.7) {
            assert_eq!(piece.stops.len(), SIGN_NAVY.len());
            for (got, navy) in piece.stops.iter().zip(&SIGN_NAVY) {
                assert_eq!(got.offset, navy.offset);
                assert_eq!(got.color[..3], navy.color[..3], "rgb untouched");
                assert_near(got.color[3], 0.7, "alpha at 70 % opacity");
            }
        }
    }

    #[test]
    fn sign_on_a_dark_theme_keeps_the_silver_finish() {
        for piece in pieces_on(&khora_dark(), 1.0) {
            assert_eq!(piece.stops, SIGN_SILVER.to_vec());
        }
    }

    /// The scheme changes the finish only — the cut square is the same shape
    /// on either field.
    #[test]
    fn sign_geometry_is_the_same_on_light_and_dark_themes() {
        let light = pieces_on(&light_scheme(), 1.0);
        let dark = pieces_on(&khora_dark(), 1.0);
        for (name, (l, d)) in ["L", "T", "R", "B"].iter().zip(light.iter().zip(&dark)) {
            assert_points(&l.points, &d.points, name);
            assert_points(&[l.from, l.to], &[d.from, d.to], name);
        }
    }

    #[test]
    fn brand_pill_on_a_light_theme_paints_a_navy_sign() {
        let mut ui = RecordingUiBuilder::new(300.0, 44.0);
        brand_pill(
            &mut ui,
            &light_scheme(),
            [0.0, 0.0, 280.0, 44.0],
            "nightfall.kscene",
        );
        let pieces = gradient_paths(&ui);
        assert_eq!(pieces.len(), 4, "the sign is four pieces");
        for piece in pieces {
            assert_eq!(piece.stops, SIGN_NAVY.to_vec());
        }
    }

    #[test]
    fn brand_pill_on_a_dark_theme_paints_a_silver_sign() {
        let mut ui = RecordingUiBuilder::new(300.0, 44.0);
        brand_pill(&mut ui, &khora_dark(), [0.0, 0.0, 280.0, 44.0], "");
        for piece in gradient_paths(&ui) {
            assert_eq!(piece.stops, SIGN_SILVER.to_vec());
        }
    }
}
