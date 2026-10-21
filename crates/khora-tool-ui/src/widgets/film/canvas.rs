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

//! The films' drawing surface: their 1440 × 900 composition fitted into a
//! rect, with group opacity and convex clipping, over the paint contract.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, TextAlign};
use khora_core::ui::editor::{GradientStop, LinearGradient, RadialGradient};
use khora_core::ui::UiBuilder;

use crate::widgets::{Color, Rect};

/// The composition's size, in its own units.
pub(super) const WIDTH: f32 = 1440.0;
/// The composition's size, in its own units.
pub(super) const HEIGHT: f32 = 900.0;

/// A colour from its sRGB hex bytes, opaque.
pub(super) const fn rgb(hex: u32) -> Color {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// `c` with its alpha multiplied by `a`.
pub(super) fn fade(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], c[3] * a.clamp(0.0, 1.0)]
}

/// How a shape is filled, in composition units.
#[derive(Clone, Copy)]
pub(super) enum Fill<'a> {
    Solid(Color),
    Linear {
        from: [f32; 2],
        to: [f32; 2],
        stops: &'a [GradientStop],
    },
    Radial {
        center: [f32; 2],
        radius: f32,
        stops: &'a [GradientStop],
    },
}

/// A 2D affine map applied to composition points before the stage: what an
/// SVG group's `translate(..) scale(..)` does.
#[derive(Clone, Copy, Debug)]
pub(super) struct Local {
    pub dx: f32,
    pub dy: f32,
    pub s: f32,
}

impl Local {
    pub const IDENTITY: Local = Local {
        dx: 0.0,
        dy: 0.0,
        s: 1.0,
    };
    fn map(&self, p: [f32; 2]) -> [f32; 2] {
        [self.dx + p[0] * self.s, self.dy + p[1] * self.s]
    }
}

/// The composition fitted into a rect (centred, uniform scale), with the
/// opacity, local transform and convex clip of the group being drawn.
pub(super) struct Canvas<'u> {
    ui: &'u mut dyn UiBuilder,
    origin: [f32; 2],
    scale: f32,
    local: Local,
    alpha: f32,
    /// Convex clip polygon, screen coordinates, counter-clockwise or clockwise.
    clip: Vec<[f32; 2]>,
}

impl<'u> Canvas<'u> {
    /// Fills `rect` with `backdrop` and fits the composition inside it, the
    /// whole frame at `opacity` (0..=1).
    pub fn new(ui: &'u mut dyn UiBuilder, rect: Rect, backdrop: Color, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        ui.paint_rect_filled(
            [rect[0], rect[1]],
            [rect[2], rect[3]],
            fade(backdrop, opacity),
            0.0,
        );
        let scale = (rect[2] / WIDTH).min(rect[3] / HEIGHT).max(0.0);
        let origin = [
            rect[0] + (rect[2] - WIDTH * scale) * 0.5,
            rect[1] + (rect[3] - HEIGHT * scale) * 0.5,
        ];
        let clip = vec![
            [rect[0], rect[1]],
            [rect[0] + rect[2], rect[1]],
            [rect[0] + rect[2], rect[1] + rect[3]],
            [rect[0], rect[1] + rect[3]],
        ];
        Self {
            ui,
            origin,
            scale,
            local: Local::IDENTITY,
            alpha: opacity,
            clip,
        }
    }

    /// Runs `f` with the group opacity multiplied by `a`.
    pub fn group(&mut self, a: f32, f: impl FnOnce(&mut Self)) {
        let saved = self.alpha;
        self.alpha *= a.clamp(0.0, 1.0);
        if self.alpha > 0.0 {
            f(self);
        }
        self.alpha = saved;
    }

    /// Runs `f` with `local` composed onto the current transform.
    pub fn transformed(&mut self, local: Local, f: impl FnOnce(&mut Self)) {
        let saved = self.local;
        self.local = Local {
            dx: saved.dx + local.dx * saved.s,
            dy: saved.dy + local.dy * saved.s,
            s: saved.s * local.s,
        };
        f(self);
        self.local = saved;
    }

    /// Runs `f` clipped, in addition, to the convex polygon `region`
    /// (composition units, current transform).
    pub fn clipped(&mut self, region: &[[f32; 2]], f: impl FnOnce(&mut Self)) {
        let region: Vec<[f32; 2]> = region.iter().map(|&p| self.to_screen(p)).collect();
        let narrowed = clip_polygon(&region, &self.clip);
        let saved = std::mem::replace(&mut self.clip, narrowed);
        if self.clip.len() >= 3 {
            f(self);
        }
        self.clip = saved;
    }

    /// A composition point on screen.
    pub fn to_screen(&self, p: [f32; 2]) -> [f32; 2] {
        let q = self.local.map(p);
        [
            self.origin[0] + q[0] * self.scale,
            self.origin[1] + q[1] * self.scale,
        ]
    }

    /// A composition length on screen.
    pub fn len(&self, l: f32) -> f32 {
        l * self.local.s * self.scale
    }

    /// Fills the convex polygon `points`.
    pub fn poly(&mut self, points: &[[f32; 2]], fill: Fill<'_>) {
        if self.alpha <= 0.0 || points.len() < 3 {
            return;
        }
        let screen: Vec<[f32; 2]> = points.iter().map(|&p| self.to_screen(p)).collect();
        let clipped = clip_polygon(&screen, &self.clip);
        if clipped.len() < 3 {
            return;
        }
        let a = self.alpha;
        match fill {
            Fill::Solid(c) => self.ui.paint_path_filled(&clipped, fade(c, a)),
            Fill::Linear { from, to, stops } => {
                let stops = faded(stops, a);
                self.ui.paint_path_filled_gradient(
                    &clipped,
                    LinearGradient {
                        from: self.to_screen(from),
                        to: self.to_screen(to),
                        stops: &stops,
                    },
                );
            }
            Fill::Radial {
                center,
                radius,
                stops,
            } => {
                let stops = faded(stops, a);
                self.ui.paint_path_filled_radial(
                    &clipped,
                    RadialGradient {
                        center: self.to_screen(center),
                        radius: self.len(radius),
                        stops: &stops,
                    },
                );
            }
        }
    }

    /// Fills a circle, as a 48-gon.
    pub fn circle(&mut self, center: [f32; 2], r: f32, fill: Fill<'_>) {
        if r <= 0.0 || !r.is_finite() {
            return;
        }
        let ring: Vec<[f32; 2]> = (0..48)
            .map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                [center[0] + r * a.cos(), center[1] + r * a.sin()]
            })
            .collect();
        self.poly(&ring, fill);
    }

    /// A straight stroke from `a` to `b`, `width` composition units wide;
    /// `round` adds round caps.
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: Color, round: bool) {
        if self.alpha <= 0.0 || width <= 0.0 {
            return;
        }
        let (sa, sb) = (self.to_screen(a), self.to_screen(b));
        if !(sa.iter().chain(&sb).all(|v| v.is_finite())) {
            return;
        }
        // The stroke is painted as its own outline — a quad, and discs for
        // round caps — clipped like any other shape, so no part of its width
        // leaves the clip region.
        let c = fade(color, self.alpha);
        let w = self.len(width);
        let body = clip_polygon(&bar(sa, sb, w), &self.clip);
        if body.len() >= 3 {
            self.ui.paint_path_filled(&body, c);
        }
        if round {
            for end in [sa, sb] {
                let cap = clip_polygon(&disc(end, w * 0.5), &self.clip);
                if cap.len() >= 3 {
                    self.ui.paint_path_filled(&cap, c);
                }
            }
        }
    }

    /// The first `progress` (0..=1) of the line from `a` to `b`.
    pub fn line_part(
        &mut self,
        a: [f32; 2],
        b: [f32; 2],
        progress: f32,
        width: f32,
        color: Color,
        round: bool,
    ) {
        let k = progress.clamp(0.0, 1.0);
        if k <= 0.0 {
            return;
        }
        let end = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
        self.line(a, end, width, color, round);
    }

    /// The first `progress` (0..=1) of the closed outline through `points`.
    pub fn outline_part(&mut self, points: &[[f32; 2]], progress: f32, width: f32, color: Color) {
        let n = points.len();
        if n < 2 {
            return;
        }
        let seg = |i: usize| (points[i], points[(i + 1) % n]);
        let len =
            |(a, b): ([f32; 2], [f32; 2])| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let total: f32 = (0..n).map(|i| len(seg(i))).sum();
        let mut left = total * progress.clamp(0.0, 1.0);
        for i in 0..n {
            if left <= 0.0 {
                break;
            }
            let (a, b) = seg(i);
            let l = len((a, b));
            self.line_part(
                a,
                b,
                if l > 0.0 { left / l } else { 1.0 },
                width,
                color,
                false,
            );
            left -= l;
        }
    }

    /// The closed outline through `points`.
    pub fn outline(&mut self, points: &[[f32; 2]], width: f32, color: Color) {
        self.outline_part(points, 1.0, width, color);
    }

    /// `text` set in `lettering`, centred on `x`, its baseline at `baseline`,
    /// painted glyph by glyph.
    pub fn tracked_text(&mut self, text: &str, x: f32, baseline: f32, lettering: Lettering) {
        let Lettering {
            size,
            tracking,
            family,
            color,
        } = lettering;
        if self.alpha <= 0.0 {
            return;
        }
        let px = self.len(size);
        if px < 1.0 {
            return;
        }
        let gap = self.len(tracking);
        let glyphs: Vec<(String, f32)> = text
            .chars()
            .map(|ch| {
                let s = ch.to_string();
                let w = self.ui.measure_text(&s, px, family)[0];
                (s, w)
            })
            .collect();
        let width: f32 =
            glyphs.iter().map(|g| g.1).sum::<f32>() + gap * (glyphs.len().saturating_sub(1)) as f32;
        let centre = self.to_screen([x, baseline]);
        let mut cx = centre[0] - width * 0.5;
        // The paint contract places text by its top; the baseline sits about
        // 0.8 em below it.
        let top = centre[1] - px * 0.8;
        let c = fade(color, self.alpha);
        for (g, w) in &glyphs {
            if inside_all(&self.clip, [cx, top], 0.0)
                && inside_all(&self.clip, [cx + w, top + px], 0.0)
            {
                self.ui
                    .paint_text_styled([cx, top], g, px, c, family, TextAlign::Left);
            }
            cx += w + gap;
        }
    }
}

/// How a line of text is set: size and tracking in composition units, face
/// and colour.
#[derive(Clone, Copy)]
pub(super) struct Lettering {
    pub size: f32,
    pub tracking: f32,
    pub family: FontFamilyHint,
    pub color: Color,
}

/// A butt-ended stroke from `a` to `b`, `width` wide, as a quad.
pub(super) fn bar(a: [f32; 2], b: [f32; 2], width: f32) -> [[f32; 2]; 4] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
    let n = [-d[1] / len * width * 0.5, d[0] / len * width * 0.5];
    [
        [a[0] + n[0], a[1] + n[1]],
        [b[0] + n[0], b[1] + n[1]],
        [b[0] - n[0], b[1] - n[1]],
        [a[0] - n[0], a[1] - n[1]],
    ]
}

/// A disc of radius `r` around `c`, as a 24-gon.
fn disc(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..24)
        .map(|i| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// `stops` with every alpha multiplied by `a`.
fn faded(stops: &[GradientStop], a: f32) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|s| GradientStop {
            offset: s.offset,
            color: fade(s.color, a),
        })
        .collect()
}

/// Which side of the directed edge `a → b` the point `p` is on (> 0 left).
fn cross(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// The clip polygon's winding sign (+1 counter-clockwise in screen axes).
fn winding(clip: &[[f32; 2]]) -> f32 {
    let n = clip.len();
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (clip[i], clip[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if area >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Whether `p`, grown by `r`, lies inside the convex `clip`.
fn inside_all(clip: &[[f32; 2]], p: [f32; 2], r: f32) -> bool {
    let w = winding(clip);
    let n = clip.len();
    (0..n).all(|i| {
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2))
            .sqrt()
            .max(1e-6);
        w * cross(a, b, p) / len >= r - 1e-3
    })
}

/// Sutherland–Hodgman: `subject` clipped to the convex `clip`.
pub(super) fn clip_polygon(subject: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if clip.len() < 3 {
        return Vec::new();
    }
    let w = winding(clip);
    let mut out: Vec<[f32; 2]> = subject.to_vec();
    let n = clip.len();
    for i in 0..n {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let input = std::mem::take(&mut out);
        let m = input.len();
        for j in 0..m {
            let (p, q) = (input[j], input[(j + 1) % m]);
            let (dp, dq) = (w * cross(a, b, p), w * cross(a, b, q));
            if dp >= 0.0 {
                out.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                let t = dp / (dp - dq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}
