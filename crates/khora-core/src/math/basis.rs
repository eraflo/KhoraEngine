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

//! Building an orthonormal basis from the axes a transform still shows: a
//! direction from any non-zero axis, and the axes a flattened scale lost.

use crate::math::Vec3;

/// Two unit directions whose cross product is shorter than this are taken as
/// parallel: [`AffineTransform::rotation`] cannot draw a perpendicular from
/// one to the other.
pub(super) const PARALLEL_SINE: f32 = 1e-6;

/// An axis shorter than this fraction of the longest one carries no
/// direction: f32 matrix products leave a residue of a few units in the last
/// place (~1e-7) of the longest axis in an axis meant to be zero — a quarter
/// turn under a parent flattened to zero leaves one — so below this neither
/// its direction nor its sign is information, only rounding.
pub(super) const NOISE_FLOOR: f32 = 1e-6;

/// `v` made unit whatever its magnitude — `None` when it is zero or not
/// finite.
///
/// Divided by its largest component first, `v` neither underflows nor
/// overflows when squared (`Vec3::normalize` also gives up below an absolute
/// length), so a millimetre axis is as good as a metre one.
pub(super) fn unit(v: Vec3) -> Option<Vec3> {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return None;
    }
    let largest = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if largest == 0.0 {
        return None;
    }
    // Component by component: `Vec3 / f32` multiplies by the reciprocal,
    // which is infinite for a subnormal `largest`.
    let v = Vec3::new(v.x / largest, v.y / largest, v.z / largest);
    Some(v / v.length())
}

/// The part of the unit vector `v` perpendicular to the unit vector `onto`,
/// made unit — `None` when `v` is (nearly) parallel to `onto`.
pub(super) fn unit_rejection(v: Vec3, onto: Vec3) -> Option<Vec3> {
    let rejection = v - onto * v.dot(onto);
    if rejection.length() <= PARALLEL_SINE {
        return None;
    }
    unit(rejection)
}

/// Three perpendicular unit axes keeping the `known` ones: an axis scaled to
/// zero has no direction of its own, so it is rebuilt from the others
/// (right-handed), and a flattened pose keeps the rotation it shows.
pub(super) fn complete_basis(known: [Option<Vec3>; 3]) -> [Vec3; 3] {
    match known {
        [Some(x), Some(y), Some(z)] => [x, y, z],
        [None, Some(y), Some(z)] => [y.cross(z), y, z],
        [Some(x), None, Some(z)] => [x, z.cross(x), z],
        [Some(x), Some(y), None] => [x, y, x.cross(y)],
        [Some(x), None, None] => {
            let y = any_perpendicular(x);
            [x, y, x.cross(y)]
        }
        [None, Some(y), None] => {
            let z = any_perpendicular(y);
            [y.cross(z), y, z]
        }
        [None, None, Some(z)] => {
            let x = any_perpendicular(z);
            [x, z.cross(x), z]
        }
        [None, None, None] => [Vec3::X, Vec3::Y, Vec3::Z],
    }
}

/// A unit vector perpendicular to the unit vector `v`.
fn any_perpendicular(v: Vec3) -> Vec3 {
    let helper = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    v.cross(helper).normalize()
}
