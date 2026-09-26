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

//! Memory layout of the math types that cross into GPU buffers.
//!
//! `Vec2`/`Vec3`/`Vec4`/`Mat3`/`Mat4` are `#[repr(C)]`; the vectors and `Mat4`
//! are also `bytemuck::Pod` and uploaded as-is. A lost `#[repr(C)]` (or a
//! field reordered by a move) would let rustc choose another layout; these
//! assertions pin size, alignment and every field offset.

use khora_core::math::matrix::{Mat3, Mat4};
use khora_core::math::vector::{Vec2, Vec3, Vec4};
use std::mem::{align_of, offset_of, size_of};

#[test]
fn vectors_are_packed_f32_in_field_order() {
    assert_eq!((size_of::<Vec2>(), align_of::<Vec2>()), (8, 4));
    assert_eq!((offset_of!(Vec2, x), offset_of!(Vec2, y)), (0, 4));

    assert_eq!((size_of::<Vec3>(), align_of::<Vec3>()), (12, 4));
    assert_eq!(
        (
            offset_of!(Vec3, x),
            offset_of!(Vec3, y),
            offset_of!(Vec3, z)
        ),
        (0, 4, 8)
    );

    assert_eq!((size_of::<Vec4>(), align_of::<Vec4>()), (16, 4));
    assert_eq!(
        (
            offset_of!(Vec4, x),
            offset_of!(Vec4, y),
            offset_of!(Vec4, z),
            offset_of!(Vec4, w)
        ),
        (0, 4, 8, 12)
    );
}

#[test]
fn matrices_are_column_arrays_at_offset_zero() {
    assert_eq!((size_of::<Mat3>(), align_of::<Mat3>()), (36, 4));
    assert_eq!(offset_of!(Mat3, cols), 0);

    assert_eq!((size_of::<Mat4>(), align_of::<Mat4>()), (64, 4));
    assert_eq!(offset_of!(Mat4, cols), 0);
}

#[test]
fn mat4_bytes_are_column_major() {
    let m = Mat4::from_cols(
        Vec4::new(1.0, 2.0, 3.0, 4.0),
        Vec4::new(5.0, 6.0, 7.0, 8.0),
        Vec4::new(9.0, 10.0, 11.0, 12.0),
        Vec4::new(13.0, 14.0, 15.0, 16.0),
    );
    let floats: &[f32] = bytemuck::cast_slice(std::slice::from_ref(&m));
    let expected: Vec<f32> = (1..=16).map(|i| i as f32).collect();
    assert_eq!(floats, expected.as_slice());
}
