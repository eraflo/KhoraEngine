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

//! The loading screen — a brand film covering a tool while it starts.
//!
//! The film loops until the app reports it is ready; it then plays on to the
//! end of the chapter under way, fades out over [`LoadingScreen::FADE`]
//! seconds, and is done. Once the app is ready, a click on it or Esc skips
//! straight to the fade; before, they do nothing.
//! With reduced motion the film holds its still frame and fades as soon as
//! the app is ready.
//!
//! Times are absolute seconds on whatever clock the caller keeps; only their
//! differences matter.

use khora_core::platform::input::KeyCode;
use khora_core::ui::UiBuilder;

use super::film::{self, Film};
use super::Rect;

/// Where a [`LoadingScreen`] is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingState {
    /// The film plays at full opacity.
    Playing,
    /// The film fades out.
    Fading,
    /// The fade is over: the screen paints nothing.
    Done,
}

/// A brand film shown over a tool while it starts, until the tool is ready.
#[derive(Debug, Clone)]
pub struct LoadingScreen {
    /// The film shown.
    film: Film,
    /// When the film started, absolute seconds.
    start: f32,
    /// Hold the still frame instead of playing.
    reduced_motion: bool,
    /// When the fade starts, absolute seconds, once known.
    end: Option<f32>,
}

impl LoadingScreen {
    /// How long the fade out lasts, in seconds.
    pub const FADE: f32 = 0.4;

    /// A loading screen starting `film` at `now`; with `reduced_motion` it
    /// holds the film's still frame.
    pub fn new(film: Film, now: f32, reduced_motion: bool) -> Self {
        Self {
            film,
            start: now,
            reduced_motion,
            end: None,
        }
    }

    /// The film's own time at `now`: looping since the start, or the still
    /// frame with reduced motion.
    fn film_time(&self, now: f32) -> f32 {
        if self.reduced_motion {
            self.film.still()
        } else {
            (now - self.start).rem_euclid(self.film.cycle())
        }
    }

    /// The app is ready: play on to the end of the current chapter, then fade.
    ///
    /// Idempotent (the first call fixes the end); with reduced motion the
    /// fade starts at `now`.
    pub fn ready(&mut self, now: f32) {
        if self.end.is_some() {
            return;
        }
        self.end = Some(if self.reduced_motion {
            now
        } else {
            let t = self.film_time(now);
            now + (self.film.chapter_end(t) - t)
        });
    }

    /// Skip, once the app is ready: fade from `now` (if not already fading or
    /// done). Before [`ready`](Self::ready) it does nothing.
    pub fn skip(&mut self, now: f32) {
        // Before the app is ready a skip could only take the film away from
        // a tool that is not there yet, so it does nothing.
        match self.end {
            Some(end) if end > now => self.end = Some(now),
            _ => {}
        }
    }

    /// Where the screen is at `now`.
    pub fn state(&self, now: f32) -> LoadingState {
        match self.end {
            Some(end) if now >= end + Self::FADE => LoadingState::Done,
            Some(end) if now >= end => LoadingState::Fading,
            _ => LoadingState::Playing,
        }
    }

    /// Paints the film over `rect` at full opacity while
    /// [`Playing`](LoadingState::Playing), fading out over [`Self::FADE`] once
    /// the end is reached, nothing once [`Done`](LoadingState::Done). A click
    /// on the rect or Esc calls [`Self::skip`].
    ///
    /// Returns the state after painting.
    pub fn paint(&mut self, ui: &mut dyn UiBuilder, rect: Rect, now: f32) -> LoadingState {
        if self.state(now) == LoadingState::Done {
            return LoadingState::Done;
        }
        let opacity = match self.end {
            Some(end) if now >= end => 1.0 - (now - end) / Self::FADE,
            _ => 1.0,
        };
        film::paint_faded(ui, self.film, rect, self.film_time(now), opacity);
        // The screen covers the tool: a click anywhere on it, or Esc, skips.
        let clicked = ui.interact_rect("khora.loading_screen", rect).clicked;
        if clicked || ui.key_pressed(KeyCode::Escape) {
            self.skip(now);
        }
        self.state(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{PaintCall, RecordingUiBuilder};
    use crate::widgets::film;
    use khora_core::platform::KeyCode;

    const RECT: Rect = [0.0, 0.0, 1440.0, 900.0];

    // ── Helpers ───────────────────────────────────────

    fn recorder() -> RecordingUiBuilder {
        RecordingUiBuilder::new(RECT[2], RECT[3])
    }

    /// Asserts the screen is in `want` at each of `times`.
    fn assert_states(screen: &LoadingScreen, times: &[f32], want: LoadingState) {
        for &now in times {
            assert_eq!(screen.state(now), want, "at now = {now}");
        }
    }

    /// The calls a paint made, its interaction regions left out.
    fn painted(calls: &[PaintCall]) -> Vec<PaintCall> {
        calls
            .iter()
            .filter(|c| !matches!(c, PaintCall::Interact { .. }))
            .cloned()
            .collect()
    }

    /// The film's own frame at `t` over [`RECT`], as recorded calls.
    fn film_frame(film: Film, t: f32) -> Vec<PaintCall> {
        let mut ui = recorder();
        film::paint(&mut ui, film, RECT, t);
        ui.calls().to_vec()
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

    /// Every number a call carries, in a fixed order, and what else
    /// identifies it (its kind, its text, its font).
    fn numbers(call: &PaintCall) -> (String, Vec<f32>) {
        let stop_numbers = |stops: &[khora_core::ui::editor::GradientStop]| {
            stops
                .iter()
                .flat_map(|s| std::iter::once(s.offset).chain(s.color))
                .collect::<Vec<f32>>()
        };
        match call {
            PaintCall::RectFilled {
                min,
                size,
                color,
                rounding,
            } => (
                "rect".into(),
                [min.as_slice(), size, color, &[*rounding]].concat(),
            ),
            PaintCall::RectStroke {
                min,
                size,
                color,
                rounding,
                thickness,
            } => (
                "rect stroke".into(),
                [min.as_slice(), size, color, &[*rounding, *thickness]].concat(),
            ),
            PaintCall::Line {
                from,
                to,
                color,
                thickness,
            } => (
                "line".into(),
                [from.as_slice(), to, color, &[*thickness]].concat(),
            ),
            PaintCall::CircleFilled {
                center,
                radius,
                color,
            } => (
                "circle".into(),
                [center.as_slice(), &[*radius], color].concat(),
            ),
            PaintCall::CircleStroke {
                center,
                radius,
                color,
                thickness,
            } => (
                "circle stroke".into(),
                [center.as_slice(), &[*radius, *thickness], color].concat(),
            ),
            PaintCall::Text {
                pos,
                text,
                size,
                color,
                family,
                align,
            } => (
                format!("text {text:?} {family:?} {align:?}"),
                [pos.as_slice(), &[*size], color].concat(),
            ),
            PaintCall::Path { points, color } => (
                "path".into(),
                points
                    .iter()
                    .flatten()
                    .copied()
                    .chain(color.iter().copied())
                    .collect(),
            ),
            PaintCall::PathGradient {
                points,
                from,
                to,
                stops,
            } => (
                "linear".into(),
                points
                    .iter()
                    .flatten()
                    .copied()
                    .chain(from.iter().copied())
                    .chain(to.iter().copied())
                    .chain(stop_numbers(stops))
                    .collect(),
            ),
            PaintCall::PathRadial {
                points,
                center,
                radius,
                stops,
            } => (
                "radial".into(),
                points
                    .iter()
                    .flatten()
                    .copied()
                    .chain(center.iter().copied())
                    .chain(std::iter::once(*radius))
                    .chain(stop_numbers(stops))
                    .collect(),
            ),
            PaintCall::Interact { id_salt, rect } => (format!("interact {id_salt}"), rect.to_vec()),
        }
    }

    /// Asserts two frames are the same calls, their numbers equal within
    /// `tol` (absolute, plus relative for large coordinates).
    fn assert_same_frame(got: &[PaintCall], want: &[PaintCall], tol: f32, what: &str) {
        assert_eq!(got.len(), want.len(), "{what}: number of calls");
        for (g, w) in got.iter().zip(want) {
            let (gk, gn) = numbers(g);
            let (wk, wn) = numbers(w);
            assert_eq!(gk, wk, "{what}: {g:?} where {w:?} was expected");
            assert_eq!(gn.len(), wn.len(), "{what}: {g:?} where {w:?} was expected");
            for (a, b) in gn.iter().zip(&wn) {
                assert!(
                    (a - b).abs() <= tol * (1.0 + b.abs()),
                    "{what}: {g:?} where {w:?} was expected"
                );
            }
        }
    }

    /// The alpha of the filled rect covering the whole of [`RECT`].
    fn backdrop_alpha(calls: &[PaintCall]) -> f32 {
        calls
            .iter()
            .find_map(|c| match c {
                PaintCall::RectFilled {
                    min, size, color, ..
                } if min[0] <= RECT[0] + 0.5
                    && min[1] <= RECT[1] + 0.5
                    && size[0] >= RECT[2] - 0.5
                    && size[1] >= RECT[3] - 0.5 =>
                {
                    Some(color[3])
                }
                _ => None,
            })
            .expect("a filled rect covers the whole screen")
    }

    // ── Before ready ──────────────────────────────────

    #[test]
    fn the_film_plays_until_the_app_is_ready() {
        let screen = LoadingScreen::new(Film::Sign, 100.0, false);
        assert_states(
            &screen,
            &[100.0, 101.0, 110.0, 100.0 + 22.6 + 1.0, 200.0, 1000.0],
            LoadingState::Playing,
        );
    }

    #[test]
    fn a_reduced_motion_screen_also_waits_for_the_app() {
        let screen = LoadingScreen::new(Film::Sign, 100.0, true);
        assert_states(&screen, &[100.0, 105.0, 500.0], LoadingState::Playing);
    }

    // ── Ready ─────────────────────────────────────────

    #[test]
    fn ready_plays_to_the_chapter_end_then_fades_for_fade_then_is_done() {
        // At film time 0.5 the first chapter is under way; it ends at 1.6.
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(100.5);
        assert_states(&screen, &[100.5, 101.0, 101.59], LoadingState::Playing);
        assert_states(&screen, &[101.61, 101.8, 101.99], LoadingState::Fading);
        assert_states(&screen, &[102.01, 103.0, 500.0], LoadingState::Done);
    }

    #[test]
    fn ready_in_a_later_chapter_plays_to_that_chapter_end() {
        // Film time 4.0 lies in "Le carré", 3.4 to 5.0.
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        screen.ready(4.0);
        assert_states(&screen, &[4.0, 4.99], LoadingState::Playing);
        assert_states(&screen, &[5.01, 5.39], LoadingState::Fading);
        assert_states(&screen, &[5.41], LoadingState::Done);
    }

    #[test]
    fn ready_twice_keeps_the_first_end() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(100.5);
        // Film time 1.0: still the first chapter, but a second call must not
        // move anything — nor must one made once the chapter is over.
        screen.ready(101.0);
        screen.ready(101.7);
        assert_states(&screen, &[101.59], LoadingState::Playing);
        assert_states(&screen, &[101.61, 101.99], LoadingState::Fading);
        assert_states(&screen, &[102.01], LoadingState::Done);
    }

    #[test]
    fn ready_after_the_film_looped_uses_the_looped_film_time() {
        let start = 100.0;
        let cycle = Film::Sign.cycle();
        let mut screen = LoadingScreen::new(Film::Sign, start, false);
        // Second time round, film time 0.5: the first chapter, ending at 1.6.
        screen.ready(start + cycle + 0.5);
        assert_states(&screen, &[start + cycle + 1.59], LoadingState::Playing);
        assert_states(
            &screen,
            &[start + cycle + 1.61, start + cycle + 1.99],
            LoadingState::Fading,
        );
        assert_states(&screen, &[start + cycle + 2.01], LoadingState::Done);
    }

    #[test]
    fn ready_on_the_third_loop_ends_with_the_chapter_of_the_looped_time() {
        let cycle = Film::Sign.cycle();
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        // Third time round, film time 4.0: "Le carré", ending at 5.0.
        let lap = 2.0 * cycle;
        screen.ready(lap + 4.0);
        assert_states(&screen, &[lap + 4.98], LoadingState::Playing);
        assert_states(&screen, &[lap + 5.02, lap + 5.38], LoadingState::Fading);
        assert_states(&screen, &[lap + 5.42], LoadingState::Done);
    }

    // ── Skip ──────────────────────────────────────────

    #[test]
    fn skip_once_ready_fades_at_once_and_is_done_after_fade() {
        // Ready at film time 3.0: the chapter would play on to 3.4.
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(103.0);
        screen.skip(103.0);
        assert_states(&screen, &[103.0, 103.2, 103.39], LoadingState::Fading);
        assert_states(&screen, &[103.41, 110.0], LoadingState::Done);
    }

    #[test]
    fn skip_before_ready_leaves_the_film_playing() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.skip(103.0);
        assert_states(
            &screen,
            &[103.0, 103.2, 103.41, 110.0, 500.0],
            LoadingState::Playing,
        );
    }

    #[test]
    fn ready_after_an_ignored_skip_still_plays_to_the_chapter_end() {
        // The skip at film time 0.2 is ignored; ready at 0.3 plays the first
        // chapter on to 1.6.
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.skip(100.2);
        screen.ready(100.3);
        assert_states(&screen, &[100.3, 100.61, 101.59], LoadingState::Playing);
        assert_states(&screen, &[101.61, 101.99], LoadingState::Fading);
        assert_states(&screen, &[102.01], LoadingState::Done);
    }

    #[test]
    fn skip_before_ready_with_reduced_motion_leaves_the_still_frame_up() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, true);
        screen.skip(101.0);
        assert_states(&screen, &[101.0, 101.5, 200.0], LoadingState::Playing);
    }

    #[test]
    fn skip_cuts_a_chapter_the_app_is_waiting_on_short() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(100.5);
        screen.skip(101.0);
        assert_states(&screen, &[101.0, 101.39], LoadingState::Fading);
        assert_states(&screen, &[101.41], LoadingState::Done);
    }

    #[test]
    fn skip_while_fading_keeps_the_fade_running() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(103.0);
        screen.skip(103.0);
        screen.skip(103.3);
        assert_states(&screen, &[103.41], LoadingState::Done);
    }

    #[test]
    fn skip_after_done_stays_done() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(103.0);
        screen.skip(103.0);
        screen.skip(104.0);
        assert_states(&screen, &[104.0, 104.2, 105.0], LoadingState::Done);
    }

    #[test]
    fn ready_again_after_a_skip_does_not_postpone_the_fade() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, false);
        screen.ready(100.2);
        screen.skip(100.2);
        screen.ready(100.3);
        assert_states(&screen, &[100.61], LoadingState::Done);
    }

    // ── Reduced motion ────────────────────────────────

    #[test]
    fn reduced_motion_paints_the_still_frame() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, true);
        for now in [100.0, 103.7, 140.0] {
            let mut ui = recorder();
            assert_eq!(screen.paint(&mut ui, RECT, now), LoadingState::Playing);
            assert_eq!(
                painted(ui.calls()),
                film_frame(Film::Sign, Film::Sign.still()),
                "at now = {now} the still frame is painted"
            );
        }
    }

    #[test]
    fn reduced_motion_fades_as_soon_as_the_app_is_ready() {
        let mut screen = LoadingScreen::new(Film::Sign, 100.0, true);
        screen.ready(100.5);
        assert_states(&screen, &[100.5, 100.89], LoadingState::Fading);
        assert_states(&screen, &[100.91], LoadingState::Done);
    }

    // ── Painting ──────────────────────────────────────

    #[test]
    fn playing_paints_the_film_at_its_time_since_the_start() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        let mut ui = recorder();
        assert_eq!(screen.paint(&mut ui, RECT, 7.5), LoadingState::Playing);
        assert_eq!(painted(ui.calls()), film_frame(Film::Sign, 7.5));
        assert_eq!(backdrop_alpha(ui.calls()), 1.0);
    }

    #[test]
    fn playing_loops_the_film() {
        let cycle = Film::Sign.cycle();
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        let mut ui = recorder();
        assert_eq!(
            screen.paint(&mut ui, RECT, cycle + 7.5),
            LoadingState::Playing
        );
        assert_same_frame(
            &painted(ui.calls()),
            &film_frame(Film::Sign, 7.5),
            1e-3,
            "one cycle on, the frame at 7.5",
        );
    }

    #[test]
    fn while_fading_every_alpha_scales_down_with_the_fade() {
        // Ready at film time 0.5: the fade runs from 1.6 to 2.0.
        for (now, progress) in [(1.7, 0.25), (1.8, 0.5), (1.9, 0.75)] {
            let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
            screen.ready(0.5);
            let mut ui = recorder();
            assert_eq!(screen.paint(&mut ui, RECT, now), LoadingState::Fading);
            let calls = painted(ui.calls());
            let opacity = 1.0 - progress;
            assert!(
                (backdrop_alpha(&calls) - opacity).abs() < 1e-3,
                "{progress} of the way through the fade the backdrop alpha is {opacity}, got {}",
                backdrop_alpha(&calls)
            );
            for call in &calls {
                for c in colours(call) {
                    assert!(
                        c[3] <= opacity + 1e-3,
                        "{progress} of the way through the fade {call:?} is above {opacity}"
                    );
                }
            }
        }
    }

    #[test]
    fn done_paints_nothing_at_all() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        screen.ready(1.0);
        screen.skip(1.0);
        let mut ui = recorder().with_click_at([720.0, 450.0]);
        assert_eq!(screen.paint(&mut ui, RECT, 1.5), LoadingState::Done);
        assert_eq!(ui.calls(), &[], "a finished loading screen paints no call");
    }

    // ── Skipping by hand ──────────────────────────────

    #[test]
    fn a_click_on_the_screen_once_ready_starts_the_fade() {
        // Ready at film time 2.0: the chapter would play on to 3.4.
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        screen.ready(2.0);
        let mut ui = recorder().with_click_at([720.0, 450.0]);
        assert_eq!(screen.paint(&mut ui, RECT, 2.0), LoadingState::Fading);
        assert_states(&screen, &[2.0, 2.39], LoadingState::Fading);
        assert_states(&screen, &[2.41], LoadingState::Done);
    }

    #[test]
    fn a_click_on_the_screen_before_ready_does_not_skip() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        let mut ui = recorder().with_click_at([720.0, 450.0]);
        assert_eq!(screen.paint(&mut ui, RECT, 2.0), LoadingState::Playing);
        assert_states(&screen, &[2.41, 3.0, 50.0], LoadingState::Playing);

        // Ready later, at film time 3.0: the chapter still plays on to 3.4.
        screen.ready(3.0);
        assert_states(&screen, &[3.39], LoadingState::Playing);
        assert_states(&screen, &[3.41, 3.79], LoadingState::Fading);
        assert_states(&screen, &[3.81], LoadingState::Done);
    }

    #[test]
    fn a_click_outside_the_screen_does_not_skip() {
        let rect = [200.0, 100.0, 600.0, 400.0];
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        // Ready, so a skip would count: the chapter plays on to 3.4.
        screen.ready(2.0);
        let mut ui = RecordingUiBuilder::new(1000.0, 600.0).with_click_at([50.0, 50.0]);
        assert_eq!(screen.paint(&mut ui, rect, 2.0), LoadingState::Playing);
        assert_states(&screen, &[3.0], LoadingState::Playing);
    }

    #[test]
    fn esc_once_ready_starts_the_fade() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        screen.ready(2.0);
        let mut ui = recorder().with_key(KeyCode::Escape);
        assert_eq!(screen.paint(&mut ui, RECT, 2.0), LoadingState::Fading);
        assert_states(&screen, &[2.39], LoadingState::Fading);
        assert_states(&screen, &[2.41], LoadingState::Done);
    }

    #[test]
    fn esc_before_ready_does_not_skip() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        let mut ui = recorder().with_key(KeyCode::Escape);
        assert_eq!(screen.paint(&mut ui, RECT, 2.0), LoadingState::Playing);
        assert_states(&screen, &[2.41, 50.0], LoadingState::Playing);
    }

    #[test]
    fn another_key_does_not_skip() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        screen.ready(2.0);
        let mut ui = recorder().with_key(KeyCode::Space);
        assert_eq!(screen.paint(&mut ui, RECT, 2.0), LoadingState::Playing);
    }

    #[test]
    fn a_painted_frame_without_input_leaves_the_screen_playing() {
        let mut screen = LoadingScreen::new(Film::Sign, 0.0, false);
        for now in [0.0, 1.0, 30.0] {
            let mut ui = recorder();
            assert_eq!(screen.paint(&mut ui, RECT, now), LoadingState::Playing);
        }
        assert_states(&screen, &[31.0], LoadingState::Playing);
    }
}
