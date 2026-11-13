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
mod tests;
