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

//! De-risking benchmark for adaptive memory layout (AGDF).
//!
//! Measures a representative hot kernel — N-body-style position integration
//! `p += v·dt` over `N` elements, repeated `K` times — under three physical
//! layouts (AoS, SoA, AoSoA), then lets the engine's [`Ucb1`] bandit pick the
//! fastest from REAL measurements on this machine.
//!
//! Purpose: decide whether a layout-polymorphic CRPECS storage refactor is
//! worth it *before* committing to it. Run with:
//!
//! ```text
//! cargo run -p khora-data --example layout_bench --release
//! ```
//!
//! This is a standalone proof-of-concept; it does not touch the engine's
//! storage or query path.

// Index-based loops are intentional here: they mirror how each physical layout
// is actually traversed, which is the whole point of the comparison.
#![allow(clippy::needless_range_loop)]

use std::hint::black_box;
use std::time::Instant;

use khora_core::math::simd::{
    compose_trs_to_mat4, compose_trs_to_mat4_scalar, normalize_quat_batch, TrsBatchSoa,
};
use khora_core::math::{Mat4, Quaternion, Vec3};
use khora_data::ecs::layout::Ucb1;
use khora_data::ecs::{Camera, Collider, GlobalTransform, RigidBody, Transform};

const N: usize = 1 << 16; // 65 536 elements
const LANES: usize = 8;
const K: usize = 400; // integration steps per measured run
const DT: f32 = 0.016;
/// Cold bytes per element in the "fat component" scenario (hot = 24 B of
/// pos+vel; total record ≈ 128 B = two cache lines).
const COLD_BYTES: usize = 104;

/// Array-of-Structures: each element is a contiguous 6-float record.
fn run_aos() -> f64 {
    let mut data: Vec<[f32; 6]> = (0..N)
        .map(|i| {
            let f = i as f32;
            [f, f * 0.5, f * 0.25, 1.0, 0.5, 0.25]
        })
        .collect();
    let start = Instant::now();
    for _ in 0..K {
        for e in data.iter_mut() {
            e[0] += e[3] * DT;
            e[1] += e[4] * DT;
            e[2] += e[5] * DT;
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(data[N - 1][0]);
    ms
}

/// Structure-of-Arrays: one contiguous array per field (today's CRPECS layout).
fn run_soa() -> f64 {
    let mut px: Vec<f32> = (0..N).map(|i| i as f32).collect();
    let mut py: Vec<f32> = (0..N).map(|i| i as f32 * 0.5).collect();
    let mut pz: Vec<f32> = (0..N).map(|i| i as f32 * 0.25).collect();
    let vx = vec![1.0f32; N];
    let vy = vec![0.5f32; N];
    let vz = vec![0.25f32; N];
    let start = Instant::now();
    for _ in 0..K {
        for i in 0..N {
            px[i] += vx[i] * DT;
            py[i] += vy[i] * DT;
            pz[i] += vz[i] * DT;
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(px[N - 1]);
    ms
}

/// Array-of-Structures-of-Arrays: tiles of `LANES` lanes per field.
fn run_aosoa() -> f64 {
    let tiles = N / LANES;
    // Per tile: 6 fields × LANES lanes.
    let mut data: Vec<[[f32; LANES]; 6]> = (0..tiles)
        .map(|t| {
            let mut tile = [[0.0f32; LANES]; 6];
            for l in 0..LANES {
                let i = (t * LANES + l) as f32;
                tile[0][l] = i;
                tile[1][l] = i * 0.5;
                tile[2][l] = i * 0.25;
                tile[3][l] = 1.0;
                tile[4][l] = 0.5;
                tile[5][l] = 0.25;
            }
            tile
        })
        .collect();
    let start = Instant::now();
    for _ in 0..K {
        for tile in data.iter_mut() {
            for l in 0..LANES {
                tile[0][l] += tile[3][l] * DT;
                tile[1][l] += tile[4][l] * DT;
                tile[2][l] += tile[5][l] * DT;
            }
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(data[tiles - 1][0][LANES - 1]);
    ms
}

/// FAT component stored interleaved — today's CRPECS column is `Vec<Component>`,
/// so iterating a few hot fields drags every element's cold bytes through cache.
fn run_fat_interleaved() -> f64 {
    #[derive(Clone, Copy)]
    struct Fat {
        hot: [f32; 6],
        _cold: [u8; COLD_BYTES],
    }
    let mut data: Vec<Fat> = (0..N)
        .map(|i| {
            let f = i as f32;
            Fat {
                hot: [f, f * 0.5, f * 0.25, 1.0, 0.5, 0.25],
                _cold: [0u8; COLD_BYTES],
            }
        })
        .collect();
    let start = Instant::now();
    for _ in 0..K {
        for e in data.iter_mut() {
            e.hot[0] += e.hot[3] * DT;
            e.hot[1] += e.hot[4] * DT;
            e.hot[2] += e.hot[5] * DT;
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(data[N - 1].hot[0]);
    ms
}

/// Same data, hot/cold **split**: hot fields in their own contiguous column,
/// cold bytes parked elsewhere and never touched by this iteration.
fn run_fat_split() -> f64 {
    let mut hot: Vec<[f32; 6]> = (0..N)
        .map(|i| {
            let f = i as f32;
            [f, f * 0.5, f * 0.25, 1.0, 0.5, 0.25]
        })
        .collect();
    let cold: Vec<[u8; COLD_BYTES]> = vec![[0u8; COLD_BYTES]; N];
    let start = Instant::now();
    for _ in 0..K {
        for h in hot.iter_mut() {
            h[0] += h[3] * DT;
            h[1] += h[4] * DT;
            h[2] += h[5] * DT;
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(hot[N - 1][0]);
    black_box(cold.len());
    ms
}

/// Compute-heavy kernel (vector normalize + accumulate) in SoA — tests whether
/// AoSoA's edge over SoA grows when the bottleneck is *compute*, not memory.
fn run_soa_heavy() -> f64 {
    let mut x: Vec<f32> = (0..N).map(|i| (i % 97) as f32 + 1.0).collect();
    let mut y: Vec<f32> = (0..N).map(|i| (i % 89) as f32 + 1.0).collect();
    let mut z: Vec<f32> = (0..N).map(|i| (i % 83) as f32 + 1.0).collect();
    let mut acc = 0.0f32;
    let start = Instant::now();
    for _ in 0..K {
        for i in 0..N {
            let inv = 1.0 / (x[i] * x[i] + y[i] * y[i] + z[i] * z[i]).sqrt();
            x[i] *= inv;
            y[i] *= inv;
            z[i] *= inv;
            acc += x[i] - y[i] + z[i];
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(acc);
    ms
}

/// Same compute-heavy kernel over AoSoA tiles.
fn run_aosoa_heavy() -> f64 {
    let tiles = N / LANES;
    let mut data: Vec<[[f32; LANES]; 3]> = (0..tiles)
        .map(|t| {
            let mut tile = [[0.0f32; LANES]; 3];
            for l in 0..LANES {
                let i = t * LANES + l;
                tile[0][l] = (i % 97) as f32 + 1.0;
                tile[1][l] = (i % 89) as f32 + 1.0;
                tile[2][l] = (i % 83) as f32 + 1.0;
            }
            tile
        })
        .collect();
    let mut acc = 0.0f32;
    let start = Instant::now();
    for _ in 0..K {
        for tile in data.iter_mut() {
            for l in 0..LANES {
                let inv = 1.0
                    / (tile[0][l] * tile[0][l] + tile[1][l] * tile[1][l] + tile[2][l] * tile[2][l])
                        .sqrt();
                tile[0][l] *= inv;
                tile[1][l] *= inv;
                tile[2][l] *= inv;
                acc += tile[0][l] - tile[1][l] + tile[2][l];
            }
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(acc);
    ms
}

/// Same compute-heavy kernel with EXPLICIT SIMD (`wide::f32x8`) over SoA — tests
/// how far past auto-vectorized AoSoA an explicit-SIMD path can go.
fn run_simd_heavy() -> f64 {
    use wide::f32x8;
    let mut x: Vec<f32> = (0..N).map(|i| (i % 97) as f32 + 1.0).collect();
    let mut y: Vec<f32> = (0..N).map(|i| (i % 89) as f32 + 1.0).collect();
    let mut z: Vec<f32> = (0..N).map(|i| (i % 83) as f32 + 1.0).collect();
    let mut acc = f32x8::splat(0.0);
    let start = Instant::now();
    for _ in 0..K {
        let xi = x.chunks_exact_mut(LANES);
        let yi = y.chunks_exact_mut(LANES);
        let zi = z.chunks_exact_mut(LANES);
        for ((cx, cy), cz) in xi.zip(yi).zip(zi) {
            let xv = f32x8::new((&*cx).try_into().unwrap());
            let yv = f32x8::new((&*cy).try_into().unwrap());
            let zv = f32x8::new((&*cz).try_into().unwrap());
            let inv = f32x8::splat(1.0) / (xv * xv + yv * yv + zv * zv).sqrt();
            let xn = xv * inv;
            let yn = yv * inv;
            let zn = zv * inv;
            cx.copy_from_slice(&xn.to_array());
            cy.copy_from_slice(&yn.to_array());
            cz.copy_from_slice(&zn.to_array());
            acc += xn - yn + zn;
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(acc.reduce_add());
    ms
}

/// Builds a representative field-SoA batch of `N` TRS inputs (varied rotation,
/// translation, and non-uniform scale) — the input to the real engine kernel.
fn trs_batch() -> TrsBatchSoa {
    let mut b = TrsBatchSoa::with_capacity(N);
    for i in 0..N {
        let f = i as f32;
        let axis = Vec3::new(0.3 + (i % 7) as f32, 1.0, 0.5 + (i % 5) as f32).normalize();
        let q = Quaternion::from_axis_angle(axis, 0.01 * f + 0.3);
        b.push(
            [f, f * 0.5, f * 0.25],
            [q.x, q.y, q.z, q.w],
            [1.0 + (i % 3) as f32 * 0.1, 1.0, 0.75],
        );
    }
    b
}

/// Times `K` passes of the SHIPPED scalar kernel over a prebuilt batch.
fn run_engine_trs_scalar(batch: &TrsBatchSoa, out: &mut [Mat4]) -> f64 {
    let start = Instant::now();
    for _ in 0..K {
        compose_trs_to_mat4_scalar(batch, out);
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(out[N - 1].cols[0].x);
    ms
}

/// Times `K` passes of the SHIPPED `wide::f32x8` kernel over a prebuilt batch.
fn run_engine_trs_simd(batch: &TrsBatchSoa, out: &mut [Mat4]) -> f64 {
    let start = Instant::now();
    for _ in 0..K {
        compose_trs_to_mat4(batch, out);
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(out[N - 1].cols[0].x);
    ms
}

/// Scalar in-place quaternion normalize baseline (the loop auto-vec refuses to
/// lane, because the `x²+y²+z²+w²` reduction is a non-associative FP sum).
fn run_scalar_normalize(qx: &mut [f32], qy: &mut [f32], qz: &mut [f32], qw: &mut [f32]) -> f64 {
    let start = Instant::now();
    for _ in 0..K {
        for i in 0..qx.len() {
            let len_sq = qx[i] * qx[i] + qy[i] * qy[i] + qz[i] * qz[i] + qw[i] * qw[i];
            if len_sq > f32::MIN_POSITIVE {
                let inv = 1.0 / len_sq.sqrt();
                qx[i] *= inv;
                qy[i] *= inv;
                qz[i] *= inv;
                qw[i] *= inv;
            }
        }
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(qx[qx.len() - 1]);
    ms
}

/// Times `K` passes of the SHIPPED in-place `wide::f32x8` normalize kernel.
fn run_engine_normalize_simd(
    qx: &mut [f32],
    qy: &mut [f32],
    qz: &mut [f32],
    qw: &mut [f32],
) -> f64 {
    let start = Instant::now();
    for _ in 0..K {
        normalize_quat_batch(qx, qy, qz, qw);
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    black_box(qx[qx.len() - 1]);
    ms
}

fn main() {
    const ARMS: [&str; 3] = ["AoS", "SoA", "AoSoA"];
    let run = |arm: usize| -> f64 {
        match arm {
            0 => run_aos(),
            1 => run_soa(),
            _ => run_aosoa(),
        }
    };

    // Warm up (let the CPU clock up and caches settle).
    for arm in 0..3 {
        black_box(run(arm));
    }

    // Baseline: best-of-5 per layout (median-ish, robust to jitter).
    println!("Layout integration kernel — N={N}, K={K} steps, {LANES} lanes\n");
    let mut baseline = [f64::MAX; 3];
    for (arm, name) in ARMS.iter().enumerate() {
        for _ in 0..5 {
            baseline[arm] = baseline[arm].min(run(arm));
        }
        println!("  {name:<6} best: {:.3} ms", baseline[arm]);
    }

    // Drive the engine's UCB1 bandit on REAL measurements. Reward = throughput
    // (1 / elapsed): faster layouts earn more, so the bandit must converge to
    // the fastest one on THIS machine.
    let mut bandit = Ucb1::new(3);
    for _ in 0..60 {
        let arm = bandit.select();
        let ms = run(arm);
        bandit.record(arm, 1.0 / ms);
    }

    println!("\nUCB1 bandit (reward = 1/ms, 60 rounds):");
    for (arm, name) in ARMS.iter().enumerate() {
        println!(
            "  {name:<6} pulls={:<3} mean_reward={:.4}",
            bandit.pulls(arm),
            bandit.mean_reward(arm).unwrap_or(0.0)
        );
    }
    let best = bandit.best_arm().unwrap();
    println!("\n  → bandit picked: {}", ARMS[best]);

    let fastest = (0..3)
        .min_by(|&a, &b| baseline[a].total_cmp(&baseline[b]))
        .unwrap();
    println!("  → baseline fastest: {}", ARMS[fastest]);
    let speedup = baseline[0] / baseline[fastest];
    println!(
        "  → fastest is {:.2}× faster than AoS — but CRPECS is already SoA, so the",
        speedup
    );
    println!(
        "    relevant gain (AoSoA vs SoA) is only {:.2}×",
        baseline[1] / baseline[2]
    );

    // The real lever: hot/cold splitting on a FAT component. The win scales with
    // the cold/hot byte ratio, so it dwarfs the lean-component AoSoA edge above.
    println!(
        "\nFat component ({} B total, {} B hot) — iterate hot fields only:",
        24 + COLD_BYTES,
        24
    );
    let fat_i = (0..5)
        .map(|_| run_fat_interleaved())
        .fold(f64::MAX, f64::min);
    let fat_s = (0..5).map(|_| run_fat_split()).fold(f64::MAX, f64::min);
    println!("  interleaved (current CRPECS Vec<Component>): {fat_i:.3} ms");
    println!("  hot/cold split (hot column only):            {fat_s:.3} ms");
    println!(
        "  → split is {:.2}× faster — but only because this component is FAT.",
        fat_i / fat_s
    );

    // Reality check: how fat are Khora's ACTUAL built-in components?
    println!("\nReal Khora component sizes (size_of):");
    println!(
        "  Transform        {:>4} B",
        std::mem::size_of::<Transform>()
    );
    println!(
        "  GlobalTransform  {:>4} B",
        std::mem::size_of::<GlobalTransform>()
    );
    println!(
        "  RigidBody        {:>4} B",
        std::mem::size_of::<RigidBody>()
    );
    println!(
        "  Collider         {:>4} B",
        std::mem::size_of::<Collider>()
    );
    println!("  Camera           {:>4} B", std::mem::size_of::<Camera>());
    println!("  (synthetic Fat   {:>4} B)", 24 + COLD_BYTES);
    println!("  → built-ins are lean & mostly hot: hot/cold split helps THEM little.");
    println!("    The 2× lever is for FAT *user* gameplay components → it's a macro TOOL.");

    // Compute-heavy kernel: does AoSoA beat SoA more when the bottleneck is math?
    println!("\nCompute-heavy kernel (normalize + accumulate):");
    let soa_h = (0..5).map(|_| run_soa_heavy()).fold(f64::MAX, f64::min);
    let aosoa_h = (0..5).map(|_| run_aosoa_heavy()).fold(f64::MAX, f64::min);
    let simd_h = (0..5).map(|_| run_simd_heavy()).fold(f64::MAX, f64::min);
    println!("  SoA (scalar)        {soa_h:.3} ms");
    println!(
        "  AoSoA (auto-vec)    {aosoa_h:.3} ms  ({:.2}× vs SoA)",
        soa_h / aosoa_h
    );
    println!(
        "  SoA + explicit SIMD {simd_h:.3} ms  ({:.2}× vs SoA)",
        soa_h / simd_h
    );
    println!("  → explicit SIMD (wide::f32x8) is the ceiling for compute-heavy hot loops.");

    // The SHIPPED engine kernels (`khora_core::math::simd`). The contrast below
    // is the single most important practical lesson for AGDF: explicit SIMD pays
    // only when the data STAYS field-SoA-resident end to end. The same f32x8
    // math wins big in-place but LOSES once it must scatter back to an AoS
    // result — so the layout win is a whole-pipeline property, not a per-kernel
    // one (which is exactly why persistent SoA storage, not a one-off transpose,
    // is the real future lever).
    println!("\nEngine kernels (khora_core::math::simd) — resident vs scatter:");

    // (a) In-place quaternion normalize: input AND output stay SoA. SIMD wins.
    let mut qx: Vec<f32> = (0..N).map(|i| (i % 97) as f32 + 1.0).collect();
    let mut qy: Vec<f32> = (0..N).map(|i| (i % 89) as f32 + 1.0).collect();
    let mut qz: Vec<f32> = (0..N).map(|i| (i % 83) as f32 + 1.0).collect();
    let mut qw: Vec<f32> = (0..N).map(|i| (i % 79) as f32 + 1.0).collect();
    for _ in 0..3 {
        black_box(run_engine_normalize_simd(
            &mut qx, &mut qy, &mut qz, &mut qw,
        ));
    }
    let nrm_scalar = (0..5)
        .map(|_| run_scalar_normalize(&mut qx, &mut qy, &mut qz, &mut qw))
        .fold(f64::MAX, f64::min);
    let nrm_simd = (0..5)
        .map(|_| run_engine_normalize_simd(&mut qx, &mut qy, &mut qz, &mut qw))
        .fold(f64::MAX, f64::min);
    println!(
        "  normalize_quat_batch (in-place SoA):  scalar {nrm_scalar:.3} ms / f32x8 {nrm_simd:.3} ms  ({:.2}× — WIN)",
        nrm_scalar / nrm_simd
    );

    // (b) TRS→Mat4 compose: SoA input, AoS `Mat4` output. The per-lane
    // transpose-back dominates the cheap quaternion math → SIMD loses.
    let batch = trs_batch();
    let mut out = vec![Mat4::IDENTITY; N];
    for _ in 0..3 {
        black_box(run_engine_trs_simd(&batch, &mut out));
    }
    let trs_scalar = (0..5)
        .map(|_| run_engine_trs_scalar(&batch, &mut out))
        .fold(f64::MAX, f64::min);
    let trs_simd = (0..5)
        .map(|_| run_engine_trs_simd(&batch, &mut out))
        .fold(f64::MAX, f64::min);
    println!(
        "  compose_trs_to_mat4 (SoA→AoS Mat4):   scalar {trs_scalar:.3} ms / f32x8 {trs_simd:.3} ms  ({:.2}× — scatter-bound, scalar wins)",
        trs_scalar / trs_simd
    );
    println!(
        "  → adopt the SIMD kernel only for SoA-resident loops; never for a one-off transpose."
    );
}
