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

//! Compile-level guard over `khora_core::math` (the rest of the crate is in
//! `public_paths.rs` and its siblings).
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, statics, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), trait
//! items, and the std / serde / bytemuck / operator trait impls. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions, bound forwarding for traits, `ptr::eq`
//! for statics).
//!
//! The list was generated from `khora_core`'s rustdoc JSON, so it is complete
//! for the tree it was written against. Nothing is constructed; the tests only
//! have to type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

fn is_add<T: std::ops::Add>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_div_f32<T: std::ops::Div<f32>>() {}
fn is_eq<T: Eq>() {}
fn is_from_affine_transform<T: From<khora_core::math::affine_transform::AffineTransform>>() {}
fn is_from_f32_2<T: From<[f32; 2]>>() {}
fn is_from_f32_4<T: From<[f32; 4]>>() {}
fn is_from_mat4<T: From<khora_core::math::matrix::Mat4>>() {}
fn is_from_vec2<T: From<khora_core::math::vector::Vec2>>() {}
fn is_from_vec4<T: From<khora_core::math::vector::Vec4>>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_index_mut_usize<T: std::ops::IndexMut<usize>>() {}
fn is_index_usize<T: std::ops::Index<usize>>() {}
fn is_mul<T: std::ops::Mul>() {}
fn is_mul_assign<T: std::ops::MulAssign>() {}
fn is_mul_f32<T: std::ops::Mul<f32>>() {}
fn is_mul_linear_rgba<T: std::ops::Mul<khora_core::math::color::LinearRgba>>() {}
fn is_mul_vec2<T: std::ops::Mul<khora_core::math::vector::Vec2>>() {}
fn is_mul_vec3<T: std::ops::Mul<khora_core::math::vector::Vec3>>() {}
fn is_mul_vec4<T: std::ops::Mul<khora_core::math::vector::Vec4>>() {}
fn is_neg<T: std::ops::Neg>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_pod<T: bytemuck::Pod>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_sub<T: std::ops::Sub>() {}
fn is_zeroable<T: bytemuck::Zeroable>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_core::math as _;
    use khora_core::math::affine_transform as _;
    use khora_core::math::color as _;
    use khora_core::math::cube as _;
    use khora_core::math::dimension as _;
    use khora_core::math::geometry as _;
    use khora_core::math::matrix as _;
    use khora_core::math::quaternion as _;
    use khora_core::math::ray as _;
    use khora_core::math::simd as _;
    use khora_core::math::vector as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn affine_transform_fields(x: &khora_core::math::affine_transform::AffineTransform) {
    let _ = (&x.0,);
}

fn linear_rgba_fields(x: &khora_core::math::color::LinearRgba) {
    let _ = (&x.r, &x.g, &x.b, &x.a);
}

fn cube_face_variants(x: &khora_core::math::cube::CubeFace) {
    match x {
        khora_core::math::cube::CubeFace::PosX => {}
        khora_core::math::cube::CubeFace::NegX => {}
        khora_core::math::cube::CubeFace::PosY => {}
        khora_core::math::cube::CubeFace::NegY => {}
        khora_core::math::cube::CubeFace::PosZ => {}
        khora_core::math::cube::CubeFace::NegZ => {}
    }
}

fn extent1_d_fields(x: &khora_core::math::dimension::Extent1D) {
    let _ = (&x.width,);
}

fn extent2_d_fields(x: &khora_core::math::dimension::Extent2D) {
    let _ = (&x.width, &x.height);
}

fn extent3_d_fields(x: &khora_core::math::dimension::Extent3D) {
    let _ = (&x.width, &x.height, &x.depth_or_array_layers);
}

fn origin2_d_fields(x: &khora_core::math::dimension::Origin2D) {
    let _ = (&x.x, &x.y);
}

fn origin3_d_fields(x: &khora_core::math::dimension::Origin3D) {
    let _ = (&x.x, &x.y, &x.z);
}

fn aabb_fields(x: &khora_core::math::geometry::Aabb) {
    let _ = (&x.min, &x.max);
}

fn rect2_d_fields(x: &khora_core::math::geometry::Rect2D) {
    let _ = (&x.min, &x.max);
}

fn mat3_fields(x: &khora_core::math::matrix::Mat3) {
    let _ = (&x.cols,);
}

fn mat4_fields(x: &khora_core::math::matrix::Mat4) {
    let _ = (&x.cols,);
}

fn quaternion_fields(x: &khora_core::math::quaternion::Quaternion) {
    let _ = (&x.x, &x.y, &x.z, &x.w);
}

fn ray_fields(x: &khora_core::math::ray::Ray) {
    let _ = (&x.origin, &x.direction);
}

fn trs_batch_soa_fields(x: &khora_core::math::simd::TrsBatchSoa) {
    let _ = (
        &x.tx, &x.ty, &x.tz, &x.qx, &x.qy, &x.qz, &x.qw, &x.sx, &x.sy, &x.sz,
    );
}

fn vec2_fields(x: &khora_core::math::vector::Vec2) {
    let _ = (&x.x, &x.y);
}

fn vec3_fields(x: &khora_core::math::vector::Vec3) {
    let _ = (&x.x, &x.y, &x.z);
}

fn vec4_fields(x: &khora_core::math::vector::Vec4) {
    let _ = (&x.x, &x.y, &x.z, &x.w);
}

#[test]
fn module_math_paths_still_resolve() {
    let _ = khora_core::math::DEG_TO_RAD;
    let _ = khora_core::math::E;
    let _ = khora_core::math::EPSILON;
    let _ = khora_core::math::FRAC_PI_2;
    let _ = khora_core::math::FRAC_PI_3;
    let _ = khora_core::math::FRAC_PI_4;
    let _ = khora_core::math::FRAC_PI_6;
    let _ = khora_core::math::FRAC_PI_8;
    let _ = khora_core::math::LN_10;
    let _ = khora_core::math::LN_2;
    let _ = khora_core::math::LOG10_E;
    let _ = khora_core::math::LOG2_E;
    let _ = khora_core::math::PI;
    let _ = khora_core::math::RAD_TO_DEG;
    let _ = khora_core::math::SQRT_2;
    let _ = khora_core::math::TAU;
    let _ = type_name::<khora_core::math::affine_transform::AffineTransform>();
    let _ = type_name::<khora_core::math::AffineTransform>();
    same_type(
        PhantomData::<khora_core::math::AffineTransform>,
        PhantomData::<khora_core::math::affine_transform::AffineTransform>,
    );
    let _ = affine_transform_fields as fn(&khora_core::math::affine_transform::AffineTransform);
    let _ = khora_core::math::affine_transform::AffineTransform::IDENTITY;
    let _ = khora_core::math::affine_transform::AffineTransform::from_translation;
    let _ = khora_core::math::affine_transform::AffineTransform::from_scale;
    let _ = khora_core::math::affine_transform::AffineTransform::from_rotation_x;
    let _ = khora_core::math::affine_transform::AffineTransform::from_rotation_y;
    let _ = khora_core::math::affine_transform::AffineTransform::from_rotation_z;
    let _ = khora_core::math::affine_transform::AffineTransform::from_axis_angle;
    let _ = khora_core::math::affine_transform::AffineTransform::from_quat;
    let _ = khora_core::math::affine_transform::AffineTransform::to_matrix;
    let _ = khora_core::math::affine_transform::AffineTransform::translation;
    let _ = khora_core::math::affine_transform::AffineTransform::right;
    let _ = khora_core::math::affine_transform::AffineTransform::up;
    let _ = khora_core::math::affine_transform::AffineTransform::forward;
    let _ = khora_core::math::affine_transform::AffineTransform::rotation;
    let _ = khora_core::math::affine_transform::AffineTransform::scale;
    let _ = khora_core::math::affine_transform::AffineTransform::interpolate;
    let _ = khora_core::math::affine_transform::AffineTransform::inverse;
    is_debug::<khora_core::math::affine_transform::AffineTransform>();
    is_clone::<khora_core::math::affine_transform::AffineTransform>();
    is_copy::<khora_core::math::affine_transform::AffineTransform>();
    is_partial_eq::<khora_core::math::affine_transform::AffineTransform>();
    is_serialize::<khora_core::math::affine_transform::AffineTransform>();
    is_deserialize_owned::<khora_core::math::affine_transform::AffineTransform>();
    is_default::<khora_core::math::affine_transform::AffineTransform>();
    is_from_affine_transform::<khora_core::math::matrix::Mat4>();
    is_from_mat4::<khora_core::math::affine_transform::AffineTransform>();
    let _ = khora_core::math::approx_eq;
    let _ = khora_core::math::approx_eq_eps;
    let _ = khora_core::math::clamp::<f32>;
    let _ = type_name::<khora_core::math::color::LinearRgba>();
    let _ = type_name::<khora_core::math::LinearRgba>();
    same_type(
        PhantomData::<khora_core::math::LinearRgba>,
        PhantomData::<khora_core::math::color::LinearRgba>,
    );
    let _ = linear_rgba_fields as fn(&khora_core::math::color::LinearRgba);
    let _ = khora_core::math::color::LinearRgba::RED;
    let _ = khora_core::math::color::LinearRgba::GREEN;
    let _ = khora_core::math::color::LinearRgba::BLUE;
    let _ = khora_core::math::color::LinearRgba::YELLOW;
    let _ = khora_core::math::color::LinearRgba::CYAN;
    let _ = khora_core::math::color::LinearRgba::MAGENTA;
    let _ = khora_core::math::color::LinearRgba::WHITE;
    let _ = khora_core::math::color::LinearRgba::BLACK;
    let _ = khora_core::math::color::LinearRgba::TRANSPARENT;
    let _ = khora_core::math::color::LinearRgba::new;
    let _ = khora_core::math::color::LinearRgba::rgb;
    let _ = khora_core::math::color::LinearRgba::gray;
    let _ = khora_core::math::color::LinearRgba::from_vec4;
    let _ = khora_core::math::color::LinearRgba::to_vec4;
    let _ = khora_core::math::color::LinearRgba::from_hex;
    let _ = khora_core::math::color::LinearRgba::to_hex;
    let _ = khora_core::math::color::LinearRgba::from_srgb;
    let _ = khora_core::math::color::LinearRgba::to_srgb;
    let _ = khora_core::math::color::LinearRgba::from_oklch;
    let _ = khora_core::math::color::LinearRgba::from_oklab;
    let _ = khora_core::math::color::LinearRgba::premultiplied;
    let _ = khora_core::math::color::LinearRgba::unpremultiplied;
    let _ = khora_core::math::color::LinearRgba::with_alpha;
    let _ = khora_core::math::color::LinearRgba::lerp;
    is_debug::<khora_core::math::color::LinearRgba>();
    is_clone::<khora_core::math::color::LinearRgba>();
    is_copy::<khora_core::math::color::LinearRgba>();
    is_partial_eq::<khora_core::math::color::LinearRgba>();
    is_pod::<khora_core::math::color::LinearRgba>();
    is_zeroable::<khora_core::math::color::LinearRgba>();
    is_serialize::<khora_core::math::color::LinearRgba>();
    is_deserialize_owned::<khora_core::math::color::LinearRgba>();
    is_default::<khora_core::math::color::LinearRgba>();
    is_add::<khora_core::math::color::LinearRgba>();
    is_sub::<khora_core::math::color::LinearRgba>();
    is_mul_f32::<khora_core::math::color::LinearRgba>();
    is_mul_linear_rgba::<f32>();
    is_mul::<khora_core::math::color::LinearRgba>();
    is_div_f32::<khora_core::math::color::LinearRgba>();
    let _ = type_name::<khora_core::math::cube::CubeFace>();
    let _ = type_name::<khora_core::math::CubeFace>();
    same_type(
        PhantomData::<khora_core::math::CubeFace>,
        PhantomData::<khora_core::math::cube::CubeFace>,
    );
    let _ = cube_face_variants as fn(&khora_core::math::cube::CubeFace);
    let _ = khora_core::math::cube::CubeFace::ALL;
    let _ = khora_core::math::cube::CubeFace::index;
    let _ = khora_core::math::cube::CubeFace::forward;
    let _ = khora_core::math::cube::CubeFace::up;
    let _ = khora_core::math::cube::CubeFace::from_direction;
    is_debug::<khora_core::math::cube::CubeFace>();
    is_clone::<khora_core::math::cube::CubeFace>();
    is_copy::<khora_core::math::cube::CubeFace>();
    is_partial_eq::<khora_core::math::cube::CubeFace>();
    is_eq::<khora_core::math::cube::CubeFace>();
    is_hash::<khora_core::math::cube::CubeFace>();
    let _ = khora_core::math::degrees_to_radians;
    let _ = type_name::<khora_core::math::dimension::Extent1D>();
    let _ = type_name::<khora_core::math::Extent1D>();
    same_type(
        PhantomData::<khora_core::math::Extent1D>,
        PhantomData::<khora_core::math::dimension::Extent1D>,
    );
    let _ = extent1_d_fields as fn(&khora_core::math::dimension::Extent1D);
    is_debug::<khora_core::math::dimension::Extent1D>();
    is_clone::<khora_core::math::dimension::Extent1D>();
    is_copy::<khora_core::math::dimension::Extent1D>();
    is_partial_eq::<khora_core::math::dimension::Extent1D>();
    is_eq::<khora_core::math::dimension::Extent1D>();
    is_hash::<khora_core::math::dimension::Extent1D>();
    is_default::<khora_core::math::dimension::Extent1D>();
    let _ = type_name::<khora_core::math::dimension::Extent2D>();
    let _ = type_name::<khora_core::math::Extent2D>();
    same_type(
        PhantomData::<khora_core::math::Extent2D>,
        PhantomData::<khora_core::math::dimension::Extent2D>,
    );
    let _ = extent2_d_fields as fn(&khora_core::math::dimension::Extent2D);
    is_debug::<khora_core::math::dimension::Extent2D>();
    is_clone::<khora_core::math::dimension::Extent2D>();
    is_copy::<khora_core::math::dimension::Extent2D>();
    is_partial_eq::<khora_core::math::dimension::Extent2D>();
    is_eq::<khora_core::math::dimension::Extent2D>();
    is_hash::<khora_core::math::dimension::Extent2D>();
    is_default::<khora_core::math::dimension::Extent2D>();
    let _ = type_name::<khora_core::math::dimension::Extent3D>();
    let _ = type_name::<khora_core::math::Extent3D>();
    same_type(
        PhantomData::<khora_core::math::Extent3D>,
        PhantomData::<khora_core::math::dimension::Extent3D>,
    );
    let _ = extent3_d_fields as fn(&khora_core::math::dimension::Extent3D);
    is_debug::<khora_core::math::dimension::Extent3D>();
    is_clone::<khora_core::math::dimension::Extent3D>();
    is_copy::<khora_core::math::dimension::Extent3D>();
    is_partial_eq::<khora_core::math::dimension::Extent3D>();
    is_eq::<khora_core::math::dimension::Extent3D>();
    is_hash::<khora_core::math::dimension::Extent3D>();
    is_default::<khora_core::math::dimension::Extent3D>();
    let _ = type_name::<khora_core::math::dimension::Origin2D>();
    let _ = type_name::<khora_core::math::Origin2D>();
    same_type(
        PhantomData::<khora_core::math::Origin2D>,
        PhantomData::<khora_core::math::dimension::Origin2D>,
    );
    let _ = origin2_d_fields as fn(&khora_core::math::dimension::Origin2D);
    is_debug::<khora_core::math::dimension::Origin2D>();
    is_clone::<khora_core::math::dimension::Origin2D>();
    is_copy::<khora_core::math::dimension::Origin2D>();
    is_partial_eq::<khora_core::math::dimension::Origin2D>();
    is_eq::<khora_core::math::dimension::Origin2D>();
    is_hash::<khora_core::math::dimension::Origin2D>();
    is_default::<khora_core::math::dimension::Origin2D>();
    let _ = type_name::<khora_core::math::dimension::Origin3D>();
    let _ = type_name::<khora_core::math::Origin3D>();
    same_type(
        PhantomData::<khora_core::math::Origin3D>,
        PhantomData::<khora_core::math::dimension::Origin3D>,
    );
    let _ = origin3_d_fields as fn(&khora_core::math::dimension::Origin3D);
    is_debug::<khora_core::math::dimension::Origin3D>();
    is_clone::<khora_core::math::dimension::Origin3D>();
    is_copy::<khora_core::math::dimension::Origin3D>();
    is_partial_eq::<khora_core::math::dimension::Origin3D>();
    is_eq::<khora_core::math::dimension::Origin3D>();
    is_hash::<khora_core::math::dimension::Origin3D>();
    is_default::<khora_core::math::dimension::Origin3D>();
    let _ = type_name::<khora_core::math::geometry::Aabb>();
    let _ = type_name::<khora_core::math::Aabb>();
    same_type(
        PhantomData::<khora_core::math::Aabb>,
        PhantomData::<khora_core::math::geometry::Aabb>,
    );
    let _ = aabb_fields as fn(&khora_core::math::geometry::Aabb);
    let _ = khora_core::math::geometry::Aabb::INVALID;
    let _ = khora_core::math::geometry::Aabb::from_min_max;
    let _ = khora_core::math::geometry::Aabb::from_center_half_extents;
    let _ = khora_core::math::geometry::Aabb::from_half_extents;
    let _ = khora_core::math::geometry::Aabb::from_point;
    let _ = khora_core::math::geometry::Aabb::from_points;
    let _ = khora_core::math::geometry::Aabb::center;
    let _ = khora_core::math::geometry::Aabb::half_extents;
    let _ = khora_core::math::geometry::Aabb::size;
    let _ = khora_core::math::geometry::Aabb::is_valid;
    let _ = khora_core::math::geometry::Aabb::contains_point;
    let _ = khora_core::math::geometry::Aabb::intersects_aabb;
    let _ = khora_core::math::geometry::Aabb::merge;
    let _ = khora_core::math::geometry::Aabb::merged_with_point;
    let _ = khora_core::math::geometry::Aabb::transform;
    let _ = khora_core::math::geometry::Aabb::surface_area;
    let _ = khora_core::math::geometry::Aabb::contains_aabb;
    let _ = khora_core::math::geometry::Aabb::intersect_ray;
    is_debug::<khora_core::math::geometry::Aabb>();
    is_clone::<khora_core::math::geometry::Aabb>();
    is_copy::<khora_core::math::geometry::Aabb>();
    is_partial_eq::<khora_core::math::geometry::Aabb>();
    is_default::<khora_core::math::geometry::Aabb>();
    let _ = type_name::<khora_core::math::geometry::Rect2D>();
    let _ = type_name::<khora_core::math::Rect2D>();
    same_type(
        PhantomData::<khora_core::math::Rect2D>,
        PhantomData::<khora_core::math::geometry::Rect2D>,
    );
    let _ = rect2_d_fields as fn(&khora_core::math::geometry::Rect2D);
    let _ = khora_core::math::geometry::Rect2D::ZERO;
    let _ = khora_core::math::geometry::Rect2D::from_min_max;
    let _ = khora_core::math::geometry::Rect2D::from_min_size;
    let _ = khora_core::math::geometry::Rect2D::from_center_size;
    let _ = khora_core::math::geometry::Rect2D::width;
    let _ = khora_core::math::geometry::Rect2D::height;
    let _ = khora_core::math::geometry::Rect2D::size;
    let _ = khora_core::math::geometry::Rect2D::center;
    let _ = khora_core::math::geometry::Rect2D::contains;
    is_debug::<khora_core::math::geometry::Rect2D>();
    is_clone::<khora_core::math::geometry::Rect2D>();
    is_copy::<khora_core::math::geometry::Rect2D>();
    is_partial_eq::<khora_core::math::geometry::Rect2D>();
    is_default::<khora_core::math::geometry::Rect2D>();
    let _ = type_name::<khora_core::math::matrix::Mat3>();
    let _ = type_name::<khora_core::math::Mat3>();
    same_type(
        PhantomData::<khora_core::math::Mat3>,
        PhantomData::<khora_core::math::matrix::Mat3>,
    );
    let _ = mat3_fields as fn(&khora_core::math::matrix::Mat3);
    let _ = khora_core::math::matrix::Mat3::IDENTITY;
    let _ = khora_core::math::matrix::Mat3::ZERO;
    let _ = khora_core::math::matrix::Mat3::from_cols;
    let _ = khora_core::math::matrix::Mat3::from_scale_vec2;
    let _ = khora_core::math::matrix::Mat3::from_scale;
    let _ = khora_core::math::matrix::Mat3::from_rotation_x;
    let _ = khora_core::math::matrix::Mat3::from_rotation_y;
    let _ = khora_core::math::matrix::Mat3::from_rotation_z;
    let _ = khora_core::math::matrix::Mat3::from_axis_angle;
    let _ = khora_core::math::matrix::Mat3::from_quat;
    let _ = khora_core::math::matrix::Mat3::from_mat4;
    let _ = khora_core::math::matrix::Mat3::determinant;
    let _ = khora_core::math::matrix::Mat3::transpose;
    let _ = khora_core::math::matrix::Mat3::inverse;
    let _ = khora_core::math::matrix::Mat3::to_mat4;
    is_debug::<khora_core::math::matrix::Mat3>();
    is_clone::<khora_core::math::matrix::Mat3>();
    is_copy::<khora_core::math::matrix::Mat3>();
    is_partial_eq::<khora_core::math::matrix::Mat3>();
    is_default::<khora_core::math::matrix::Mat3>();
    is_mul::<khora_core::math::matrix::Mat3>();
    is_mul_vec3::<khora_core::math::matrix::Mat3>();
    is_index_usize::<khora_core::math::matrix::Mat3>();
    is_index_mut_usize::<khora_core::math::matrix::Mat3>();
    let _ = type_name::<khora_core::math::matrix::Mat4>();
    let _ = type_name::<khora_core::math::Mat4>();
    same_type(
        PhantomData::<khora_core::math::Mat4>,
        PhantomData::<khora_core::math::matrix::Mat4>,
    );
    let _ = mat4_fields as fn(&khora_core::math::matrix::Mat4);
    let _ = khora_core::math::matrix::Mat4::IDENTITY;
    let _ = khora_core::math::matrix::Mat4::ZERO;
    let _ = khora_core::math::matrix::Mat4::from_cols;
    let _ = khora_core::math::matrix::Mat4::get_row;
    let _ = khora_core::math::matrix::Mat4::to_cols_array_2d;
    let _ = khora_core::math::matrix::Mat4::from_translation;
    let _ = khora_core::math::matrix::Mat4::from_scale;
    let _ = khora_core::math::matrix::Mat4::from_rotation_x;
    let _ = khora_core::math::matrix::Mat4::from_rotation_y;
    let _ = khora_core::math::matrix::Mat4::from_rotation_z;
    let _ = khora_core::math::matrix::Mat4::from_axis_angle;
    let _ = khora_core::math::matrix::Mat4::from_quat;
    let _ = khora_core::math::matrix::Mat4::perspective_rh_zo;
    let _ = khora_core::math::matrix::Mat4::orthographic_rh_zo;
    let _ = khora_core::math::matrix::Mat4::look_at_rh;
    let _ = khora_core::math::matrix::Mat4::cube_face_view_proj;
    let _ = khora_core::math::matrix::Mat4::cube_face_view_projs;
    let _ = khora_core::math::matrix::Mat4::transpose;
    let _ = khora_core::math::matrix::Mat4::determinant;
    let _ = khora_core::math::matrix::Mat4::inverse;
    let _ = khora_core::math::matrix::Mat4::affine_inverse;
    let _ = khora_core::math::matrix::Mat4::transform_point;
    let _ = khora_core::math::matrix::Mat4::transform_vector;
    is_debug::<khora_core::math::matrix::Mat4>();
    is_clone::<khora_core::math::matrix::Mat4>();
    is_copy::<khora_core::math::matrix::Mat4>();
    is_partial_eq::<khora_core::math::matrix::Mat4>();
    is_pod::<khora_core::math::matrix::Mat4>();
    is_zeroable::<khora_core::math::matrix::Mat4>();
    is_serialize::<khora_core::math::matrix::Mat4>();
    is_deserialize_owned::<khora_core::math::matrix::Mat4>();
    is_default::<khora_core::math::matrix::Mat4>();
    is_mul::<khora_core::math::matrix::Mat4>();
    is_mul_vec4::<khora_core::math::matrix::Mat4>();
    let _ = type_name::<khora_core::math::quaternion::Quat>();
    let _ = type_name::<khora_core::math::Quat>();
    same_type(
        PhantomData::<khora_core::math::Quat>,
        PhantomData::<khora_core::math::quaternion::Quat>,
    );
    let _ = type_name::<khora_core::math::quaternion::Quaternion>();
    let _ = type_name::<khora_core::math::Quaternion>();
    same_type(
        PhantomData::<khora_core::math::Quaternion>,
        PhantomData::<khora_core::math::quaternion::Quaternion>,
    );
    let _ = quaternion_fields as fn(&khora_core::math::quaternion::Quaternion);
    let _ = khora_core::math::quaternion::Quaternion::IDENTITY;
    let _ = khora_core::math::quaternion::Quaternion::new;
    let _ = khora_core::math::quaternion::Quaternion::from_axis_angle;
    let _ = khora_core::math::quaternion::Quaternion::from_rotation_matrix;
    let _ = khora_core::math::quaternion::Quaternion::magnitude_squared;
    let _ = khora_core::math::quaternion::Quaternion::magnitude;
    let _ = khora_core::math::quaternion::Quaternion::normalize;
    let _ = khora_core::math::quaternion::Quaternion::conjugate;
    let _ = khora_core::math::quaternion::Quaternion::inverse;
    let _ = khora_core::math::quaternion::Quaternion::dot;
    let _ = khora_core::math::quaternion::Quaternion::rotate_vec3;
    let _ = khora_core::math::quaternion::Quaternion::slerp;
    let _ = khora_core::math::quaternion::Quaternion::to_euler_xyz;
    let _ = khora_core::math::quaternion::Quaternion::from_euler_xyz;
    is_debug::<khora_core::math::quaternion::Quaternion>();
    is_clone::<khora_core::math::quaternion::Quaternion>();
    is_copy::<khora_core::math::quaternion::Quaternion>();
    is_partial_eq::<khora_core::math::quaternion::Quaternion>();
    is_serialize::<khora_core::math::quaternion::Quaternion>();
    is_deserialize_owned::<khora_core::math::quaternion::Quaternion>();
    is_default::<khora_core::math::quaternion::Quaternion>();
    is_mul::<khora_core::math::quaternion::Quaternion>();
    is_mul_assign::<khora_core::math::quaternion::Quaternion>();
    is_mul_vec3::<khora_core::math::quaternion::Quaternion>();
    is_add::<khora_core::math::quaternion::Quaternion>();
    is_sub::<khora_core::math::quaternion::Quaternion>();
    is_mul_f32::<khora_core::math::quaternion::Quaternion>();
    is_neg::<khora_core::math::quaternion::Quaternion>();
    let _ = khora_core::math::radians_to_degrees;
    let _ = type_name::<khora_core::math::ray::Ray>();
    let _ = type_name::<khora_core::math::Ray>();
    let _ = type_name::<khora_core::physics::Ray>();
    same_type(
        PhantomData::<khora_core::math::Ray>,
        PhantomData::<khora_core::math::ray::Ray>,
    );
    same_type(
        PhantomData::<khora_core::physics::Ray>,
        PhantomData::<khora_core::math::ray::Ray>,
    );
    let _ = ray_fields as fn(&khora_core::math::ray::Ray);
    let _ = khora_core::math::ray::Ray::new;
    let _ = khora_core::math::ray::Ray::at;
    let _ = khora_core::math::ray::Ray::inv_direction;
    let _ = khora_core::math::ray::Ray::intersect_aabb;
    let _ = khora_core::math::ray::Ray::intersect_plane;
    let _ = khora_core::math::ray::Ray::distance_to_point;
    let _ = khora_core::math::ray::Ray::closest_param_on_line;
    let _ = khora_core::math::ray::Ray::closest_point_on_segment;
    is_debug::<khora_core::math::ray::Ray>();
    is_clone::<khora_core::math::ray::Ray>();
    is_copy::<khora_core::math::ray::Ray>();
    is_partial_eq::<khora_core::math::ray::Ray>();
    is_serialize::<khora_core::math::ray::Ray>();
    is_deserialize_owned::<khora_core::math::ray::Ray>();
    let _ = khora_core::math::saturate;
    let _ = khora_core::math::simd::LANES;
    let _ = type_name::<khora_core::math::simd::TrsBatchSoa>();
    let _ = trs_batch_soa_fields as fn(&khora_core::math::simd::TrsBatchSoa);
    let _ = khora_core::math::simd::TrsBatchSoa::with_capacity;
    let _ = khora_core::math::simd::TrsBatchSoa::len;
    let _ = khora_core::math::simd::TrsBatchSoa::is_empty;
    let _ = khora_core::math::simd::TrsBatchSoa::clear;
    let _ = khora_core::math::simd::TrsBatchSoa::push;
    is_debug::<khora_core::math::simd::TrsBatchSoa>();
    is_default::<khora_core::math::simd::TrsBatchSoa>();
    is_clone::<khora_core::math::simd::TrsBatchSoa>();
    let _ = khora_core::math::simd::compose_trs_to_mat4;
    let _ = khora_core::math::simd::compose_trs_to_mat4_scalar;
    let _ = khora_core::math::simd::normalize_quat_batch;
    let _ = type_name::<khora_core::math::vector::Vec2>();
    let _ = type_name::<khora_core::math::Vec2>();
    same_type(
        PhantomData::<khora_core::math::Vec2>,
        PhantomData::<khora_core::math::vector::Vec2>,
    );
    let _ = vec2_fields as fn(&khora_core::math::vector::Vec2);
    let _ = khora_core::math::vector::Vec2::ZERO;
    let _ = khora_core::math::vector::Vec2::ONE;
    let _ = khora_core::math::vector::Vec2::X;
    let _ = khora_core::math::vector::Vec2::Y;
    let _ = khora_core::math::vector::Vec2::new;
    let _ = khora_core::math::vector::Vec2::abs;
    let _ = khora_core::math::vector::Vec2::length_squared;
    let _ = khora_core::math::vector::Vec2::length;
    let _ = khora_core::math::vector::Vec2::normalize;
    let _ = khora_core::math::vector::Vec2::dot;
    let _ = khora_core::math::vector::Vec2::lerp;
    let _ = khora_core::math::vector::Vec2::to_array;
    is_debug::<khora_core::math::vector::Vec2>();
    is_default::<khora_core::math::vector::Vec2>();
    is_copy::<khora_core::math::vector::Vec2>();
    is_clone::<khora_core::math::vector::Vec2>();
    is_partial_eq::<khora_core::math::vector::Vec2>();
    is_pod::<khora_core::math::vector::Vec2>();
    is_zeroable::<khora_core::math::vector::Vec2>();
    is_serialize::<khora_core::math::vector::Vec2>();
    is_deserialize_owned::<khora_core::math::vector::Vec2>();
    is_from_vec2::<[f32; 2]>();
    is_from_f32_2::<khora_core::math::vector::Vec2>();
    is_add::<khora_core::math::vector::Vec2>();
    is_sub::<khora_core::math::vector::Vec2>();
    is_mul_f32::<khora_core::math::vector::Vec2>();
    is_mul_vec2::<f32>();
    is_mul::<khora_core::math::vector::Vec2>();
    is_div_f32::<khora_core::math::vector::Vec2>();
    is_neg::<khora_core::math::vector::Vec2>();
    is_index_usize::<khora_core::math::vector::Vec2>();
    is_index_mut_usize::<khora_core::math::vector::Vec2>();
    let _ = type_name::<khora_core::math::vector::Vec3>();
    let _ = type_name::<khora_core::math::Vec3>();
    same_type(
        PhantomData::<khora_core::math::Vec3>,
        PhantomData::<khora_core::math::vector::Vec3>,
    );
    let _ = vec3_fields as fn(&khora_core::math::vector::Vec3);
    let _ = khora_core::math::vector::Vec3::ZERO;
    let _ = khora_core::math::vector::Vec3::ONE;
    let _ = khora_core::math::vector::Vec3::X;
    let _ = khora_core::math::vector::Vec3::Y;
    let _ = khora_core::math::vector::Vec3::Z;
    let _ = khora_core::math::vector::Vec3::NEG_X;
    let _ = khora_core::math::vector::Vec3::NEG_Y;
    let _ = khora_core::math::vector::Vec3::NEG_Z;
    let _ = khora_core::math::vector::Vec3::new;
    let _ = khora_core::math::vector::Vec3::abs;
    let _ = khora_core::math::vector::Vec3::max_element;
    let _ = khora_core::math::vector::Vec3::min_element;
    let _ = khora_core::math::vector::Vec3::length_squared;
    let _ = khora_core::math::vector::Vec3::length;
    let _ = khora_core::math::vector::Vec3::normalize;
    let _ = khora_core::math::vector::Vec3::dot;
    let _ = khora_core::math::vector::Vec3::cross;
    let _ = khora_core::math::vector::Vec3::distance_squared;
    let _ = khora_core::math::vector::Vec3::distance;
    let _ = khora_core::math::vector::Vec3::lerp;
    let _ = khora_core::math::vector::Vec3::get;
    is_debug::<khora_core::math::vector::Vec3>();
    is_clone::<khora_core::math::vector::Vec3>();
    is_copy::<khora_core::math::vector::Vec3>();
    is_partial_eq::<khora_core::math::vector::Vec3>();
    is_pod::<khora_core::math::vector::Vec3>();
    is_zeroable::<khora_core::math::vector::Vec3>();
    is_serialize::<khora_core::math::vector::Vec3>();
    is_deserialize_owned::<khora_core::math::vector::Vec3>();
    is_default::<khora_core::math::vector::Vec3>();
    is_add::<khora_core::math::vector::Vec3>();
    is_sub::<khora_core::math::vector::Vec3>();
    is_mul_f32::<khora_core::math::vector::Vec3>();
    is_mul_vec3::<f32>();
    is_mul::<khora_core::math::vector::Vec3>();
    is_div_f32::<khora_core::math::vector::Vec3>();
    is_neg::<khora_core::math::vector::Vec3>();
    is_index_usize::<khora_core::math::vector::Vec3>();
    is_index_mut_usize::<khora_core::math::vector::Vec3>();
    let _ = type_name::<khora_core::math::vector::Vec4>();
    let _ = type_name::<khora_core::math::Vec4>();
    same_type(
        PhantomData::<khora_core::math::Vec4>,
        PhantomData::<khora_core::math::vector::Vec4>,
    );
    let _ = vec4_fields as fn(&khora_core::math::vector::Vec4);
    let _ = khora_core::math::vector::Vec4::ZERO;
    let _ = khora_core::math::vector::Vec4::ONE;
    let _ = khora_core::math::vector::Vec4::X;
    let _ = khora_core::math::vector::Vec4::Y;
    let _ = khora_core::math::vector::Vec4::Z;
    let _ = khora_core::math::vector::Vec4::W;
    let _ = khora_core::math::vector::Vec4::new;
    let _ = khora_core::math::vector::Vec4::abs;
    let _ = khora_core::math::vector::Vec4::from_vec3;
    let _ = khora_core::math::vector::Vec4::truncate;
    let _ = khora_core::math::vector::Vec4::dot;
    let _ = khora_core::math::vector::Vec4::get;
    let _ = khora_core::math::vector::Vec4::to_array;
    is_debug::<khora_core::math::vector::Vec4>();
    is_default::<khora_core::math::vector::Vec4>();
    is_copy::<khora_core::math::vector::Vec4>();
    is_clone::<khora_core::math::vector::Vec4>();
    is_partial_eq::<khora_core::math::vector::Vec4>();
    is_pod::<khora_core::math::vector::Vec4>();
    is_zeroable::<khora_core::math::vector::Vec4>();
    is_serialize::<khora_core::math::vector::Vec4>();
    is_deserialize_owned::<khora_core::math::vector::Vec4>();
    is_from_vec4::<[f32; 4]>();
    is_from_f32_4::<khora_core::math::vector::Vec4>();
    is_add::<khora_core::math::vector::Vec4>();
    is_sub::<khora_core::math::vector::Vec4>();
    is_mul_f32::<khora_core::math::vector::Vec4>();
    is_mul_vec4::<f32>();
    is_mul::<khora_core::math::vector::Vec4>();
    is_div_f32::<khora_core::math::vector::Vec4>();
    is_neg::<khora_core::math::vector::Vec4>();
    is_index_usize::<khora_core::math::vector::Vec4>();
    is_index_mut_usize::<khora_core::math::vector::Vec4>();
}
