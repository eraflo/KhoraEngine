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

//! Explicit-SIMD batch kernels for transform math — the AGDF *compute-heavy hot
//! loop* lever.
//!
//! A component column is stored as an array of whole structs (AoS-within-the-
//! component), which is fine for random access but leaves SIMD lanes empty on a
//! batched math sweep: a `Vec3`/`Quaternion` only fills part of a vector
//! register. The lever is to feed the heavy loops *field-split* data — every
//! field its own contiguous `f32` stream ([`TrsBatchSoa`]) — and process eight
//! entities at a time with [`wide::f32x8`]. `f32x8` maps 1:1 to an 8-wide AoSoA
//! tile, so the index split is trivial and the compiler emits one aligned vector
//! op per field instead of eight scalar ones.
//!
//! Why *explicit* SIMD rather than trusting auto-vectorisation: the heavy parts
//! of this math are a quaternion-to-matrix expansion and a `1/√(x²+y²+z²+w²)`
//! normalisation. The normalisation's horizontal sum is a floating-point
//! reduction, and FP addition is not associative — the compiler is not allowed
//! to reorder it into independent lanes, so it serialises and the loop stays
//! scalar. Writing the lanes by hand recovers the throughput the auto-vectoriser
//! leaves on the table.
//!
//! The kernels are layout-agnostic in the LLAMA sense: callers fill a
//! [`TrsBatchSoa`] from whatever their storage is, and the kernel owns the
//! physical f32x8 tiling underneath. Each kernel has a scalar twin (used for the
//! ragged tail and as the equivalence oracle in tests) so the SIMD path is
//! guaranteed to match the scalar result to the last representable bit of the
//! same operations.

use wide::{f32x8, CmpGt};

use super::{Mat4, Vec4};

/// SIMD lane width. Eight `f32` lanes = one 256-bit register (AVX) and the
/// AoSoA tile width the [`TrsBatchSoa`] kernels stride by.
pub const LANES: usize = 8;

/// A field-split (struct-of-arrays) batch of translate/rotate/scale inputs.
///
/// Each field is its own contiguous `f32` stream so a kernel can load eight
/// entities of one field into a single [`f32x8`]. Fill it from any source
/// (e.g. an ECS `Transform` column) with [`push`](Self::push); the engine's
/// component types stay in the data crate, this buffer speaks only `f32`.
#[derive(Debug, Default, Clone)]
pub struct TrsBatchSoa {
    /// Translation X / Y / Z, one entry per entity.
    pub tx: Vec<f32>,
    /// Translation Y.
    pub ty: Vec<f32>,
    /// Translation Z.
    pub tz: Vec<f32>,
    /// Rotation quaternion X / Y / Z / W, one entry per entity.
    pub qx: Vec<f32>,
    /// Rotation quaternion Y.
    pub qy: Vec<f32>,
    /// Rotation quaternion Z.
    pub qz: Vec<f32>,
    /// Rotation quaternion W.
    pub qw: Vec<f32>,
    /// Scale X / Y / Z, one entry per entity.
    pub sx: Vec<f32>,
    /// Scale Y.
    pub sy: Vec<f32>,
    /// Scale Z.
    pub sz: Vec<f32>,
}

impl TrsBatchSoa {
    /// Creates an empty batch with room for `n` entities reserved in every field.
    pub fn with_capacity(n: usize) -> Self {
        let v = || Vec::with_capacity(n);
        Self {
            tx: v(),
            ty: v(),
            tz: v(),
            qx: v(),
            qy: v(),
            qz: v(),
            qw: v(),
            sx: v(),
            sy: v(),
            sz: v(),
        }
    }

    /// The number of entities in the batch.
    pub fn len(&self) -> usize {
        self.tx.len()
    }

    /// Whether the batch holds no entities.
    pub fn is_empty(&self) -> bool {
        self.tx.is_empty()
    }

    /// Drops every entity, keeping the allocated capacity for reuse next frame.
    pub fn clear(&mut self) {
        self.tx.clear();
        self.ty.clear();
        self.tz.clear();
        self.qx.clear();
        self.qy.clear();
        self.qz.clear();
        self.qw.clear();
        self.sx.clear();
        self.sy.clear();
        self.sz.clear();
    }

    /// Appends one entity's TRS, given translation `[x, y, z]`, rotation
    /// quaternion `[x, y, z, w]`, and scale `[x, y, z]`.
    pub fn push(&mut self, translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) {
        self.tx.push(translation[0]);
        self.ty.push(translation[1]);
        self.tz.push(translation[2]);
        self.qx.push(rotation[0]);
        self.qy.push(rotation[1]);
        self.qz.push(rotation[2]);
        self.qw.push(rotation[3]);
        self.sx.push(scale[0]);
        self.sy.push(scale[1]);
        self.sz.push(scale[2]);
    }
}

/// Composes `out[i] = T · R · S` for every entity in `batch`, the same
/// column-major affine matrix [`Mat4::from_translation`]`(t) *
/// `[`Mat4::from_quat`]`(q) * `[`Mat4::from_scale`]`(s)` produces — eight
/// entities per [`f32x8`] tile, scalar for the ragged tail.
///
/// The quaternion is taken as-is (not renormalised), matching
/// [`Mat4::from_quat`]. The last matrix row is written as the exact affine
/// constant `[0, 0, 0, 1]`.
///
/// **Performance caveat (measured):** because the output is *array-of-structs*
/// `Mat4`, each lane is transposed back out individually, and that scatter
/// dominates the relatively cheap quaternion expansion — so for AoS `Mat4`
/// output this kernel is *slower* than [`compose_trs_to_mat4_scalar`]. The
/// explicit-SIMD win materialises only when the data stays field-SoA *resident*
/// across the loop (see [`normalize_quat_batch`], which is in-place SoA and does
/// win). Use this kernel only inside a pipeline whose result also stays SoA, not
/// for a one-off SoA→AoS transpose.
///
/// # Panics
/// Panics if `out.len() != batch.len()`.
pub fn compose_trs_to_mat4(batch: &TrsBatchSoa, out: &mut [Mat4]) {
    let n = batch.len();
    assert_eq!(
        out.len(),
        n,
        "output slice length ({}) must match batch length ({n})",
        out.len()
    );

    let one = f32x8::splat(1.0);
    let full = (n / LANES) * LANES;
    let mut i = 0;
    while i < full {
        let qx = load8(&batch.qx, i);
        let qy = load8(&batch.qy, i);
        let qz = load8(&batch.qz, i);
        let qw = load8(&batch.qw, i);
        let sx = load8(&batch.sx, i);
        let sy = load8(&batch.sy, i);
        let sz = load8(&batch.sz, i);

        // Quaternion → rotation columns, mirroring `Mat4::from_quat` exactly.
        let x2 = qx + qx;
        let y2 = qy + qy;
        let z2 = qz + qz;
        let xx = qx * x2;
        let xy = qx * y2;
        let xz = qx * z2;
        let yy = qy * y2;
        let yz = qy * z2;
        let zz = qz * z2;
        let wx = qw * x2;
        let wy = qw * y2;
        let wz = qw * z2;

        // `T·R·S` upper-left = rotation columns scaled per-axis by S.
        let c0x = (one - (yy + zz)) * sx;
        let c0y = (xy + wz) * sx;
        let c0z = (xz - wy) * sx;
        let c1x = (xy - wz) * sy;
        let c1y = (one - (xx + zz)) * sy;
        let c1z = (yz + wx) * sy;
        let c2x = (xz + wy) * sz;
        let c2y = (yz - wx) * sz;
        let c2z = (one - (xx + yy)) * sz;

        let tx = load8(&batch.tx, i).to_array();
        let ty = load8(&batch.ty, i).to_array();
        let tz = load8(&batch.tz, i).to_array();
        let (c0x, c0y, c0z) = (c0x.to_array(), c0y.to_array(), c0z.to_array());
        let (c1x, c1y, c1z) = (c1x.to_array(), c1y.to_array(), c1z.to_array());
        let (c2x, c2y, c2z) = (c2x.to_array(), c2y.to_array(), c2z.to_array());

        for lane in 0..LANES {
            out[i + lane] = Mat4::from_cols(
                Vec4::new(c0x[lane], c0y[lane], c0z[lane], 0.0),
                Vec4::new(c1x[lane], c1y[lane], c1z[lane], 0.0),
                Vec4::new(c2x[lane], c2y[lane], c2z[lane], 0.0),
                Vec4::new(tx[lane], ty[lane], tz[lane], 1.0),
            );
        }
        i += LANES;
    }

    // Ragged tail (n not a multiple of LANES) — scalar twin of the kernel above.
    while i < n {
        out[i] = compose_one(
            [batch.tx[i], batch.ty[i], batch.tz[i]],
            [batch.qx[i], batch.qy[i], batch.qz[i], batch.qw[i]],
            [batch.sx[i], batch.sy[i], batch.sz[i]],
        );
        i += 1;
    }
}

/// Scalar twin of [`compose_trs_to_mat4`] — identical result, one entity at a
/// time, no SIMD. This is the fallback a caller should prefer for small batches
/// (where the field-split transpose costs more than the vector op saves) and the
/// equivalence oracle the SIMD path is tested against.
///
/// # Panics
/// Panics if `out.len() != batch.len()`.
pub fn compose_trs_to_mat4_scalar(batch: &TrsBatchSoa, out: &mut [Mat4]) {
    assert_eq!(
        out.len(),
        batch.len(),
        "output slice length ({}) must match batch length ({})",
        out.len(),
        batch.len()
    );
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = compose_one(
            [batch.tx[i], batch.ty[i], batch.tz[i]],
            [batch.qx[i], batch.qy[i], batch.qz[i], batch.qw[i]],
            [batch.sx[i], batch.sy[i], batch.sz[i]],
        );
    }
}

/// Scalar `T · R · S` for one entity — the equivalence oracle for
/// [`compose_trs_to_mat4`] and the kernel used for its ragged tail.
#[inline]
fn compose_one(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Mat4 {
    let [qx, qy, qz, qw] = q;
    let x2 = qx + qx;
    let y2 = qy + qy;
    let z2 = qz + qz;
    let xx = qx * x2;
    let xy = qx * y2;
    let xz = qx * z2;
    let yy = qy * y2;
    let yz = qy * z2;
    let zz = qz * z2;
    let wx = qw * x2;
    let wy = qw * y2;
    let wz = qw * z2;
    Mat4::from_cols(
        Vec4::new(
            (1.0 - (yy + zz)) * s[0],
            (xy + wz) * s[0],
            (xz - wy) * s[0],
            0.0,
        ),
        Vec4::new(
            (xy - wz) * s[1],
            (1.0 - (xx + zz)) * s[1],
            (yz + wx) * s[1],
            0.0,
        ),
        Vec4::new(
            (xz + wy) * s[2],
            (yz - wx) * s[2],
            (1.0 - (xx + yy)) * s[2],
            0.0,
        ),
        Vec4::new(t[0], t[1], t[2], 1.0),
    )
}

/// Normalises a batch of quaternions in place to unit length, eight at a time.
///
/// This is the kernel whose reduction (`x²+y²+z²+w²`) and `1/√` the
/// auto-vectoriser refuses to lane because FP addition is not associative;
/// doing it explicitly is the point of the SIMD path. All four slices must be
/// the same length; a near-zero quaternion is left unchanged (no divide-by-zero).
///
/// # Panics
/// Panics if the four slices do not all have the same length.
pub fn normalize_quat_batch(qx: &mut [f32], qy: &mut [f32], qz: &mut [f32], qw: &mut [f32]) {
    let n = qx.len();
    assert!(
        qy.len() == n && qz.len() == n && qw.len() == n,
        "all four quaternion-component slices must have the same length"
    );

    let full = (n / LANES) * LANES;
    let mut i = 0;
    while i < full {
        let x = load8(qx, i);
        let y = load8(qy, i);
        let z = load8(qz, i);
        let w = load8(qw, i);
        let len_sq = x * x + y * y + z * z + w * w;
        // 1/√len² with the near-zero lanes forced to a unit scale (no NaN/Inf).
        let safe = len_sq.cmp_gt(f32x8::splat(f32::MIN_POSITIVE));
        let inv_len = safe.blend(f32x8::splat(1.0) / len_sq.sqrt(), f32x8::splat(1.0));
        store8(qx, i, x * inv_len);
        store8(qy, i, y * inv_len);
        store8(qz, i, z * inv_len);
        store8(qw, i, w * inv_len);
        i += LANES;
    }

    while i < n {
        let len_sq = qx[i] * qx[i] + qy[i] * qy[i] + qz[i] * qz[i] + qw[i] * qw[i];
        if len_sq > f32::MIN_POSITIVE {
            let inv = 1.0 / len_sq.sqrt();
            qx[i] *= inv;
            qy[i] *= inv;
            qz[i] *= inv;
            qw[i] *= inv;
        }
        i += 1;
    }
}

/// Loads eight contiguous `f32`s starting at `i` into a vector lane.
///
/// Goes through a contiguous slice→array conversion (one 32-byte move the
/// backend lowers to a single vector load) rather than eight indexed reads —
/// the latter keeps per-element bounds checks and defeats the vectorisation.
#[inline]
fn load8(s: &[f32], i: usize) -> f32x8 {
    let chunk: [f32; LANES] = s[i..i + LANES].try_into().unwrap();
    f32x8::new(chunk)
}

/// Stores a vector lane back into eight contiguous `f32`s starting at `i`.
#[inline]
fn store8(s: &mut [f32], i: usize, v: f32x8) {
    let a = v.to_array();
    s[i..i + LANES].copy_from_slice(&a);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Quaternion, Vec3};

    /// A spread of TRS inputs whose count is deliberately not a multiple of
    /// `LANES`, so both the vector body and the scalar tail are exercised.
    fn sample_batch(n: usize) -> TrsBatchSoa {
        let mut b = TrsBatchSoa::with_capacity(n);
        for k in 0..n {
            let f = k as f32;
            let axis = Vec3::new(0.3 + f, 1.0 - 0.2 * f, 0.5 + 0.1 * f).normalize();
            let q = Quaternion::from_axis_angle(axis, 0.17 * f + 0.4);
            b.push(
                [f, -2.0 * f, 0.5 * f + 1.0],
                [q.x, q.y, q.z, q.w],
                [1.0 + 0.1 * f, 2.0 - 0.05 * f, 0.75 + 0.02 * f],
            );
        }
        b
    }

    #[test]
    fn compose_matches_scalar_oracle() {
        let n = 21; // 2 full tiles + 5 tail
        let batch = sample_batch(n);
        let mut out = vec![Mat4::IDENTITY; n];
        compose_trs_to_mat4(&batch, &mut out);

        for (k, mat) in out.iter().enumerate() {
            let expected = compose_one(
                [batch.tx[k], batch.ty[k], batch.tz[k]],
                [batch.qx[k], batch.qy[k], batch.qz[k], batch.qw[k]],
                [batch.sx[k], batch.sy[k], batch.sz[k]],
            );
            for col in 0..4 {
                for row in 0..4 {
                    assert!(
                        crate::math::approx_eq(mat.cols[col][row], expected.cols[col][row]),
                        "entity {k} mismatch at col {col} row {row}"
                    );
                }
            }
            // Affine last row must be the exact constant the AffineTransform
            // conversion asserts on.
            assert_eq!(mat.get_row(3), Vec4::new(0.0, 0.0, 0.0, 1.0));
        }
    }

    #[test]
    fn normalize_matches_scalar_and_yields_unit_length() {
        let n = 19; // 2 full tiles + 3 tail
        let mut qx = Vec::new();
        let mut qy = Vec::new();
        let mut qz = Vec::new();
        let mut qw = Vec::new();
        for k in 0..n {
            let f = k as f32;
            qx.push(0.5 + f);
            qy.push(1.0 - 0.1 * f);
            qz.push(0.25 * f);
            qw.push(2.0 + 0.3 * f);
        }
        let (rx, ry, rz, rw) = (qx.clone(), qy.clone(), qz.clone(), qw.clone());

        normalize_quat_batch(&mut qx, &mut qy, &mut qz, &mut qw);

        for k in 0..n {
            let inv = 1.0 / (rx[k] * rx[k] + ry[k] * ry[k] + rz[k] * rz[k] + rw[k] * rw[k]).sqrt();
            assert!(crate::math::approx_eq(qx[k], rx[k] * inv));
            assert!(crate::math::approx_eq(qy[k], ry[k] * inv));
            assert!(crate::math::approx_eq(qz[k], rz[k] * inv));
            assert!(crate::math::approx_eq(qw[k], rw[k] * inv));
            let len = (qx[k] * qx[k] + qy[k] * qy[k] + qz[k] * qz[k] + qw[k] * qw[k]).sqrt();
            assert!(
                crate::math::approx_eq(len, 1.0),
                "entity {k} not unit length"
            );
        }
    }

    #[test]
    fn near_zero_quaternion_is_left_unchanged() {
        let mut qx = vec![0.0f32; LANES];
        let mut qy = vec![0.0f32; LANES];
        let mut qz = vec![0.0f32; LANES];
        let mut qw = vec![0.0f32; LANES];
        normalize_quat_batch(&mut qx, &mut qy, &mut qz, &mut qw);
        assert!(qx.iter().all(|&v| v == 0.0));
        assert!(qw.iter().all(|&v| v == 0.0));
    }
}
