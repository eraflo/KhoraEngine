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

//! "La porte": the plate stands on a floor, whole. A hole opens at the joint,
//! light wells up in it, runs to the edges and draws the K, casting it in gold
//! on the floor. Someone behind kicks the door open, stands in the light and
//! walks back into it. The light breathes in — dust is drawn into the doorway,
//! the draught pulls the door — and out. The door closes, the light goes out,
//! the pieces close up, and the plate is whole again.

use std::f32::consts::{PI, SQRT_2, TAU};

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::GradientStop;

use super::canvas::{bar, fade, rgb, Canvas, Fill, Lettering};
use super::{cl, eo, ez, GOLD, LIGHT};

// The camera: focal length, scale and centre of the projection, the floor's
// height and the light's position (for the cast shadows).
const F: f32 = 1700.0;
const SC: f32 = 0.74;
const CX: f32 = 720.0;
const CY: f32 = 330.0;
const FLOOR: f32 = 200.0;
const LY: f32 = -900.0;
const LZ: f32 = -520.0;
const LX: f32 = -60.0;

const GRID: [f32; 4] = rgb(0x8fa6bf);
const SHADOW: [f32; 4] = rgb(0x070a0f);
const EDGE: [f32; 4] = rgb(0xc9d6e3);
const INK: [f32; 4] = rgb(0xeef3f8);

/// The light, centred on the plate: the glow and the lit doorway.
const GLOW: [GradientStop; 3] = [
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

/// The light cast on the floor, strong near the plate and gone far from it.
const FLOOR_LIGHT: [GradientStop; 2] = [
    GradientStop {
        offset: 0.0,
        color: [LIGHT[1][0], LIGHT[1][1], LIGHT[1][2], 0.95],
    },
    GradientStop {
        offset: 1.0,
        color: [LIGHT[1][0], LIGHT[1][1], LIGHT[1][2], 0.1],
    },
];

const PLATE: [GradientStop; 2] = stops2(0x253045, 0x121925);
const DOOR: [GradientStop; 2] = stops2(0x1a2232, 0x2c3850);
const WHOLE: [GradientStop; 2] = stops2(0x253045, 0x141b28);
const SILVER: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: rgb(0xf7fafc),
    },
    GradientStop {
        offset: 0.55,
        color: rgb(0xbccbdb),
    },
    GradientStop {
        offset: 1.0,
        color: rgb(0x8ea4bc),
    },
];
const HEAD: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: rgb(0xfffaf0),
    },
    GradientStop {
        offset: 0.35,
        color: [1.0, 241.0 / 255.0, 201.0 / 255.0, 0.85],
    },
    GradientStop {
        offset: 1.0,
        color: [1.0, 241.0 / 255.0, 201.0 / 255.0, 0.0],
    },
];

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

// ── The figure behind the door ─────────────────────────────────────────

const HD: usize = 0;
const NK: usize = 1;
const SH_L: usize = 2;
const SH_R: usize = 3;
const EL_L: usize = 4;
const EL_R: usize = 5;
const HA_L: usize = 6;
const HA_R: usize = 7;
const WS_L: usize = 8;
const WS_R: usize = 9;
const HP_L: usize = 10;
const HP_R: usize = 11;
const KN_L: usize = 12;
const KN_R: usize = 13;
const FT_L: usize = 14;
const FT_R: usize = 15;

type Pose = [[f32; 3]; 16];

const STAND: Pose = [
    [0.0, -67.0, 0.0],
    [0.0, -44.0, 0.0],
    [-30.0, -30.0, 0.0],
    [30.0, -30.0, 0.0],
    [-38.0, 26.0, 0.0],
    [38.0, 26.0, 0.0],
    [-40.0, 80.0, 0.0],
    [40.0, 80.0, 0.0],
    [-20.0, 40.0, 0.0],
    [20.0, 40.0, 0.0],
    [-17.0, 70.0, 0.0],
    [17.0, 70.0, 0.0],
    [-17.0, 136.0, 0.0],
    [17.0, 136.0, 0.0],
    [-18.0, 196.0, 0.0],
    [18.0, 196.0, 0.0],
];

/// The knee drawn up before the kick.
const CHAMBER: Pose = {
    let mut p = STAND;
    p[HD] = [0.0, -67.0, -14.0];
    p[NK] = [0.0, -44.0, -10.0];
    p[SH_L] = [-30.0, -30.0, -10.0];
    p[SH_R] = [30.0, -30.0, -10.0];
    p[EL_L] = [-56.0, 8.0, -22.0];
    p[EL_R] = [56.0, 8.0, -22.0];
    p[HA_L] = [-72.0, 50.0, -34.0];
    p[HA_R] = [72.0, 50.0, -34.0];
    p[KN_R] = [17.0, 86.0, 50.0];
    p[FT_R] = [18.0, 150.0, 18.0];
    p
};

/// The kick itself.
const KICK: Pose = {
    let mut p = CHAMBER;
    p[HD] = [0.0, -65.0, -26.0];
    p[NK] = [0.0, -42.0, -20.0];
    p[SH_L] = [-30.0, -28.0, -18.0];
    p[SH_R] = [30.0, -28.0, -18.0];
    p[EL_L] = [-62.0, 2.0, -36.0];
    p[EL_R] = [62.0, 2.0, -36.0];
    p[HA_L] = [-84.0, 34.0, -52.0];
    p[HA_R] = [84.0, 34.0, -52.0];
    p[KN_R] = [17.0, 74.0, 30.0];
    p[FT_R] = [17.0, 78.0, 64.0];
    p
};

/// Limbs as thick round strokes: from, to, width. The head is a stroke of
/// zero length.
const BONES: [(usize, usize, f32); 10] = [
    (SH_L, EL_L, 21.0),
    (EL_L, HA_L, 17.0),
    (SH_R, EL_R, 21.0),
    (EL_R, HA_R, 17.0),
    (HP_L, KN_L, 30.0),
    (KN_L, FT_L, 22.0),
    (HP_R, KN_R, 30.0),
    (KN_R, FT_R, 22.0),
    (NK, HD, 18.0),
    (HD, HD, 40.0),
];

const TRUNK: [usize; 6] = [SH_L, SH_R, WS_R, HP_R, HP_L, WS_L];

const FIG_X: f32 = -105.0;
const FIG_Z: f32 = -70.0;

fn lerp_pose(a: &Pose, b: &Pose, k: f32) -> Pose {
    let mut o = *a;
    for (j, joint) in o.iter_mut().enumerate() {
        for (i, v) in joint.iter_mut().enumerate() {
            *v += (b[j][i] - *v) * k;
        }
    }
    o
}

// ── The dust ───────────────────────────────────────────────────────────

struct Mote {
    s: [f32; 3],
    e: [f32; 3],
    d: f32,
    w: f32,
    curl: f32,
}

/// The 40 motes, always the same: a Park–Miller generator from seed 7.
fn motes() -> Vec<Mote> {
    let mut seed: u64 = 7;
    let mut rnd = || {
        seed = (seed * 16807) % 2_147_483_647;
        seed as f32 / 2_147_483_647.0
    };
    (0..40)
        .map(|_| {
            let s = [
                -460.0 + 560.0 * rnd(),
                -170.0 + 360.0 * rnd(),
                60.0 + 360.0 * rnd(),
            ];
            let e = [-185.0 + 165.0 * rnd(), -170.0 + 340.0 * rnd(), -30.0];
            let d = 0.8 * rnd();
            let w = 0.8 + 1.5 * rnd();
            let curl = (rnd() - 0.5) * 120.0;
            Mote { s, e, d, w, curl }
        })
        .collect()
}

/// Two breaths in; each draws the dust in just ahead of the door.
const BREATHS: [(f32, f32); 2] = [(18.3, 1.0), (21.7, 0.65)];

// ── The camera ─────────────────────────────────────────────────────────

/// A world point on the composition, for a camera turned by `th` about the
/// vertical and tilted by `ph`.
fn project(th: f32, ph: f32, [x, y, z]: [f32; 3]) -> [f32; 2] {
    let x1 = x * th.cos() + z * th.sin();
    let z1 = -x * th.sin() + z * th.cos();
    let y2 = y * ph.cos() - z1 * ph.sin();
    let z2 = y * ph.sin() + z1 * ph.cos();
    let s = F / (F - z2);
    [CX + SC * x1 * s, CY + SC * y2 * s]
}

/// Where the light casts a world point on the floor.
fn cast([x, y, z]: [f32; 3]) -> [f32; 3] {
    let s = (FLOOR - LY) / (y - LY);
    [LX + s * (x - LX), FLOOR, LZ + s * (z - LZ)]
}

/// The bounding box of `points`, as `(min, max)`.
fn bbox(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    points.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    )
}

/// A linear gradient across `points`' bounding box, from its top-left corner
/// to `(fx, fy)` of its size (SVG `objectBoundingBox` units).
fn box_linear<'a>(points: &[[f32; 2]], fx: f32, fy: f32, stops: &'a [GradientStop]) -> Fill<'a> {
    let (lo, hi) = bbox(points);
    Fill::Linear {
        from: lo,
        to: [lo[0] + (hi[0] - lo[0]) * fx, lo[1] + (hi[1] - lo[1]) * fy],
        stops,
    }
}

pub(super) fn paint(c: &mut Canvas<'_>, t: f32) {
    let p_grid = ez(t / 1.6);
    let p_out = ez((t - 0.8) / 1.8);
    let p_piece = ez((t - 2.4) / 1.4);
    let light_out = ez((t - 31.0) / 1.6);
    let p_trace = ez((t - 7.6) / 2.0);
    let p_light = ez((t - 9.0) / 1.2) * (1.0 - light_out);
    let p_floor = ez((t - 8.1) / 2.3) * (1.0 - light_out);
    let fade_all = cl((t - 34.4) / 1.0);
    // The door opens with a push and a settle, and closes on its own weight
    // with a small bounce against the frame.
    const HIT: f32 = 12.07;
    let open_in = cl((t - HIT) / 1.5);
    let close_in = cl((t - 27.2) / 2.8);
    let settle = if open_in <= 0.0 {
        0.0
    } else if open_in < 1.0 {
        1.0 - (-5.0 * open_in).exp() * (7.0 * open_in).cos()
    } else {
        1.0
    };
    let shut = if close_in <= 0.0 {
        0.0
    } else if close_in < 0.85 {
        ez(close_in / 0.85)
    } else {
        1.0 - 0.04 * (PI * (close_in - 0.85) / 0.15).sin()
    };
    let p_open = (settle * (1.0 - shut)).max(0.0);
    let draught_on = cl((t - 18.8) / 2.0) * (1.0 - ez((t - 25.4) / 1.6));
    let join = ez(cl((t - 32.6) / 1.3));
    let h = 6.0 * (1.0 - join);
    let a = h * SQRT_2;
    let b = h * (1.0 + SQRT_2);
    let th = (-12.0 + 1.5 * (TAU * t / 26.0).sin()).to_radians();
    let ph = (-13.0_f32).to_radians();
    let proj = |p: [f32; 3]| project(th, ph, p);
    let poly = |q: &[[f32; 2]], z: f32| -> Vec<[f32; 2]> {
        q.iter().map(|&[x, y]| proj([x, y, z])).collect()
    };
    let cast_poly = |q: &[[f32; 2]]| -> Vec<[f32; 2]> {
        q.iter().map(|&[x, y]| proj(cast([x, y, 0.0]))).collect()
    };
    let sq = [
        [-200.0, -200.0],
        [200.0, -200.0],
        [200.0, 200.0],
        [-200.0, 200.0],
    ];

    // The plate's three right-hand pieces, and the gaps between all four.
    let pieces = [
        vec![[h, -200.0], [200.0 - a, -200.0], [h, -b]],
        vec![[a, 0.0], [200.0, -200.0 + a], [200.0, 200.0 - a]],
        vec![[h, 200.0], [h, b], [200.0 - a, 200.0]],
    ];
    let gaps = [
        vec![[-h, -200.0], [h, -200.0], [h, 200.0], [-h, 200.0]],
        vec![[h, -b], [200.0 - a, -200.0], [200.0, -200.0 + a], [a, 0.0]],
        vec![[h, b], [a, 0.0], [200.0, 200.0 - a], [200.0 - a, 200.0]],
        vec![[h, -b], [a, 0.0], [h, b]],
    ];

    // The door, on its hinge at the plate's left edge; the draught pulls it.
    let draught = draught_on
        * (0.5 - 0.5 * (TAU * (t - 18.8) / 3.4).cos())
        * (1.0 - 0.35 * cl((t - 22.2) / 0.6));
    let al = (p_open * (76.0 - 38.0 * draught)).to_radians();
    let w_door = 200.0 - h;
    let dp = |w: f32, y: f32, zl: f32| {
        [
            -200.0 + w * al.cos() - zl * al.sin(),
            y,
            w * al.sin() + zl * al.cos(),
        ]
    };
    let door3 = [
        dp(0.0, -200.0, 0.0),
        dp(w_door, -200.0, 0.0),
        dp(w_door, 200.0, 0.0),
        dp(0.0, 200.0, 0.0),
    ];
    let door: Vec<[f32; 2]> = door3.iter().map(|&p| proj(p)).collect();
    let door_edge: Vec<[f32; 2]> = [
        dp(w_door, -200.0, 0.0),
        dp(w_door, -200.0, -6.0),
        dp(w_door, 200.0, -6.0),
        dp(w_door, 200.0, 0.0),
    ]
    .iter()
    .map(|&p| proj(p))
    .collect();
    let doorway_pts = [[-200.0, -200.0], [-h, -200.0], [-h, 200.0], [-200.0, 200.0]];
    let doorway = poly(&doorway_pts, 0.0);
    let patch = cast_poly(&doorway_pts);

    // The kicker: knee up, the kick, the leg back; a moment standing in the
    // light, then the walk back into it.
    let kc = ez(cl((t - 11.3) / 0.55));
    let kx = eo(cl((t - 11.85) / 0.22), 3.0);
    let kb = ez(cl((t - 12.3) / 0.6));
    let away = ez(cl((t - 13.7) / 3.2));
    let mut pose = lerp_pose(
        &lerp_pose(&lerp_pose(&STAND, &CHAMBER, kc), &KICK, kx),
        &STAND,
        kb,
    );
    let stride = (away * PI * 4.0).sin() * 22.0 * (PI * away).sin();
    pose[FT_L][2] += stride;
    pose[KN_L][2] += stride * 0.5;
    pose[FT_R][2] -= stride;
    pose[KN_R][2] -= stride * 0.5;
    let joint = |j: usize, dy: f32| {
        [
            FIG_X + pose[j][0],
            pose[j][1] + dy,
            FIG_Z - 300.0 * away + pose[j][2],
        ]
    };
    let fig_on = if t > 11.2 && t < 17.1 {
        1.0 - ez(cl((t - 14.1) / 2.8))
    } else {
        0.0
    };
    // How long a unit along x is, on screen, at a world point.
    let px_at = |q: [f32; 3]| {
        let (p0, p1) = (proj(q), proj([q[0] + 1.0, q[1], q[2]]));
        ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt()
    };

    let o2 = proj([0.0, 0.0, 0.0]);
    let tips: Vec<[f32; 2]> = [[0.0, -200.0], [0.0, 200.0], [200.0, -200.0], [200.0, 200.0]]
        .iter()
        .map(|&[x, y]| proj([x, y, 0.0]))
        .collect();
    let gc = proj([-100.0, -40.0, -2.0]);
    let glow = Fill::Radial {
        center: gc,
        radius: 320.0,
        stops: &GLOW,
    };
    let near = proj([0.0, FLOOR, 0.0]);
    let far = proj(cast([0.0, -200.0, 0.0]));
    let floor_fill = Fill::Linear {
        from: [0.0, near[1]],
        to: [0.0, far[1]],
        stops: &FLOOR_LIGHT,
    };

    let lit = cl(p_open * 4.0);
    let placed = if p_piece >= 1.0 { 1.0 } else { 0.0 };
    let rim_on = p_light * cl(p_open * 8.0);
    let edges = ez(cl((t - 7.6) / 2.0));
    let unit = {
        let ou = proj([1.0, 0.0, 0.0]);
        ((ou[0] - o2[0]).powi(2) + (ou[1] - o2[1]).powi(2)).sqrt()
    };
    let whole = 1.0 - ez(cl((t - 9.7) / 1.0));
    let plate = poly(&sq, 0.0);

    c.group(1.0 - fade_all, |c| {
        // The floor grid.
        c.group(p_grid * 0.9, |c| {
            let mut x = -480.0;
            while x <= 480.0 {
                let o = if x == 0.0 { 0.32 } else { 0.16 };
                c.line(
                    proj([x, FLOOR, -240.0]),
                    proj([x, FLOOR, 420.0]),
                    0.5,
                    fade(GRID, o),
                    false,
                );
                x += 80.0;
            }
            let mut z = -240.0;
            while z <= 420.0 {
                let o = if z == 0.0 { 0.32 } else { 0.16 };
                c.line(
                    proj([-480.0, FLOOR, z]),
                    proj([480.0, FLOOR, z]),
                    0.5,
                    fade(GRID, o),
                    false,
                );
                z += 60.0;
            }
        });
        // The plate's shadow, and the light it lets through.
        c.group(0.55 * p_floor, |c| {
            c.poly(&cast_poly(&sq), Fill::Solid(SHADOW))
        });
        c.group(0.9 * p_light * lit, |c| c.poly(&patch, floor_fill));
        c.group(0.9 * p_floor, |c| {
            for gap in &gaps {
                c.poly(&cast_poly(gap), floor_fill);
            }
        });
        // The figure's shadow, inside the lit patch.
        c.group(0.8 * fig_on * p_light * lit, |c| {
            c.clipped(&patch, |c| {
                for &(ja, jb, w) in &BONES {
                    let p1 = joint(ja, 0.0);
                    let p2 = joint(jb, if ja == jb { 0.5 } else { 0.0 });
                    let fk = px_at(cast(p1));
                    c.line(proj(cast(p1)), proj(cast(p2)), w * 1.1 * fk, SHADOW, true);
                }
                let trunk: Vec<[f32; 2]> =
                    TRUNK.iter().map(|&j| proj(cast(joint(j, 0.0)))).collect();
                let tw = 10.0 * px_at(cast(joint(HP_L, 0.0)));
                c.poly(&trunk, Fill::Solid(SHADOW));
                stroke_round(c, &trunk, tw, SHADOW);
            });
        });
        let door_shadow: Vec<[f32; 2]> = door3.iter().map(|&p| proj(cast(p))).collect();
        c.group(0.85 * p_light * lit, |c| {
            c.poly(&door_shadow, Fill::Solid(SHADOW))
        });
        c.group(0.18, |c| c.outline(&poly(&sq, -6.0), 0.5, EDGE));
        // The light behind the plate.
        c.group(p_light, |c| c.poly(&poly(&sq, -2.0), glow));
        c.group(placed * (1.0 - p_light), |c| {
            c.poly(&poly(&sq, -1.0), Fill::Solid(rgb(0x1b2333)))
        });
        // The light running out along the cuts, inside the plate.
        if t >= 7.6 {
            c.group(1.0 - light_out, |c| {
                c.clipped(&plate, |c| {
                    for &tip in &tips {
                        let end = [
                            o2[0] + (tip[0] - o2[0]) * p_trace,
                            o2[1] + (tip[1] - o2[1]) * p_trace,
                        ];
                        c.poly(&bar(o2, end, 16.0), glow);
                    }
                });
            });
        }
        // The figure in the doorway, backlit.
        c.group(fig_on, |c| {
            c.clipped(&doorway, |c| {
                let k0 = px_at(joint(HP_L, 0.0));
                for (ink, extra, alpha) in [(rgb(0xfff7e2), 5.0, 0.55), (rgb(0x0a0e15), 0.0, 1.0)] {
                    c.group(alpha, |c| {
                        for &(ja, jb, w) in &BONES {
                            let p1 = joint(ja, 0.0);
                            let p2 = joint(jb, if ja == jb { 0.5 } else { 0.0 });
                            let k = px_at(p1);
                            c.line(proj(p1), proj(p2), w * k + extra, ink, true);
                        }
                        let trunk: Vec<[f32; 2]> =
                            TRUNK.iter().map(|&j| proj(joint(j, 0.0))).collect();
                        c.poly(&trunk, Fill::Solid(ink));
                        stroke_round(c, &trunk, 10.0 * k0 + extra, ink);
                    });
                }
            });
        });
        // The three fixed pieces, the door's gold edge, the door.
        c.group(placed, |c| {
            for piece in &pieces {
                let pts = poly(piece, 0.0);
                c.poly(&pts, box_linear(&pts, 1.0, 1.0, &PLATE));
                c.group(0.38 * edges * (1.0 - join), |c| c.outline(&pts, 0.6, EDGE));
            }
        });
        c.group(rim_on, |c| {
            c.poly(&door_edge, Fill::Solid(fade(GOLD, 0.85)))
        });
        c.group(placed, |c| {
            c.poly(&door, box_linear(&door, 1.0, 0.0, &DOOR));
            c.group(0.45 * edges * (1.0 - join), |c| c.outline(&door, 0.6, EDGE));
        });
        c.group(rim_on * 0.95, |c| {
            c.line(door[1], door[2], 1.0, rgb(0xf7d985), false)
        });
        // The plate whole, before the cut and after the pieces close up.
        let cover = (p_piece * whole).max(ez(cl((t - 33.4) / 0.6)));
        c.group(cover, |c| {
            c.poly(&plate, box_linear(&plate, 1.0, 1.0, &WHOLE))
        });
        // The light drawing the K on top of the whole plate.
        if t >= 7.6 && whole > 0.0 {
            c.clipped(&plate, |c| {
                for &tip in &tips {
                    let end = [
                        o2[0] + (tip[0] - o2[0]) * p_trace,
                        o2[1] + (tip[1] - o2[1]) * p_trace,
                    ];
                    c.poly(&bar(o2, end, 2.0 * h * unit), glow);
                }
            });
        }
        c.group(0.55, |c| c.outline_part(&plate, p_out, 0.6, EDGE));
        // The hole: it opens dark, the light wells up in it, and it hands over
        // to the trace, which is the same light.
        let hole_gone = 1.0 - cl((t - 9.6) / 0.3);
        let fill = ez(cl((t - 5.3) / 1.3));
        if t > 3.9 && fill < 1.0 {
            let r = h * unit * ez(cl((t - 3.9) / 1.1));
            c.circle(
                o2,
                r,
                Fill::Radial {
                    center: [o2[0] - r + 0.8 * r, o2[1] - r + 0.7 * r],
                    radius: 1.4 * r,
                    stops: &SILVER,
                },
            );
        }
        if fill > 0.0 {
            c.group(hole_gone, |c| c.circle(o2, h * unit * fill, glow));
        }
        // The dust: drawn in faster and faster, it streaks, curls, and vanishes
        // into the light as it crosses the doorway.
        for m in motes() {
            let mut f = -1.0;
            let mut k = 0.0;
            for (g, w) in BREATHS {
                let u = (t - g - m.d) / 1.8;
                if u > 0.0 && u < 1.0 {
                    f = u;
                    k = w;
                }
            }
            if f < 0.0 {
                continue;
            }
            let at = |u: f32| {
                let v = u * u;
                let curl = (PI * v).sin() * m.curl;
                proj([
                    m.s[0] + (m.e[0] - m.s[0]) * v + 0.3 * curl,
                    m.s[1] + (m.e[1] - m.s[1]) * v,
                    m.s[2] + (m.e[2] - m.s[2]) * v + curl,
                ])
            };
            let alpha = k * (PI * f).sin().powf(0.6) * (1.0 - light_out);
            c.group(alpha, |c| {
                c.line(at(f), at((f - 0.06).max(0.0)), m.w, rgb(0xfff1c9), true)
            });
        }
        // The light's heads, running out along the cuts.
        let bloom = ez(cl((t - 7.0) / 1.1));
        c.group(bloom * (1.0 - ez((t - 8.9) / 0.7)), |c| {
            let r = 13.0 * (0.35 + 0.65 * bloom);
            for &tip in &tips {
                let at = [
                    o2[0] + (tip[0] - o2[0]) * p_trace,
                    o2[1] + (tip[1] - o2[1]) * p_trace,
                ];
                c.circle(
                    at,
                    r,
                    Fill::Radial {
                        center: at,
                        radius: r,
                        stops: &HEAD,
                    },
                );
            }
        });
    });

    // The name.
    c.group(ez((t - 9.7) / 1.8) * (1.0 - fade_all), |c| {
        c.tracked_text(
            "KHORA",
            720.0,
            802.0,
            Lettering {
                size: 34.0,
                tracking: 21.0,
                family: FontFamilyHint::Wordmark,
                color: INK,
            },
        );
        c.tracked_text(
            "ΧΩΡΑ",
            720.0,
            834.0,
            Lettering {
                size: 11.0,
                tracking: 5.5,
                family: FontFamilyHint::Monospace,
                color: GRID,
            },
        );
    });
}

/// The outline of `points` with round joins: its edges as round-capped
/// strokes.
fn stroke_round(c: &mut Canvas<'_>, points: &[[f32; 2]], width: f32, color: [f32; 4]) {
    let n = points.len();
    for i in 0..n {
        c.line(points[i], points[(i + 1) % n], width, color, true);
    }
}
