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

//! The figure behind the door in "La porte": its poses, from standing to the
//! kick and the walk back into the light, and its silhouette.

use std::f32::consts::PI;

use super::canvas::{Canvas, Fill};
use super::{cl, eo, ez};

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

/// Returns where the figure's joint `j` stands at `t`, in world space, lifted
/// by `dy`.
pub(super) fn figure_at(t: f32) -> impl Fn(usize, f32) -> [f32; 3] + Copy {
    let (pose, away) = figure_pose(t);
    move |j: usize, dy: f32| {
        [
            FIG_X + pose[j][0],
            pose[j][1] + dy,
            FIG_Z - 300.0 * away + pose[j][2],
        ]
    }
}

/// Paints the figure's limbs and trunk in `ink`, each limb `scale` times its
/// width plus `extra` pixels.
///
/// `px_at` gives the screen length of a world unit at a point, `proj` the
/// screen point of a world point.
pub(super) fn paint_silhouette(
    c: &mut Canvas<'_>,
    joint: impl Fn(usize, f32) -> [f32; 3],
    px_at: impl Fn([f32; 3]) -> f32,
    proj: impl Fn([f32; 3]) -> [f32; 2],
    ink: [f32; 4],
    scale: f32,
    extra: f32,
) {
    for &(ja, jb, w) in &BONES {
        let p1 = joint(ja, 0.0);
        let p2 = joint(jb, if ja == jb { 0.5 } else { 0.0 });
        let k = px_at(p1);
        c.line(proj(p1), proj(p2), w * scale * k + extra, ink, true);
    }
    let trunk: Vec<[f32; 2]> = TRUNK.iter().map(|&j| proj(joint(j, 0.0))).collect();
    let tw = 10.0 * px_at(joint(HP_L, 0.0)) + extra;
    c.poly(&trunk, Fill::Solid(ink));
    stroke_round(c, &trunk, tw, ink);
}

/// Returns the kicker's pose at `t`, and how far it has walked back into the
/// light, from 0 to 1.
///
/// Knee up, the kick, the leg back; a moment standing in the light, then the
/// walk back into it.
fn figure_pose(t: f32) -> (Pose, f32) {
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
    (pose, away)
}

fn lerp_pose(a: &Pose, b: &Pose, k: f32) -> Pose {
    let mut o = *a;
    for (j, joint) in o.iter_mut().enumerate() {
        for (i, v) in joint.iter_mut().enumerate() {
            *v += (b[j][i] - *v) * k;
        }
    }
    o
}

/// Strokes the outline of `points` with round joins: its edges as round-capped
/// strokes.
fn stroke_round(c: &mut Canvas<'_>, points: &[[f32; 2]], width: f32, color: [f32; 4]) {
    let n = points.len();
    for i in 0..n {
        c.line(points[i], points[(i + 1) % n], width, color, true);
    }
}
