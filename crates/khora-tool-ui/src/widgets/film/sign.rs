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

//! "Le signe": the square is traced, appears whole, is cut from a round hole
//! at the joint; the pieces negotiate around a gold glow; the left piece
//! swings open onto the light and shuts; the light goes out, the cuts close
//! back into the centre and everything fades.

use std::f32::consts::{PI, TAU};

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::GradientStop;

use super::canvas::{rgb, Canvas, Fill, Lettering, Local};
use super::{cl, eo, ez, sign_pieces, GOLD, LIGHT};

const H: f32 = 7.0;
const DARK: [f32; 4] = rgb(0x0f141d);
const TRACE: [f32; 4] = rgb(0x8fa6bf);
const INK: [f32; 4] = rgb(0xeef3f8);

const PIECE_L: [GradientStop; 2] = stops2(0xeef3f8, 0xa9bacc);
const PIECE_T: [GradientStop; 2] = stops2(0xf7fafc, 0xcdd9e5);
const PIECE_R: [GradientStop; 2] = stops2(0xd3dde8, 0x8ea4bc);
const PIECE_B: [GradientStop; 2] = stops2(0xb9c8d7, 0x7a91aa);

const fn stops2(a: u32, b: u32) -> [GradientStop; 2] {
    [
        GradientStop {
            offset: 0.0,
            color: rgb(a),
        },
        GradientStop {
            offset: 1.0,
            color: rgb(b),
        },
    ]
}

/// The glow behind the cut: the light's centre and middle, fading out.
const GLOW: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: LIGHT[0],
    },
    GradientStop {
        offset: 0.35,
        color: LIGHT[1],
    },
    GradientStop {
        offset: 1.0,
        color: [LIGHT[1][0], LIGHT[1][1], LIGHT[1][2], 0.0],
    },
];

/// The lit square behind the door.
const LIT: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: LIGHT[0],
    },
    GradientStop {
        offset: 0.5,
        color: LIGHT[1],
    },
    GradientStop {
        offset: 1.0,
        color: LIGHT[2],
    },
];

pub(super) fn paint(c: &mut Canvas<'_>, t: f32) {
    let p_tick = ez(t / 1.2);
    let p_frame = ez((t - 0.4) / 1.8);
    let p_cut = ez((t - 1.6) / 1.8);
    let p_piece = ez((t - 3.4) / 1.4);
    // The hole, as wide as the cut will be, then the cuts carved out of it
    // along the gold lines. Once carved, the pieces stand at their gap and the
    // carving is no longer drawn: the same shape.
    let p_hole = eo((t - 5.0) / 0.6, 3.0);
    let p_carve = ez((t - 5.7) / 1.8);
    let carved = t >= 5.7 + 1.8;
    let p_open = if carved { 1.0 } else { 0.0 };
    // The door: the left rectangle swings open on its left edge onto the
    // light, holds, and swings shut with a small settle.
    let d_open = eo((t - 15.7) / 1.2, 3.0);
    let d_shut = ez((t - 17.3) / 1.1);
    let settle = if t > 18.4 && t < 18.75 {
        0.035 * (PI * (t - 18.4) / 0.35).sin()
    } else {
        0.0
    };
    let door = (d_open * (1.0 - d_shut) + settle).max(0.0);
    // Once the door is shut the light goes out, and only when it is out do the
    // cuts close, retract into the centre, and fade.
    let dark = ez((t - 18.5) / 1.0);
    let p_close = ez((t - 19.7) / 1.1);
    let p_retract = ez((t - 20.4) / 0.8);
    let fade = cl((t - 21.1) / 0.9);
    let idle = cl((t - 7.6) / 2.0) * (1.0 - d_shut);
    // The gap breathes a little; the pieces trade space in opposed pairs, so
    // the K keeps its width. They hold still while the door is open.
    let calm = 1.0 - cl(door * 3.0);
    let breath = idle * calm * 0.07 * (TAU * (t - 7.6) / 5.6).sin();
    let h = H * p_open * (1.0 - p_close) * (1.0 + breath);
    let sway_a = idle * calm * 1.6 * (TAU * (t - 7.6) / 4.4).sin();
    let sway_b = idle * calm * 1.3 * (TAU * (t - 7.6) / 5.2 + 1.2).sin();
    let p = sign_pieces(h);

    // The left piece as a door, seen in perspective.
    let ang = door * 74.0_f32.to_radians();
    let foc = 1100.0;
    let hinge_x = -200.0 - sway_a;
    let door_pts: Vec<[f32; 2]> = p[0]
        .iter()
        .map(|&[x, y]| {
            let w = x + 200.0;
            let z = w * ang.sin();
            let sc = foc / (foc - z);
            [hinge_x + w * ang.cos() * sc, y * sc]
        })
        .collect();
    let shift = |q: &[[f32; 2]], dx: f32, dy: f32| -> Vec<[f32; 2]> {
        q.iter().map(|&[x, y]| [x + dx, y + dy]).collect()
    };
    let r = shift(&p[2], -sway_a * 0.9, 0.0);
    let tp = shift(&p[1], 0.447 * sway_b, -0.894 * sway_b);
    let b = shift(&p[3], -0.447 * sway_b, 0.894 * sway_b);

    // The light: a glow that rises behind the cut while the pieces negotiate,
    // then the lit square, seen through the cuts before the door swings.
    let rise = ez((t - 14.2) / 1.6);
    let pulse = 0.8 + 0.2 * (TAU * t / 3.1).sin() * (1.0 - rise);
    let square = [
        [-200.0, -200.0],
        [200.0, -200.0],
        [200.0, 200.0],
        [-200.0, 200.0],
    ];

    let sign_space = Local {
        dx: 720.0,
        dy: 350.0,
        s: 0.74,
    };
    c.transformed(sign_space, |c| {
        c.group(1.0 - fade, |c| {
            c.group(0.75 * idle * pulse * (1.0 - rise), |c| {
                c.poly(
                    &square,
                    Fill::Radial {
                        center: [0.0, 0.0],
                        radius: 230.0,
                        stops: &GLOW,
                    },
                );
            });
            c.group(rise * (1.0 - dark), |c| {
                c.poly(
                    &square,
                    Fill::Radial {
                        center: [0.0, 0.0],
                        radius: 300.0,
                        stops: &LIT,
                    },
                );
            });
            c.group(0.55 * (1.0 - p_piece), |c| {
                c.outline_part(&square, p_frame, 0.8, TRACE);
            });
            c.group(0.5 * p_tick * (1.0 - 0.6 * p_piece), |c| {
                for corner in [
                    [[-236.0, -212.0], [-236.0, -236.0], [-212.0, -236.0]],
                    [[212.0, -236.0], [236.0, -236.0], [236.0, -212.0]],
                    [[236.0, 212.0], [236.0, 236.0], [212.0, 236.0]],
                    [[-212.0, 236.0], [-236.0, 236.0], [-236.0, 212.0]],
                ] {
                    c.line(corner[0], corner[1], 0.8, TRACE, false);
                    c.line(corner[1], corner[2], 0.8, TRACE, false);
                }
            });
            c.group(p_piece, |c| {
                c.poly(
                    &door_pts,
                    Fill::Linear {
                        from: [-200.0, -200.0],
                        to: [0.0, 200.0],
                        stops: &PIECE_L,
                    },
                );
                c.group(0.55 * ang.sin(), |c| {
                    c.poly(&door_pts, Fill::Solid(rgb(0x0b1018)));
                });
                c.group(cl(door * 6.0), |c| {
                    c.line(door_pts[1], door_pts[2], 1.6, rgb(0xf3cf7a), false);
                });
                c.poly(
                    &tp,
                    Fill::Linear {
                        from: [0.0, -200.0],
                        to: [200.0, 0.0],
                        stops: &PIECE_T,
                    },
                );
                c.poly(
                    &r,
                    Fill::Linear {
                        from: [0.0, -200.0],
                        to: [200.0, 200.0],
                        stops: &PIECE_R,
                    },
                );
                c.poly(
                    &b,
                    Fill::Linear {
                        from: [0.0, 0.0],
                        to: [200.0, 200.0],
                        stops: &PIECE_B,
                    },
                );
            });
            // The hole and the carving, shown only until the pieces take over.
            if !carved {
                for end in [[0.0, -206.0], [0.0, 206.0], [206.0, -206.0], [206.0, 206.0]] {
                    c.line_part([0.0, 0.0], end, p_carve, 14.0, DARK, false);
                }
                c.circle([0.0, 0.0], H * p_hole, Fill::Solid(DARK));
            }
            // The gold guide lines fade out as the silver square appears.
            let cut_shown = 1.0 - ((1.0 - p_cut) + p_retract).min(1.0);
            c.group(1.0 - p_piece, |c| {
                for end in [[0.0, -200.0], [0.0, 200.0], [200.0, -200.0], [200.0, 200.0]] {
                    c.line_part([0.0, 0.0], end, cut_shown, 0.9, GOLD, true);
                }
            });
            c.group((p_retract * (1.0 - fade)).max(0.0), |c| {
                c.circle([0.0, 0.0], 3.2, Fill::Solid(GOLD));
            });
        });
    });

    // The name, tightening as it arrives.
    let wm = ez((t - 6.6) / 1.8) * (1.0 - fade);
    let lift = 10.0 * (1.0 - wm);
    c.group(wm, |c| {
        c.tracked_text(
            "KHORA",
            720.0,
            690.0 + lift,
            Lettering {
                size: 46.0,
                tracking: 28.5 + 22.0 * (1.0 - wm),
                family: FontFamilyHint::Wordmark,
                color: INK,
            },
        );
        c.poly(
            &[
                [706.0, 722.0 + lift],
                [734.0, 722.0 + lift],
                [734.0, 723.0 + lift],
                [706.0, 723.0 + lift],
            ],
            Fill::Solid(GOLD),
        );
        c.tracked_text(
            "ΧΩΡΑ",
            720.0,
            762.0 + lift,
            Lettering {
                size: 12.0,
                tracking: 6.0,
                family: FontFamilyHint::Monospace,
                color: TRACE,
            },
        );
    });
}
