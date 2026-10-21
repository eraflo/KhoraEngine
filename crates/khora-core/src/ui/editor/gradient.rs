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

//! Linear and radial gradients for the paint contract.
//!
//! A [`LinearGradient`] colours a shape along an axis running from `from` to
//! `to`, through a list of [`GradientStop`]s — the same model as an SVG
//! `<linearGradient>` in user-space units. A [`RadialGradient`] colours it by
//! distance from a centre through the same stops — an SVG `<radialGradient>`
//! whose focal point is its centre.

/// One colour on a gradient's axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// Position on the axis: 0 at the gradient's `from`, 1 at its `to`.
    pub offset: f32,
    /// sRGB colour `[r, g, b, a]` in `0..=1`.
    pub color: [f32; 4],
}

/// A linear gradient in window-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearGradient<'a> {
    /// Where offset 0 lies.
    pub from: [f32; 2],
    /// Where offset 1 lies.
    pub to: [f32; 2],
    /// The colour stops, sorted by offset.
    pub stops: &'a [GradientStop],
}

impl LinearGradient<'_> {
    /// Projection of `p` on the axis: 0 at `from`, 1 at `to`; not clamped.
    /// A degenerate axis (`from == to`) gives 1.0.
    pub fn offset_at(&self, p: [f32; 2]) -> f32 {
        let axis = [self.to[0] - self.from[0], self.to[1] - self.from[1]];
        let len_sq = axis[0] * axis[0] + axis[1] * axis[1];
        if len_sq <= f32::EPSILON {
            return 1.0;
        }
        ((p[0] - self.from[0]) * axis[0] + (p[1] - self.from[1]) * axis[1]) / len_sq
    }

    /// The colour at `p`, with SVG semantics: clamped to the first / last
    /// stop outside them, linear (componentwise) between stops. Stops are
    /// assumed sorted by offset. No stops gives `[0.0; 4]`, one stop gives its
    /// colour, and a degenerate axis (`from == to`) gives the last stop's
    /// colour.
    pub fn color_at(&self, p: [f32; 2]) -> [f32; 4] {
        color_at_offset(self.stops, self.offset_at(p))
    }
}

/// A radial gradient in window-space coordinates: offset 0 at `center`,
/// offset 1 on the circle of `radius` around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadialGradient<'a> {
    /// Where offset 0 lies.
    pub center: [f32; 2],
    /// Distance from `center` at which offset 1 lies.
    pub radius: f32,
    /// The colour stops, sorted by offset.
    pub stops: &'a [GradientStop],
}

impl RadialGradient<'_> {
    /// Distance of `p` from `center` divided by `radius`: 0 at the centre, 1
    /// on the circle; not clamped. A radius ≤ 0 gives 1.0.
    pub fn offset_at(&self, p: [f32; 2]) -> f32 {
        if self.radius <= 0.0 {
            return 1.0;
        }
        let d = [p[0] - self.center[0], p[1] - self.center[1]];
        (d[0] * d[0] + d[1] * d[1]).sqrt() / self.radius
    }

    /// The colour at `p`, with the same stop semantics as
    /// [`LinearGradient::color_at`]: clamped to the first / last stop outside
    /// them, linear (componentwise) between stops. No stops gives
    /// `[0.0; 4]`, one stop gives its colour, and a radius ≤ 0 gives the last
    /// stop's colour.
    pub fn color_at(&self, p: [f32; 2]) -> [f32; 4] {
        color_at_offset(self.stops, self.offset_at(p))
    }
}

/// The colour of `stops` at offset `t`: clamped to the first / last stop
/// outside them, linear (componentwise) between stops; the later of two stops
/// sharing an offset wins at that offset. No stops gives `[0.0; 4]`.
fn color_at_offset(stops: &[GradientStop], t: f32) -> [f32; 4] {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return [0.0; 4];
    };
    if t < first.offset {
        return first.color;
    }
    if t >= last.offset {
        return last.color;
    }
    // The first pair ending past `t`: a hard edge exactly at `t` is skipped,
    // so its later stop starts the next pair and wins.
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t < b.offset {
            let span = b.offset - a.offset;
            let u = if span > 0.0 {
                (t - a.offset) / span
            } else {
                1.0
            };
            return std::array::from_fn(|i| a.color[i] + (b.color[i] - a.color[i]) * u);
        }
    }
    last.color
}

/// The vertex centroid of a polygon of at least 3 points — where a backend
/// without gradient fills samples the gradient for its flat colour.
pub fn flat_fill_point(points: &[[f32; 2]]) -> Option<[f32; 2]> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f32;
    let sum = points
        .iter()
        .fold([0.0, 0.0], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
    Some([sum[0] / n, sum[1] / n])
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 0.5];

    const THREE: [GradientStop; 3] = [
        GradientStop {
            offset: 0.0,
            color: RED,
        },
        GradientStop {
            offset: 0.5,
            color: GREEN,
        },
        GradientStop {
            offset: 1.0,
            color: BLUE,
        },
    ];

    /// A horizontal axis 100 units long, red → green → blue.
    fn horizontal(stops: &[GradientStop]) -> LinearGradient<'_> {
        LinearGradient {
            from: [0.0, 0.0],
            to: [100.0, 0.0],
            stops,
        }
    }

    fn assert_color(actual: [f32; 4], expected: [f32; 4]) {
        for (i, (got, want)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() < EPS,
                "channel {i}: got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn offset_at_is_zero_at_from_and_one_at_to() {
        let g = horizontal(&THREE);
        assert!((g.offset_at([0.0, 0.0]) - 0.0).abs() < EPS);
        assert!((g.offset_at([100.0, 0.0]) - 1.0).abs() < EPS);
        assert!((g.offset_at([25.0, 0.0]) - 0.25).abs() < EPS);
    }

    #[test]
    fn offset_at_is_not_clamped_beyond_the_axis_ends() {
        let g = horizontal(&THREE);
        assert!((g.offset_at([150.0, 0.0]) - 1.5).abs() < EPS);
        assert!((g.offset_at([-50.0, 0.0]) + 0.5).abs() < EPS);
    }

    #[test]
    fn offset_at_projects_onto_a_diagonal_axis() {
        let g = LinearGradient {
            from: [0.0, 0.0],
            to: [10.0, 10.0],
            stops: &THREE,
        };
        // [10, 0] projects onto the middle of the diagonal.
        assert!((g.offset_at([10.0, 0.0]) - 0.5).abs() < EPS);
        assert!((g.offset_at([0.0, 10.0]) - 0.5).abs() < EPS);
        assert!((g.offset_at([20.0, 20.0]) - 2.0).abs() < EPS);
    }

    #[test]
    fn offset_at_on_a_degenerate_axis_is_one() {
        let g = LinearGradient {
            from: [5.0, 5.0],
            to: [5.0, 5.0],
            stops: &THREE,
        };
        assert_eq!(g.offset_at([0.0, 0.0]), 1.0);
        assert_eq!(g.offset_at([5.0, 5.0]), 1.0);
    }

    #[test]
    fn color_at_from_is_the_first_stop() {
        assert_color(horizontal(&THREE).color_at([0.0, 0.0]), RED);
    }

    #[test]
    fn color_at_a_middle_stop_is_that_stop() {
        assert_color(horizontal(&THREE).color_at([50.0, 0.0]), GREEN);
    }

    #[test]
    fn color_at_to_is_the_last_stop() {
        assert_color(horizontal(&THREE).color_at([100.0, 0.0]), BLUE);
    }

    #[test]
    fn color_between_stops_is_linear_per_component() {
        let g = horizontal(&THREE);
        assert_color(g.color_at([25.0, 0.0]), [0.5, 0.5, 0.0, 1.0]);
        // Alpha interpolates too: green (1.0) → blue (0.5).
        assert_color(g.color_at([75.0, 0.0]), [0.0, 0.5, 0.5, 0.75]);
    }

    #[test]
    fn color_before_from_clamps_to_the_first_stop() {
        assert_color(horizontal(&THREE).color_at([-40.0, 0.0]), RED);
    }

    #[test]
    fn color_beyond_to_clamps_to_the_last_stop() {
        assert_color(horizontal(&THREE).color_at([180.0, 0.0]), BLUE);
    }

    #[test]
    fn color_ignores_the_offset_perpendicular_to_the_axis() {
        let g = horizontal(&THREE);
        assert_color(g.color_at([25.0, 80.0]), g.color_at([25.0, 0.0]));
        assert_color(g.color_at([25.0, -300.0]), [0.5, 0.5, 0.0, 1.0]);
    }

    #[test]
    fn color_clamps_to_stops_that_do_not_span_the_whole_axis() {
        let stops = [
            GradientStop {
                offset: 0.2,
                color: RED,
            },
            GradientStop {
                offset: 0.8,
                color: GREEN,
            },
        ];
        let g = horizontal(&stops);
        assert_color(g.color_at([10.0, 0.0]), RED);
        assert_color(g.color_at([50.0, 0.0]), [0.5, 0.5, 0.0, 1.0]);
        assert_color(g.color_at([90.0, 0.0]), GREEN);
    }

    #[test]
    fn color_exactly_on_a_hard_edge_is_the_later_stop() {
        // SVG: "If two gradient stops have the same offset value, then the
        // latter gradient stop controls the color value at the overlap point."
        let stops = [
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: GREEN,
            },
            GradientStop {
                offset: 0.5,
                color: BLUE,
            },
            GradientStop {
                offset: 1.0,
                color: RED,
            },
        ];
        assert_color(horizontal(&stops).color_at([50.0, 0.0]), BLUE);
        assert_color(ring(&stops).color_at(at(10.0, 0.0)), BLUE);
    }

    #[test]
    fn color_with_no_stops_is_transparent_black() {
        assert_eq!(horizontal(&[]).color_at([50.0, 0.0]), [0.0; 4]);
    }

    #[test]
    fn color_with_one_stop_is_that_stop_everywhere() {
        let stops = [GradientStop {
            offset: 0.5,
            color: BLUE,
        }];
        let g = horizontal(&stops);
        for p in [
            [-10.0, 0.0],
            [0.0, 0.0],
            [50.0, 7.0],
            [100.0, 0.0],
            [300.0, 0.0],
        ] {
            assert_color(g.color_at(p), BLUE);
        }
    }

    #[test]
    fn color_on_a_degenerate_axis_is_the_last_stop() {
        let g = LinearGradient {
            from: [5.0, 5.0],
            to: [5.0, 5.0],
            stops: &THREE,
        };
        assert_color(g.color_at([5.0, 5.0]), BLUE);
        assert_color(g.color_at([-100.0, 40.0]), BLUE);
    }

    /// A circle of radius 20 around `[50, 40]`.
    fn ring(stops: &[GradientStop]) -> RadialGradient<'_> {
        RadialGradient {
            center: [50.0, 40.0],
            radius: 20.0,
            stops,
        }
    }

    /// The point `distance` away from [`ring`]'s centre in direction `angle`
    /// (radians).
    fn at(distance: f32, angle: f32) -> [f32; 2] {
        [50.0 + distance * angle.cos(), 40.0 + distance * angle.sin()]
    }

    /// Eight directions spread around the circle, none along an axis.
    fn directions() -> impl Iterator<Item = f32> {
        (0..8).map(|k| k as f32 * std::f32::consts::FRAC_PI_4 + 0.3)
    }

    #[test]
    fn radial_offset_at_is_zero_at_the_centre_and_one_on_the_circle() {
        let g = ring(&THREE);
        assert!(g.offset_at([50.0, 40.0]).abs() < EPS);
        assert!((g.offset_at([70.0, 40.0]) - 1.0).abs() < EPS);
        assert!((g.offset_at([50.0, 20.0]) - 1.0).abs() < EPS);
    }

    #[test]
    fn radial_offset_at_is_the_same_in_every_direction() {
        let g = ring(&THREE);
        for angle in directions() {
            let half = g.offset_at(at(10.0, angle));
            assert!((half - 0.5).abs() < EPS, "halfway at {angle} rad: {half}");
            let quarter = g.offset_at(at(5.0, angle));
            assert!(
                (quarter - 0.25).abs() < EPS,
                "a quarter of the way at {angle} rad: {quarter}"
            );
        }
    }

    #[test]
    fn radial_offset_at_is_not_clamped_beyond_the_circle() {
        let g = ring(&THREE);
        assert!((g.offset_at([90.0, 40.0]) - 2.0).abs() < EPS);
        assert!((g.offset_at(at(30.0, 2.0)) - 1.5).abs() < EPS);
    }

    #[test]
    fn radial_offset_at_with_a_zero_or_negative_radius_is_one() {
        for radius in [0.0, -5.0] {
            let g = RadialGradient {
                center: [50.0, 40.0],
                radius,
                stops: &THREE,
            };
            assert_eq!(
                g.offset_at([50.0, 40.0]),
                1.0,
                "at the centre, r = {radius}"
            );
            assert_eq!(g.offset_at([0.0, 0.0]), 1.0, "away, r = {radius}");
        }
    }

    #[test]
    fn radial_color_at_the_centre_is_the_first_stop() {
        assert_color(ring(&THREE).color_at([50.0, 40.0]), RED);
    }

    #[test]
    fn radial_color_halfway_is_the_middle_stop_in_every_direction() {
        let g = ring(&THREE);
        for angle in directions() {
            assert_color(g.color_at(at(10.0, angle)), GREEN);
        }
    }

    #[test]
    fn radial_color_on_the_circle_is_the_last_stop() {
        let g = ring(&THREE);
        for angle in directions() {
            assert_color(g.color_at(at(20.0, angle)), BLUE);
        }
    }

    #[test]
    fn radial_color_between_stops_is_linear_per_component() {
        let g = ring(&THREE);
        assert_color(g.color_at(at(5.0, 1.0)), [0.5, 0.5, 0.0, 1.0]);
        assert_color(g.color_at(at(15.0, -2.0)), [0.0, 0.5, 0.5, 0.75]);
    }

    #[test]
    fn radial_color_beyond_the_circle_clamps_to_the_last_stop() {
        let g = ring(&THREE);
        assert_color(g.color_at([90.0, 40.0]), BLUE);
        assert_color(g.color_at([-500.0, 900.0]), BLUE);
    }

    #[test]
    fn radial_color_clamps_to_stops_that_do_not_span_the_whole_radius() {
        let stops = [
            GradientStop {
                offset: 0.2,
                color: RED,
            },
            GradientStop {
                offset: 0.8,
                color: GREEN,
            },
        ];
        let g = ring(&stops);
        assert_color(g.color_at(at(2.0, 0.7)), RED);
        assert_color(g.color_at(at(10.0, 0.7)), [0.5, 0.5, 0.0, 1.0]);
        assert_color(g.color_at(at(18.0, 0.7)), GREEN);
    }

    #[test]
    fn radial_color_matches_a_linear_gradient_at_the_same_offset() {
        // A hard edge at 0.5 and stops short of both ends: the radial colour
        // at distance d follows the linear colour at offset d / radius.
        let stops = [
            GradientStop {
                offset: 0.1,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: GREEN,
            },
            GradientStop {
                offset: 0.5,
                color: BLUE,
            },
            GradientStop {
                offset: 0.9,
                color: RED,
            },
        ];
        let radial = ring(&stops);
        let linear = LinearGradient {
            from: [0.0, 0.0],
            to: [20.0, 0.0],
            stops: &stops,
        };
        for d in [0.0, 1.0, 3.0, 7.5, 9.9, 10.1, 13.0, 17.0, 19.0, 25.0] {
            assert_color(radial.color_at(at(d, 2.5)), linear.color_at([d, 0.0]));
        }
    }

    #[test]
    fn radial_color_with_no_stops_is_transparent_black() {
        assert_eq!(ring(&[]).color_at([55.0, 40.0]), [0.0; 4]);
    }

    #[test]
    fn radial_color_with_one_stop_is_that_stop_everywhere() {
        let stops = [GradientStop {
            offset: 0.5,
            color: BLUE,
        }];
        let g = ring(&stops);
        for p in [[50.0, 40.0], at(10.0, 1.0), at(20.0, 3.0), [-300.0, 0.0]] {
            assert_color(g.color_at(p), BLUE);
        }
    }

    #[test]
    fn radial_color_with_a_zero_radius_is_the_last_stop() {
        let g = RadialGradient {
            center: [50.0, 40.0],
            radius: 0.0,
            stops: &THREE,
        };
        assert_color(g.color_at([50.0, 40.0]), BLUE);
        assert_color(g.color_at([-100.0, 40.0]), BLUE);
    }
}
