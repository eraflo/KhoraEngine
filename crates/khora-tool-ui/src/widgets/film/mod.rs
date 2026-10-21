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

//! The brand films — short looping animations painted frame by frame.
//!
//! A film is a pure painter: [`paint`] draws the frame at time `t` (seconds
//! from the start of the cycle) into a rect, through [`UiBuilder`] primitives
//! only. The composition is laid out on a 1440 × 900 stage, scaled uniformly
//! and centred in the rect; the space left over is letterboxed in the film's
//! own background.

use khora_core::ui::UiBuilder;

use super::{Color, Rect};

mod canvas;
mod door;
mod sign;

/// The films' backdrop, also the letterbox around the composition.
const BACKDROP: Color = canvas::rgb(0x0f141d);
/// The brand gold.
const GOLD: Color = canvas::rgb(0xe3b04b);
/// The light behind the door — "the editor's gold": its centre, middle and
/// edge.
const LIGHT: [Color; 3] = [
    canvas::rgb(0xf0cf86),
    canvas::rgb(0xe7be68),
    canvas::rgb(0xc99a45),
];

/// One of the brand films.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Film {
    /// "Le signe" — the sign is traced, cut along its K, and closes.
    Sign,
    /// "La porte" — a plate, a light, a door, and the breath through it.
    Door,
}

/// Chapters of [`Film::Sign`]: start time in seconds, label.
const SIGN_CHAPTERS: [(f32, &str); 8] = [
    (0.0, "Tracé"),
    (1.6, "Tracé d'or"),
    (3.4, "Le carré"),
    (5.0, "Le trou"),
    (5.7, "Les fentes"),
    (7.6, "Négociation"),
    (15.6, "La porte"),
    (18.5, "Fermeture"),
];

/// Chapters of [`Film::Door`]: start time in seconds, label.
const DOOR_CHAPTERS: [(f32, &str); 5] = [
    (0.0, "Plaque"),
    (3.9, "Lumière"),
    (11.3, "Porte"),
    (18.3, "Le souffle"),
    (27.2, "Fermeture"),
];

impl Film {
    /// Length of one full cycle, in seconds.
    pub fn cycle(self) -> f32 {
        match self {
            Film::Sign => 22.6,
            Film::Door => 35.6,
        }
    }

    /// The time, in seconds, of the frame that stands for the whole film when
    /// motion is reduced.
    pub fn still(self) -> f32 {
        match self {
            Film::Sign => 10.0,
            Film::Door => 17.3,
        }
    }

    /// The film's chapters, in order: start time in seconds and label.
    pub fn chapters(self) -> &'static [(f32, &'static str)] {
        match self {
            Film::Sign => &SIGN_CHAPTERS,
            Film::Door => &DOOR_CHAPTERS,
        }
    }

    /// The end of the chapter playing at `t`: the start of the next chapter,
    /// or [`Film::cycle`] for the last one. `t` is clamped into the cycle.
    pub fn chapter_end(self, t: f32) -> f32 {
        let t = t.clamp(0.0, self.cycle());
        self.chapters()
            .iter()
            .map(|&(start, _)| start)
            .find(|&start| start > t)
            .unwrap_or(self.cycle())
    }
}

/// Paints the frame of `film` at `t` seconds into `rect`.
///
/// `t` is clamped into the cycle. Everything painted stays inside `rect`.
pub fn paint(ui: &mut dyn UiBuilder, film: Film, rect: Rect, t: f32) {
    let t = if t.is_finite() {
        t.clamp(0.0, film.cycle())
    } else {
        0.0
    };
    paint_faded(ui, film, rect, t, 1.0);
}

/// Paints the frame of `film` at `t` seconds into `rect`, the whole frame —
/// backdrop included — at `opacity`.
///
/// `opacity` is clamped into 0..=1 (a non-finite one counts as 0); at 0
/// nothing visible is painted. `t` is clamped into the cycle, as by
/// [`paint`].
pub fn paint_faded(ui: &mut dyn UiBuilder, film: Film, rect: Rect, t: f32, opacity: f32) {
    let t = if t.is_finite() {
        t.clamp(0.0, film.cycle())
    } else {
        0.0
    };
    let opacity = if opacity.is_nan() {
        0.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    let mut c = canvas::Canvas::new(ui, rect, BACKDROP, opacity);
    match film {
        Film::Sign => sign::paint(&mut c, t),
        Film::Door => door::paint(&mut c, t),
    }
}

/// `v` clamped into 0..=1.
fn cl(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Cubic ease in-out over 0..=1.
fn ez(v: f32) -> f32 {
    let v = cl(v);
    if v < 0.5 {
        4.0 * v * v * v
    } else {
        1.0 - (-2.0 * v + 2.0).powi(3) / 2.0
    }
}

/// Ease out with exponent `k` over 0..=1.
fn eo(v: f32, k: f32) -> f32 {
    1.0 - (1.0 - cl(v)).powf(k)
}

/// The sign's four pieces in its ±200 space, `h` half the cut: left, top,
/// right, bottom.
fn sign_pieces(h: f32) -> [Vec<[f32; 2]>; 4] {
    let a = h * std::f32::consts::SQRT_2;
    let b = h + a;
    [
        vec![[-200.0, -200.0], [-h, -200.0], [-h, 200.0], [-200.0, 200.0]],
        vec![[h, -200.0], [200.0 - a, -200.0], [h, -b]],
        vec![[a, 0.0], [200.0, -200.0 + a], [200.0, 200.0 - a]],
        vec![[h, 200.0], [h, b], [200.0 - a, 200.0]],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{PaintCall, RecordingUiBuilder};
    use khora_core::ui::editor::ui_builder::FontFamilyHint;

    const FILMS: [Film; 2] = [Film::Sign, Film::Door];

    // ── Helpers ───────────────────────────────────────

    /// The frame of `film` at `t` painted into `rect`, as recorded calls.
    fn frame(film: Film, rect: Rect, t: f32) -> Vec<PaintCall> {
        let mut ui = RecordingUiBuilder::new(rect[0] + rect[2], rect[1] + rect[3]);
        paint(&mut ui, film, rect, t);
        ui.calls().to_vec()
    }

    /// `n` evenly spaced times from `from` to `to`, both included.
    fn times(from: f32, to: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| from + (to - from) * i as f32 / (n - 1) as f32)
            .collect()
    }

    /// Every painted point of a call: polygon vertices, line endpoints, circle
    /// centres, rect corners, text anchors. Gradient anchors are not painted
    /// geometry and are left out.
    fn painted_points(call: &PaintCall) -> Vec<[f32; 2]> {
        match call {
            PaintCall::RectFilled { min, size, .. } | PaintCall::RectStroke { min, size, .. } => {
                vec![*min, [min[0] + size[0], min[1] + size[1]]]
            }
            PaintCall::Line { from, to, .. } => vec![*from, *to],
            PaintCall::CircleFilled { center, .. } | PaintCall::CircleStroke { center, .. } => {
                vec![*center]
            }
            PaintCall::Text { pos, .. } => vec![*pos],
            PaintCall::Path { points, .. }
            | PaintCall::PathGradient { points, .. }
            | PaintCall::PathRadial { points, .. } => points.clone(),
            PaintCall::Interact { .. } => Vec::new(),
        }
    }

    /// Every other number a call carries that must be finite: gradient
    /// anchors, radii, thicknesses, text sizes, stop offsets.
    fn other_numbers(call: &PaintCall) -> Vec<f32> {
        match call {
            PaintCall::RectFilled { rounding, .. } => vec![*rounding],
            PaintCall::RectStroke {
                rounding,
                thickness,
                ..
            } => vec![*rounding, *thickness],
            PaintCall::Line { thickness, .. } => vec![*thickness],
            PaintCall::CircleFilled { radius, .. } => vec![*radius],
            PaintCall::CircleStroke {
                radius, thickness, ..
            } => vec![*radius, *thickness],
            PaintCall::Text { size, .. } => vec![*size],
            PaintCall::Path { .. } => Vec::new(),
            PaintCall::PathGradient {
                from, to, stops, ..
            } => {
                let mut v = vec![from[0], from[1], to[0], to[1]];
                v.extend(stops.iter().map(|s| s.offset));
                v
            }
            PaintCall::PathRadial {
                center,
                radius,
                stops,
                ..
            } => {
                let mut v = vec![center[0], center[1], *radius];
                v.extend(stops.iter().map(|s| s.offset));
                v
            }
            PaintCall::Interact { .. } => Vec::new(),
        }
    }

    /// Every colour a call paints with, gradient stops included.
    fn colours(call: &PaintCall) -> Vec<[f32; 4]> {
        match call {
            PaintCall::RectFilled { color, .. }
            | PaintCall::RectStroke { color, .. }
            | PaintCall::Line { color, .. }
            | PaintCall::CircleFilled { color, .. }
            | PaintCall::CircleStroke { color, .. }
            | PaintCall::Text { color, .. }
            | PaintCall::Path { color, .. } => vec![*color],
            PaintCall::PathGradient { stops, .. } | PaintCall::PathRadial { stops, .. } => {
                stops.iter().map(|s| s.color).collect()
            }
            PaintCall::Interact { .. } => Vec::new(),
        }
    }

    /// The most opaque alpha a call paints with.
    fn max_alpha(call: &PaintCall) -> f32 {
        colours(call).iter().map(|c| c[3]).fold(0.0, f32::max)
    }

    /// `true` if the call is a filled rect covering the whole of `rect`.
    fn covers(call: &PaintCall, rect: Rect) -> bool {
        match call {
            PaintCall::RectFilled { min, size, .. } => {
                (min[0] - rect[0]).abs() <= 0.5
                    && (min[1] - rect[1]).abs() <= 0.5
                    && (size[0] - rect[2]).abs() <= 0.5
                    && (size[1] - rect[3]).abs() <= 0.5
            }
            _ => false,
        }
    }

    /// The frame's calls with its full-rect backdrop removed. Panics if the
    /// frame paints no backdrop.
    fn without_backdrop(calls: &[PaintCall], rect: Rect, what: &str) -> Vec<PaintCall> {
        let at = calls
            .iter()
            .position(|c| covers(c, rect))
            .unwrap_or_else(|| panic!("{what}: no filled rect covers the whole rect {rect:?}"));
        calls
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != at)
            .map(|(_, c)| c.clone())
            .collect()
    }

    /// Asserts every number of every call is finite, every painted point lies
    /// inside `rect` grown by one pixel, and every colour channel is in `0..=1`.
    fn assert_well_formed(calls: &[PaintCall], rect: Rect, what: &str) {
        let (x0, y0) = (rect[0] - 1.0, rect[1] - 1.0);
        let (x1, y1) = (rect[0] + rect[2] + 1.0, rect[1] + rect[3] + 1.0);
        for call in calls {
            for p in painted_points(call) {
                assert!(
                    p[0].is_finite() && p[1].is_finite(),
                    "{what}: non-finite point {p:?} in {call:?}"
                );
                assert!(
                    p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1,
                    "{what}: point {p:?} leaves the rect {rect:?} in {call:?}"
                );
            }
            for n in other_numbers(call) {
                assert!(n.is_finite(), "{what}: non-finite number {n} in {call:?}");
            }
            if let PaintCall::CircleFilled { radius, .. }
            | PaintCall::CircleStroke { radius, .. }
            | PaintCall::PathRadial { radius, .. } = call
            {
                assert!(*radius >= 0.0, "{what}: negative radius in {call:?}");
            }
            if let PaintCall::Line { thickness, .. }
            | PaintCall::RectStroke { thickness, .. }
            | PaintCall::CircleStroke { thickness, .. } = call
            {
                assert!(*thickness >= 0.0, "{what}: negative thickness in {call:?}");
            }
            if let PaintCall::Text { size, .. } = call {
                assert!(*size > 0.0, "{what}: text size not positive in {call:?}");
            }
            if let PaintCall::PathGradient { stops, .. } | PaintCall::PathRadial { stops, .. } =
                call
            {
                for s in stops {
                    assert!(
                        (0.0..=1.0).contains(&s.offset),
                        "{what}: stop offset outside 0..=1 in {call:?}"
                    );
                }
            }
            for c in colours(call) {
                for ch in c {
                    assert!(
                        ch.is_finite() && (0.0..=1.0).contains(&ch),
                        "{what}: colour {c:?} outside 0..=1 in {call:?}"
                    );
                }
            }
        }
    }

    // ── Chapters ──────────────────────────────────────

    #[test]
    fn chapters_start_at_zero_strictly_increase_and_end_before_the_cycle() {
        for film in FILMS {
            let chapters = film.chapters();
            assert!(!chapters.is_empty(), "{film:?} has chapters");
            assert_eq!(chapters[0].0, 0.0, "{film:?} starts its first chapter at 0");
            for pair in chapters.windows(2) {
                assert!(
                    pair[1].0 > pair[0].0,
                    "{film:?}: {:?} does not start after {:?}",
                    pair[1],
                    pair[0]
                );
            }
            for (start, label) in chapters {
                assert!(
                    *start < film.cycle(),
                    "{film:?}: chapter {label} starts at {start}, past the cycle"
                );
                assert!(!label.is_empty(), "{film:?}: a chapter has no label");
            }
            assert!(
                film.still() > 0.0 && film.still() < film.cycle(),
                "{film:?}: the still frame lies inside the cycle"
            );
        }
    }

    #[test]
    fn films_have_the_board_timings() {
        assert_eq!(Film::Sign.cycle(), 22.6);
        assert_eq!(Film::Sign.still(), 10.0);
        assert_eq!(Film::Sign.chapters().len(), 8);
        assert_eq!(Film::Door.cycle(), 35.6);
        assert_eq!(Film::Door.still(), 17.3);
        assert_eq!(Film::Door.chapters().len(), 5);
    }

    #[test]
    fn chapter_end_is_the_next_chapter_start_inside_each_chapter() {
        for film in FILMS {
            let chapters = film.chapters();
            for (i, (start, label)) in chapters.iter().enumerate() {
                let end = chapters.get(i + 1).map_or(film.cycle(), |c| c.0);
                for t in [*start, (*start + end) * 0.5, end - 0.01] {
                    assert!(
                        (film.chapter_end(t) - end).abs() < 1e-5,
                        "{film:?}: at t = {t} (chapter {label}) the chapter ends at {end}, got {}",
                        film.chapter_end(t)
                    );
                }
            }
        }
    }

    #[test]
    fn chapter_end_in_the_last_chapter_is_the_cycle_end() {
        for film in FILMS {
            let last = film.chapters().last().unwrap().0;
            for t in [last, (last + film.cycle()) * 0.5, film.cycle()] {
                assert!(
                    (film.chapter_end(t) - film.cycle()).abs() < 1e-5,
                    "{film:?}: at t = {t} the last chapter ends with the cycle"
                );
            }
        }
    }

    #[test]
    fn chapter_end_is_monotone_and_never_before_t() {
        for film in FILMS {
            let mut previous = f32::NEG_INFINITY;
            for t in times(0.0, film.cycle(), 1000) {
                let end = film.chapter_end(t);
                assert!(end.is_finite(), "{film:?}: chapter_end({t}) is finite");
                assert!(end >= t, "{film:?}: chapter_end({t}) = {end} is before t");
                assert!(
                    end >= previous,
                    "{film:?}: chapter_end({t}) = {end} went back from {previous}"
                );
                previous = end;
            }
        }
    }

    #[test]
    fn chapter_end_clamps_t_into_the_cycle() {
        for film in FILMS {
            let first_end = film.chapters()[1].0;
            for t in [-0.01, -5.0, -1000.0] {
                assert!(
                    (film.chapter_end(t) - first_end).abs() < 1e-5,
                    "{film:?}: before the cycle, t = {t} belongs to the first chapter"
                );
            }
            for t in [film.cycle() + 0.01, film.cycle() + 5.0, 1.0e6] {
                assert!(
                    (film.chapter_end(t) - film.cycle()).abs() < 1e-5,
                    "{film:?}: past the cycle, t = {t} ends with the cycle"
                );
            }
        }
    }

    // ── Painting stays well-formed ────────────────────

    #[test]
    fn every_frame_paints_finite_geometry_inside_the_rect_and_colours_in_range() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            for t in times(-1.0, film.cycle() + 1.0, 200) {
                let calls = frame(film, rect, t);
                assert!(!calls.is_empty(), "{film:?} at t = {t} paints something");
                assert_well_formed(&calls, rect, &format!("{film:?} at t = {t}"));
            }
        }
    }

    #[test]
    fn a_tall_rect_and_a_wide_rect_contain_every_frame() {
        for rect in [[0.0, 0.0, 300.0, 900.0], [0.0, 0.0, 1600.0, 400.0]] {
            for film in FILMS {
                for t in times(-1.0, film.cycle() + 1.0, 200) {
                    let calls = frame(film, rect, t);
                    assert_well_formed(&calls, rect, &format!("{film:?} at t = {t} in {rect:?}"));
                }
            }
        }
    }

    /// The outermost points a call covers: a stroke's four corners (its
    /// width included), a disc's centre pushed out by its radius along both
    /// axes, a polygon's vertices.
    fn covered_points(call: &PaintCall) -> Vec<[f32; 2]> {
        match call {
            PaintCall::Line {
                from,
                to,
                thickness,
                ..
            } => {
                let d = [to[0] - from[0], to[1] - from[1]];
                let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
                let n = [-d[1] / len * thickness * 0.5, d[0] / len * thickness * 0.5];
                vec![
                    [from[0] + n[0], from[1] + n[1]],
                    [from[0] - n[0], from[1] - n[1]],
                    [to[0] + n[0], to[1] + n[1]],
                    [to[0] - n[0], to[1] - n[1]],
                ]
            }
            PaintCall::CircleFilled { center, radius, .. } => vec![
                [center[0] - radius, center[1]],
                [center[0] + radius, center[1]],
                [center[0], center[1] - radius],
                [center[0], center[1] + radius],
            ],
            other => painted_points(other),
        }
    }

    #[test]
    fn a_thick_stroke_drawn_inside_a_clip_region_stays_inside_it() {
        // What the door film does with the figure in the doorway: limbs are
        // strokes tens of pixels wide, clipped to the doorway. A stroke running
        // along the region's edge, and one leaving it at a slant, must not
        // paint past that edge by half their width.
        let rect = [0.0, 0.0, 1440.0, 900.0];
        let region = [
            [100.0, 100.0],
            [300.0, 100.0],
            [300.0, 300.0],
            [100.0, 300.0],
        ];
        let mut ui = RecordingUiBuilder::new(rect[2], rect[3]);
        {
            let mut c = canvas::Canvas::new(&mut ui, rect, BACKDROP, 1.0);
            c.clipped(&region, |c| {
                c.line([290.0, 150.0], [290.0, 250.0], 40.0, GOLD, false);
                c.line([200.0, 200.0], [400.0, 260.0], 40.0, GOLD, true);
            });
        }
        let painted = without_backdrop(ui.calls(), rect, "strokes in a clip");
        assert!(!painted.is_empty(), "the strokes are painted");
        for call in &painted {
            for p in covered_points(call) {
                assert!(
                    (99.5..=300.5).contains(&p[0]) && (99.5..=300.5).contains(&p[1]),
                    "{call:?} covers {p:?}, outside the clip region 100..300"
                );
            }
        }
    }

    #[test]
    fn every_frame_starts_with_the_film_backdrop_over_the_whole_rect() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        // `#0f141d`, opaque.
        let backdrop = [15.0 / 255.0, 20.0 / 255.0, 29.0 / 255.0, 1.0];
        for film in FILMS {
            for t in [0.0, film.still(), film.cycle() - 0.05] {
                let calls = frame(film, rect, t);
                let first = calls
                    .iter()
                    .find(|c| covers(c, rect))
                    .unwrap_or_else(|| panic!("{film:?} at t = {t}: no backdrop over the rect"));
                let PaintCall::RectFilled { color, .. } = first else {
                    unreachable!()
                };
                for (got, want) in color.iter().zip(backdrop) {
                    assert!(
                        (got - want).abs() < 1.0 / 255.0,
                        "{film:?} at t = {t}: backdrop {color:?}, expected #0f141d"
                    );
                }
            }
        }
    }

    // ── The sign at rest ──────────────────────────────

    /// A polygon the film painted, with the most opaque alpha it used.
    struct Polygon {
        points: Vec<[f32; 2]>,
        alpha: f32,
    }

    fn polygons(calls: &[PaintCall]) -> Vec<Polygon> {
        calls
            .iter()
            .filter_map(|c| match c {
                PaintCall::Path { points, .. }
                | PaintCall::PathGradient { points, .. }
                | PaintCall::PathRadial { points, .. } => Some(Polygon {
                    points: points.clone(),
                    alpha: max_alpha(c),
                }),
                _ => None,
            })
            .collect()
    }

    fn span(points: &[[f32; 2]], axis: usize) -> (f32, f32) {
        points
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[axis]), hi.max(p[axis]))
            })
    }

    /// The x shared by the two vertices of a triangle lying on its left (or
    /// right) vertical edge, if it has one.
    fn vertical_edge(points: &[[f32; 2]], side_left: bool, tol: f32) -> Option<f32> {
        let (lo, hi) = span(points, 0);
        let x = if side_left { lo } else { hi };
        let on_edge = points.iter().filter(|p| (p[0] - x).abs() <= tol).count();
        (on_edge == 2).then_some(x)
    }

    #[test]
    fn the_sign_at_rest_is_four_pieces_cut_by_a_k_gap_of_14_on_400() {
        // Just after the cuts are carved (7.5 s) and before the pieces start
        // to negotiate (7.6 s): the only moment the gap is exactly the cut —
        // during the negotiation it breathes and the pieces sway in pairs.
        let rect = [0.0, 0.0, 1440.0, 900.0];
        let calls = frame(Film::Sign, rect, 7.55);
        let visible: Vec<Polygon> = polygons(&calls)
            .into_iter()
            .filter(|p| p.alpha > 0.5)
            .collect();

        // L: an axis-aligned rectangle a little under half as wide as tall.
        let mut found = None;
        for l in visible.iter().filter(|p| p.points.len() == 4) {
            let (lx0, lx1) = span(&l.points, 0);
            let (ly0, ly1) = span(&l.points, 1);
            let side = ly1 - ly0;
            if side < 20.0 {
                continue;
            }
            let ratio = (lx1 - lx0) / side;
            let tol = side * 0.005;
            let axis_aligned = l.points.iter().all(|p| {
                ((p[0] - lx0).abs() <= tol || (p[0] - lx1).abs() <= tol)
                    && ((p[1] - ly0).abs() <= tol || (p[1] - ly1).abs() <= tol)
            });
            if !axis_aligned || !(0.45..=0.5).contains(&ratio) {
                continue;
            }
            // T: a triangle on the square's top edge, its left edge vertical
            // just right of L.
            let t = visible.iter().filter(|p| p.points.len() == 3).find(|p| {
                let (ty0, ty1) = span(&p.points, 1);
                vertical_edge(&p.points, true, tol).is_some_and(|x| x > lx1 && x - lx1 < side * 0.1)
                    && (ty0 - ly0).abs() <= tol
                    && ty1 < ly0 + side * 0.5
            });
            // B: the same against the bottom edge.
            let b = visible.iter().filter(|p| p.points.len() == 3).find(|p| {
                let (ty0, ty1) = span(&p.points, 1);
                vertical_edge(&p.points, true, tol).is_some_and(|x| x > lx1 && x - lx1 < side * 0.1)
                    && (ty1 - ly1).abs() <= tol
                    && ty0 > ly0 + side * 0.5
            });
            // R: a triangle whose vertical edge is the square's right side.
            let r = visible.iter().filter(|p| p.points.len() == 3).find(|p| {
                vertical_edge(&p.points, false, tol)
                    .is_some_and(|x| (x - (lx0 + side)).abs() <= tol)
            });
            if let (Some(t), Some(_), Some(_)) = (t, r, b) {
                let tx = vertical_edge(&t.points, true, tol).unwrap();
                found = Some((side, tx - lx1));
                break;
            }
        }

        let (side, gap) = found
            .expect("at the still frame the sign is painted as its four opaque pieces L, T, R, B");
        let expected = side * 14.0 / 400.0;
        assert!(
            (gap - expected).abs() <= expected * 0.02,
            "the K gap between L and T is {gap}, expected {expected} (14/400 of {side})"
        );
    }

    // ── Fades ─────────────────────────────────────────

    #[test]
    fn both_films_have_faded_out_just_before_the_cycle_ends() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            let t = film.cycle() - 0.05;
            let calls = frame(film, rect, t);
            for call in without_backdrop(&calls, rect, &format!("{film:?} at t = {t}")) {
                assert!(
                    max_alpha(&call) <= 0.02,
                    "{film:?} at t = {t}: {call:?} is still visible"
                );
            }
        }
    }

    #[test]
    fn the_door_film_opens_on_nothing_but_its_backdrop() {
        // Only the plate's back edge, a hairline at 18 %, is there from the
        // first frame, as on the board.
        let rect = [100.0, 50.0, 800.0, 500.0];
        let calls = frame(Film::Door, rect, 0.0);
        for call in without_backdrop(&calls, rect, "Door at t = 0") {
            // Strokes are painted as filled outlines.
            let back_edge = matches!(call, PaintCall::Path { .. });
            let limit = if back_edge { 0.18 + 1e-3 } else { 0.02 };
            assert!(
                max_alpha(&call) <= limit,
                "Door at t = 0: {call:?} is visible before the grid fades in"
            );
        }
    }

    // ── Painting faded ────────────────────────────────

    /// The frame of `film` at `t` painted into `rect` at `opacity`.
    fn faded_frame(film: Film, rect: Rect, t: f32, opacity: f32) -> Vec<PaintCall> {
        let mut ui = RecordingUiBuilder::new(rect[0] + rect[2], rect[1] + rect[3]);
        paint_faded(&mut ui, film, rect, t, opacity);
        ui.calls().to_vec()
    }

    /// `call` with every alpha it paints with set to 0: what is left is its
    /// geometry and its hues.
    fn without_alpha(call: &PaintCall) -> PaintCall {
        let mut call = call.clone();
        match &mut call {
            PaintCall::RectFilled { color, .. }
            | PaintCall::RectStroke { color, .. }
            | PaintCall::Line { color, .. }
            | PaintCall::CircleFilled { color, .. }
            | PaintCall::CircleStroke { color, .. }
            | PaintCall::Text { color, .. }
            | PaintCall::Path { color, .. } => color[3] = 0.0,
            PaintCall::PathGradient { stops, .. } | PaintCall::PathRadial { stops, .. } => {
                for s in stops {
                    s.color[3] = 0.0;
                }
            }
            PaintCall::Interact { .. } => {}
        }
        call
    }

    /// The alpha of the filled rect covering the whole of `rect`.
    fn backdrop_alpha(calls: &[PaintCall], rect: Rect, what: &str) -> f32 {
        match calls.iter().find(|c| covers(c, rect)) {
            Some(PaintCall::RectFilled { color, .. }) => color[3],
            _ => panic!("{what}: no filled rect covers the whole rect {rect:?}"),
        }
    }

    #[test]
    fn half_opacity_halves_the_backdrop_and_never_more_than_halves_any_alpha() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            for t in [0.5, 2.0, 7.55, film.still(), film.cycle() - 2.0] {
                let what = format!("{film:?} at t = {t}");
                let full = faded_frame(film, rect, t, 1.0);
                let half = faded_frame(film, rect, t, 0.5);
                assert_eq!(
                    half.len(),
                    full.len(),
                    "{what}: fading paints the same calls"
                );
                assert!(
                    (backdrop_alpha(&half, rect, &what) - 0.5).abs() < 1e-5,
                    "{what}: the backdrop is painted at half opacity"
                );
                for (h, f) in half.iter().zip(&full) {
                    assert_eq!(
                        without_alpha(h),
                        without_alpha(f),
                        "{what}: fading changes alphas only"
                    );
                    for (ch, cf) in colours(h).iter().zip(colours(f)) {
                        assert!(
                            ch[3] <= cf[3] * 0.5 + 1e-5,
                            "{what}: alpha {} at half opacity, {} at full, in {h:?}",
                            ch[3],
                            cf[3]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn full_opacity_is_the_plain_frame() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            for t in [0.5, film.still(), film.cycle() - 2.0] {
                assert_eq!(
                    faded_frame(film, rect, t, 1.0),
                    frame(film, rect, t),
                    "{film:?} at t = {t}"
                );
            }
        }
    }

    #[test]
    fn zero_opacity_paints_nothing_visible() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            for t in [0.5, 7.55, film.still()] {
                for call in faded_frame(film, rect, t, 0.0) {
                    assert!(
                        max_alpha(&call) <= 0.0,
                        "{film:?} at t = {t}: {call:?} is visible at opacity 0"
                    );
                }
            }
        }
    }

    #[test]
    fn opacity_above_one_paints_as_one() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            let t = film.still();
            assert_eq!(
                faded_frame(film, rect, t, 2.0),
                frame(film, rect, t),
                "{film:?}: opacity 2 is opacity 1"
            );
        }
    }

    #[test]
    fn a_nan_or_negative_opacity_paints_nothing_visible() {
        let rect = [100.0, 50.0, 800.0, 500.0];
        for film in FILMS {
            let t = film.still();
            for opacity in [f32::NAN, -1.0, f32::NEG_INFINITY] {
                for call in faded_frame(film, rect, t, opacity) {
                    for c in colours(&call) {
                        assert!(
                            c[3].is_finite() && c[3] <= 0.0,
                            "{film:?}: opacity {opacity} paints {call:?}"
                        );
                    }
                }
            }
        }
    }

    // ── The wordmark ──────────────────────────────────

    #[test]
    fn the_sign_film_sets_khora_in_the_wordmark_face_and_greek_in_monospace() {
        let rect = [0.0, 0.0, 1440.0, 900.0];
        let calls = frame(Film::Sign, rect, 10.0);
        let texts: Vec<(&str, FontFamilyHint, f32)> = calls
            .iter()
            .filter_map(|c| match c {
                PaintCall::Text {
                    text,
                    family,
                    color,
                    ..
                } => Some((text.as_str(), *family, color[3])),
                _ => None,
            })
            .collect();

        let wordmark: String = texts
            .iter()
            .filter(|(_, family, alpha)| *family == FontFamilyHint::Wordmark && *alpha > 0.5)
            .map(|(text, _, _)| *text)
            .collect();
        for letter in ['K', 'H', 'O', 'R', 'A'] {
            assert!(
                wordmark.contains(letter),
                "the visible wordmark-face text {wordmark:?} lacks {letter}"
            );
        }

        // Χ Ω Ρ Α — Greek capitals, not their Latin look-alikes.
        let greek = ['\u{03A7}', '\u{03A9}', '\u{03A1}', '\u{0391}'];
        let greek_runs: Vec<_> = texts
            .iter()
            .filter(|(text, _, _)| text.chars().any(|c| greek.contains(&c)))
            .collect();
        assert!(
            !greek_runs.is_empty(),
            "the Greek name is painted at t = 10"
        );
        for (text, family, _) in greek_runs {
            assert_eq!(
                *family,
                FontFamilyHint::Monospace,
                "the Greek glyphs {text:?} are set in monospace"
            );
        }
    }
}
