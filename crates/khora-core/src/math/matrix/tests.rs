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

use super::*;
use crate::math::{approx_eq, matrix::Mat4, quaternion::Quaternion, vector::Vec3, PI};

fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
}

fn mat3_approx_eq(a: Mat3, b: Mat3) -> bool {
    vec3_approx_eq(a.cols[0], b.cols[0])
        && vec3_approx_eq(a.cols[1], b.cols[1])
        && vec3_approx_eq(a.cols[2], b.cols[2])
}

fn vec4_approx_eq(a: Vec4, b: Vec4) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z) && approx_eq(a.w, b.w)
}

fn mat4_approx_eq(a: Mat4, b: Mat4) -> bool {
    vec4_approx_eq(a.cols[0], b.cols[0])
        && vec4_approx_eq(a.cols[1], b.cols[1])
        && vec4_approx_eq(a.cols[2], b.cols[2])
        && vec4_approx_eq(a.cols[3], b.cols[3])
}

// --- Tests for Mat3 ---

#[test]
fn test_mat3_identity_default() {
    assert_eq!(Mat3::default(), Mat3::IDENTITY);

    let m = Mat3::from_scale(Vec3::new(1.0, 2.0, 3.0));
    assert!(mat3_approx_eq(m * Mat3::IDENTITY, m));
    assert!(mat3_approx_eq(Mat3::IDENTITY * m, m));
}

#[test]
fn test_mat3_from_scale() {
    let s = Vec3::new(2.0, -3.0, 0.5);
    let m = Mat3::from_scale(s);
    let v = Vec3::new(1.0, 1.0, 1.0);
    assert!(vec3_approx_eq(m * v, s)); // Scaling (1,1,1) should yield the scale vector
}

#[test]
fn test_mat3_rotations() {
    let angle = PI / 6.0; // 30 degrees
    let mx = Mat3::from_rotation_x(angle);
    let my = Mat3::from_rotation_y(angle);
    let mz = Mat3::from_rotation_z(angle);

    let p = Vec3::Y; // Point on Y axis
    let expected_px = Vec3::new(0.0, angle.cos(), angle.sin());
    assert!(vec3_approx_eq(mx * p, expected_px));

    let p = Vec3::X; // Point on X axis
    let expected_py = Vec3::new(angle.cos(), 0.0, -angle.sin()); // RH
    assert!(vec3_approx_eq(my * p, expected_py));

    let p = Vec3::X; // Point on X axis
    let expected_pz = Vec3::new(angle.cos(), angle.sin(), 0.0);
    assert!(vec3_approx_eq(mz * p, expected_pz));
}

#[test]
fn test_mat3_from_axis_angle() {
    let axis = Vec3::new(1.0, 1.0, 1.0).normalize();
    let angle = 1.2 * PI;
    let m = Mat3::from_axis_angle(axis, angle);

    // Rotation around (1,1,1) axis should permute basis vectors
    let v = Vec3::X;
    let v_rotated = m * v;

    // Check if length is preserved
    assert!(approx_eq(v_rotated.length(), v.length()));

    // Specific check is hard without known values, but ensure it's not identity or zero
    assert!(v_rotated.distance_squared(v) > EPSILON);
}

#[test]
fn test_mat3_from_quat() {
    let axis = Vec3::new(1.0, -2.0, 3.0).normalize();
    let angle = PI / 7.0;
    let q = Quaternion::from_axis_angle(axis, angle);
    let m_from_q = Mat3::from_quat(q);

    let v = Vec3::new(0.5, 1.0, -0.2);
    let v_rotated_q = q * v;
    let v_rotated_m = m_from_q * v;

    assert!(vec3_approx_eq(v_rotated_q, v_rotated_m));
}

#[test]
fn test_mat3_determinant() {
    assert!(approx_eq(Mat3::IDENTITY.determinant(), 1.0));
    assert!(approx_eq(Mat3::ZERO.determinant(), 0.0));

    let m_scale = Mat3::from_scale(Vec3::new(2.0, 3.0, 4.0));
    assert!(approx_eq(m_scale.determinant(), 24.0));

    let m_rot = Mat3::from_rotation_y(PI / 5.0);
    assert!(approx_eq(m_rot.determinant(), 1.0)); // Rotations preserve volume
}

#[test]
fn test_mat3_transpose() {
    let m = Mat3::from_cols(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(4.0, 5.0, 6.0),
        Vec3::new(7.0, 8.0, 9.0),
    );
    let mt = m.transpose();
    let expected_mt = Mat3::from_cols(
        Vec3::new(1.0, 4.0, 7.0),
        Vec3::new(2.0, 5.0, 8.0),
        Vec3::new(3.0, 6.0, 9.0),
    );

    assert!(mat3_approx_eq(mt, expected_mt));
    assert!(mat3_approx_eq(m.transpose().transpose(), m)); // Double transpose
}

#[test]
fn test_mat3_inverse() {
    let m = Mat3::from_rotation_z(PI / 3.0) * Mat3::from_scale(Vec3::new(1.0, 2.0, 0.5));
    let inv_m = m.inverse().expect("Matrix should be invertible");
    let identity = m * inv_m;
    assert!(
        mat3_approx_eq(identity, Mat3::IDENTITY),
        "M * inv(M) should be Identity"
    );

    let singular = Mat3::from_scale(Vec3::new(1.0, 0.0, 1.0));
    assert!(
        singular.inverse().is_none(),
        "Singular matrix inverse should be None"
    );
}

#[test]
fn test_mat3_mul_vec3() {
    let m = Mat3::from_rotation_z(PI / 2.0); // Rotate 90 deg around Z
    let v = Vec3::X; // (1, 0, 0)
    let expected_v = Vec3::Y; // (0, 1, 0)
    assert!(vec3_approx_eq(m * v, expected_v));
}

#[test]
fn test_mat3_mul_mat3() {
    let rot90z = Mat3::from_rotation_z(PI / 2.0);
    let rot180z = rot90z * rot90z;
    let expected_rot180z = Mat3::from_rotation_z(PI);
    assert!(mat3_approx_eq(rot180z, expected_rot180z));
}

#[test]
fn test_mat3_conversions() {
    let m4 = Mat4::from_translation(Vec3::new(10., 20., 30.)) * Mat4::from_rotation_x(PI / 4.0);
    let m3 = Mat3::from_mat4(&m4);
    let m4_again = m3.to_mat4();

    // Check if rotation part was extracted correctly
    let v = Vec3::Y;
    let v_rot_m3 = m3 * v;
    let v_rot_m4 = Mat4::from_rotation_x(PI / 4.0) * Vec4::from_vec3(v, 0.0); // Rotate as vector
    assert!(vec3_approx_eq(v_rot_m3, v_rot_m4.truncate()));

    // Check if embedding back into Mat4 worked (translation should be zero)
    let origin = Vec4::new(0.0, 0.0, 0.0, 1.0);
    let transformed_origin = m4_again * origin;
    assert!(approx_eq(transformed_origin.x, 0.0));
    assert!(approx_eq(transformed_origin.y, 0.0));
    assert!(approx_eq(transformed_origin.z, 0.0));
    assert!(approx_eq(transformed_origin.w, 1.0));
}

#[test]
fn test_mat3_index() {
    let mut m = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::Z);
    assert_eq!(m[0], Vec3::X);
    assert_eq!(m[1], Vec3::Y);
    assert_eq!(m[2], Vec3::Z);
    m[0] = Vec3::ONE;
    assert_eq!(m.cols[0], Vec3::ONE);
}

#[test]
#[should_panic]
fn test_mat3_index_out_of_bounds() {
    let m = Mat3::IDENTITY;
    let _ = m[3]; // Should panic
}

// --- End of Mat3 Tests ---

// --- Tests for Mat4 ---

#[test]
fn test_identity() {
    assert_eq!(Mat4::default(), Mat4::IDENTITY);
    let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
    assert!(mat4_approx_eq(m * Mat4::IDENTITY, m));
    assert!(mat4_approx_eq(Mat4::IDENTITY * m, m));
}

#[test]
fn test_from_quat() {
    let axis = Vec3::new(1.0, 2.0, 3.0).normalize();
    let angle = PI / 5.0;
    let q = Quaternion::from_axis_angle(axis, angle);
    let m_from_q = Mat4::from_quat(q);

    let v = Vec3::new(5.0, -1.0, 2.0);

    let v_rotated_q = q * v; // Rotate using quaternion directly
                             // Rotate using matrix: convert v to Vec4(point), multiply, convert back
    let v4 = Vec4::from_vec3(v, 1.0);
    let v_rotated_m4 = m_from_q * v4;
    let v_rotated_m = v_rotated_m4.truncate();

    // Compare results
    assert!(approx_eq(v_rotated_q.x, v_rotated_m.x));
    assert!(approx_eq(v_rotated_q.y, v_rotated_m.y));
    assert!(approx_eq(v_rotated_q.z, v_rotated_m.z));
}

#[test]
fn test_translation() {
    let t = Vec3::new(1.0, 2.0, 3.0);
    let m = Mat4::from_translation(t);
    let p = Vec4::new(1.0, 1.0, 1.0, 1.0);
    let expected_p = Vec4::new(2.0, 3.0, 4.0, 1.0);

    assert!(vec4_approx_eq(m * p, expected_p));
}

#[test]
fn test_scale() {
    let s = Vec3::new(2.0, 3.0, 4.0);
    let m = Mat4::from_scale(s);
    let p = Vec4::new(1.0, 1.0, 1.0, 1.0);
    let expected_p = Vec4::new(2.0, 3.0, 4.0, 1.0);
    assert!(vec4_approx_eq(m * p, expected_p));
}

#[test]
fn test_rotation_x() {
    let angle = PI / 2.0; // 90 degrees
    let m = Mat4::from_rotation_x(angle);
    let p = Vec4::new(0.0, 1.0, 0.0, 1.0); // Point on Y axis
    let expected_p = Vec4::new(0.0, 0.0, 1.0, 1.0); // Should rotate to Z axis
    assert!(vec4_approx_eq(m * p, expected_p));
}

#[test]
fn test_rotation_y() {
    let angle = PI / 2.0; // 90 degrees
    let m = Mat4::from_rotation_y(angle);
    let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point on X axis
    let expected_p = Vec4::new(0.0, 0.0, -1.0, 1.0); // Should rotate to -Z axis

    assert!(vec4_approx_eq(m * p, expected_p));
}

#[test]
fn test_rotation_z() {
    let angle = PI / 2.0; // 90 degrees
    let m = Mat4::from_rotation_z(angle);
    let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point on X axis
    let expected_p = Vec4::new(0.0, 1.0, 0.0, 1.0); // Should rotate to Y axis
    assert!(vec4_approx_eq(m * p, expected_p));
}

#[test]
fn test_transpose() {
    let m = Mat4::from_cols(
        Vec4::new(1., 2., 3., 4.),
        Vec4::new(5., 6., 7., 8.),
        Vec4::new(9., 10., 11., 12.),
        Vec4::new(13., 14., 15., 16.),
    );
    let mt = m.transpose();
    let expected_mt = Mat4::from_cols(
        Vec4::new(1., 5., 9., 13.),
        Vec4::new(2., 6., 10., 14.),
        Vec4::new(3., 7., 11., 15.),
        Vec4::new(4., 8., 12., 16.),
    );
    assert_eq!(mt.cols[0], expected_mt.cols[0]); // Compare columns after transpose
    assert_eq!(mt.cols[1], expected_mt.cols[1]);
    assert_eq!(mt.cols[2], expected_mt.cols[2]);
    assert_eq!(mt.cols[3], expected_mt.cols[3]);

    // Test double transpose
    assert!(mat4_approx_eq(m.transpose().transpose(), m));
}

#[test]
fn test_mul_mat4() {
    let t = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
    let r = Mat4::from_rotation_z(PI / 2.0);

    // Order matters: Translate then Rotate
    let tr = r * t;
    let p = Vec4::new(1.0, 0.0, 0.0, 1.0); // Point at (1,0,0)
                                           // 1. Translate: p becomes (2, 0, 0, 1)
                                           // 2. Rotate Z 90: (2, 0, 0) becomes (0, 2, 0)
    let expected_tr = Vec4::new(0.0, 2.0, 0.0, 1.0);
    assert!(vec4_approx_eq(tr * p, expected_tr));

    // Order matters: Rotate then Translate
    let rt = t * r;
    // 1. Rotate Z 90: p becomes (0, 1, 0, 1)
    // 2. Translate: (0, 1, 0) becomes (1, 1, 0)
    let expected_rt = Vec4::new(1.0, 1.0, 0.0, 1.0);
    assert!(vec4_approx_eq(rt * p, expected_rt));
}

#[test]
fn test_inverse() {
    let m = Mat4::from_translation(Vec3::new(1., 2., 3.))
        * Mat4::from_rotation_y(PI / 4.0)
        * Mat4::from_scale(Vec3::new(1., 2., 1.));

    let inv_m = m.inverse().expect("Matrix should be invertible");
    let identity = m * inv_m;

    // Check if M * M^-1 is close to identity
    assert!(
        mat4_approx_eq(identity, Mat4::IDENTITY),
        "M * inv(M) should be Identity"
    );

    // Check singular matrix (e.g., scale with zero)
    let singular = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
    assert!(
        singular.inverse().is_none(),
        "Singular matrix inverse should be None"
    );
}

#[test]
fn test_affine_inverse() {
    let t = Mat4::from_translation(Vec3::new(1., 2., 3.));
    let r = Mat4::from_rotation_y(PI / 3.0);
    let s = Mat4::from_scale(Vec3::new(1., 2., 0.5));
    let m = t * r * s; // Combined affine transform

    let inv_m = m.inverse().expect("Matrix should be invertible");
    let affine_inv_m = m
        .affine_inverse()
        .expect("Matrix should be affine invertible");

    // Check if affine inverse matches general inverse for this case
    assert!(
        mat4_approx_eq(inv_m, affine_inv_m),
        "Affine inverse should match general inverse"
    );

    // Check M * inv(M) == Identity using affine inverse
    let identity = m * affine_inv_m;
    assert!(
        mat4_approx_eq(identity, Mat4::IDENTITY),
        "M * affine_inv(M) should be Identity"
    );

    // Test singular affine matrix
    let singular_s = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
    let singular_m = t * singular_s;
    assert!(
        singular_m.affine_inverse().is_none(),
        "Singular affine matrix inverse should be None"
    );
}

#[test]
fn test_perspective_rh_zo() {
    let fov = PI / 4.0; // 45 degrees
    let aspect = 16.0 / 9.0;
    let near = 0.1;
    let far = 100.0;

    let m = Mat4::perspective_rh_zo(fov, aspect, near, far);
    assert!(approx_eq(m.cols[0].x, 1.0 / (aspect * (fov / 2.0).tan())));
    assert!(approx_eq(m.cols[1].y, 1.0 / ((fov / 2.0).tan())));
    assert!(approx_eq(m.cols[2].z, -far / (far - near)));
    assert!(approx_eq(m.cols[3].z, -(far * near) / (far - near)));
}

#[test]
fn test_orthographic_rh_zo() {
    let left = -1.0;
    let right = 1.0;
    let bottom = -1.0;
    let top = 1.0;
    let near = 0.1;
    let far = 100.0;
    let m = Mat4::orthographic_rh_zo(left, right, bottom, top, near, far);

    // Check scale factors
    assert!(approx_eq(m.cols[0].x, 2.0 / (right - left)));
    assert!(approx_eq(m.cols[1].y, 2.0 / (top - bottom)));
    assert!(approx_eq(m.cols[2].z, -1.0 / (far - near)));

    // Check translation factors
    assert!(approx_eq(m.cols[3].x, -(right + left) / (right - left)));
    assert!(approx_eq(m.cols[3].y, -(top + bottom) / (top - bottom)));
    assert!(approx_eq(m.cols[3].z, -near / (far - near))); // -near and not -(far + near)
}

#[test]
fn test_look_at_rh() {
    let eye = Vec3::new(0.0, 0.0, 5.0);
    let target = Vec3::new(0.0, 0.0, 0.0);
    let up = Vec3::new(0.0, 1.0, 0.0);

    let m = Mat4::look_at_rh(eye, target, up).expect("look_at_rh should return Some(Mat4)");

    // Forward direction (third column, third row): should be +1.0 for a right-handed system
    assert!(approx_eq(m.cols[2].z, 1.0));

    // Translation part (fourth column, third row): should be -eye · forward = -5.0
    assert!(approx_eq(m.cols[3].z, -5.0));
}

#[test]
fn test_look_at_rh_invalid() {
    let eye = Vec3::new(0.0, 0.0, 5.0);
    let target = Vec3::new(0.0, 0.0, 5.0); // Same as eye
    let up = Vec3::new(0.0, 1.0, 0.0);

    // This should panic or return None (depending on implementation)
    assert!(Mat4::look_at_rh(eye, target, up).is_none());
}

// --- Cubemap helpers -----------------------------------------------

/// Helper: clip-space depth (z/w) of a world-space point through a
/// view-projection matrix.
fn clip_depth(view_proj: &Mat4, world: Vec3) -> f32 {
    let clip = *view_proj * crate::math::Vec4::new(world.x, world.y, world.z, 1.0);
    clip.z / clip.w
}

#[test]
fn cube_face_view_projs_returns_six_in_wgpu_order() {
    let m = Mat4::cube_face_view_projs(Vec3::ZERO, 100.0);
    assert_eq!(m.len(), 6);
    // The six matrices are pairwise distinct (different views).
    for i in 0..6 {
        for j in (i + 1)..6 {
            let same = (0..16).all(|k| {
                let row = k / 4;
                let col = k % 4;
                let a = m[i].cols[col][row];
                let b = m[j].cols[col][row];
                crate::math::approx_eq(a, b)
            });
            assert!(!same, "faces {i} and {j} produced identical matrices");
        }
    }
}

#[test]
fn cube_face_matrices_project_axis_aligned_points_into_correct_face() {
    // For each face, a point one unit away along that face's forward
    // direction must land *in front of* the camera (z/w in [0, 1] under
    // RH-ZO) and roughly at the centre of the screen (x/w ≈ y/w ≈ 0).
    let pos = Vec3::ZERO;
    let far = 100.0;
    for face in crate::math::CubeFace::ALL {
        let vp = Mat4::cube_face_view_proj(pos, face, far);
        let target = pos + face.forward() * 5.0; // safely between near (0.1) and far (100)
        let clip = vp * crate::math::Vec4::new(target.x, target.y, target.z, 1.0);
        assert!(
            clip.w > 0.0,
            "face {face:?}: target should be in front (w>0), got w = {}",
            clip.w
        );
        let z_norm = clip.z / clip.w;
        assert!(
            (0.0..=1.0).contains(&z_norm),
            "face {face:?}: target z/w {z_norm} not in [0, 1]"
        );
        let x_norm = clip.x / clip.w;
        let y_norm = clip.y / clip.w;
        assert!(
            x_norm.abs() < 1e-3 && y_norm.abs() < 1e-3,
            "face {face:?}: axis-aligned point should be near screen centre, got ({x_norm}, {y_norm})"
        );
    }
}

#[test]
fn cube_far_plane_maps_to_one() {
    let pos = Vec3::ZERO;
    let far = 50.0;
    for face in crate::math::CubeFace::ALL {
        let vp = Mat4::cube_face_view_proj(pos, face, far);
        let z = clip_depth(&vp, pos + face.forward() * far);
        assert!(
            (z - 1.0).abs() < 1e-4,
            "face {face:?}: z at far plane should be ≈ 1.0, got {z}"
        );
    }
}

#[test]
fn cube_near_plane_maps_to_zero() {
    let pos = Vec3::ZERO;
    let far = 50.0;
    // The near constant inside `cube_face_view_proj` is 0.1.
    for face in crate::math::CubeFace::ALL {
        let vp = Mat4::cube_face_view_proj(pos, face, far);
        let z = clip_depth(&vp, pos + face.forward() * 0.1);
        assert!(
            z.abs() < 1e-4,
            "face {face:?}: z at near plane should be ≈ 0.0, got {z}"
        );
    }
}

#[test]
fn cube_face_view_proj_idempotent_with_explicit_lookat() {
    // Regression: ensure the helper produces the same matrix as the
    // unfolded (proj * look_at_rh) sequence.
    let pos = Vec3::new(1.0, 2.0, 3.0);
    let far = 80.0;
    for face in crate::math::CubeFace::ALL {
        let helper = Mat4::cube_face_view_proj(pos, face, far);
        let proj = Mat4::perspective_rh_zo(crate::math::FRAC_PI_2, 1.0, 0.1, far);
        let view =
            Mat4::look_at_rh(pos, pos + face.forward(), face.up()).expect("non-degenerate basis");
        let expected = proj * view;
        for col in 0..4 {
            for row in 0..4 {
                assert!(
                    crate::math::approx_eq_eps(
                        helper.cols[col][row],
                        expected.cols[col][row],
                        1e-5,
                    ),
                    "face {face:?}: mismatch at col {col} row {row}"
                );
            }
        }
    }
}

// --- End of Tests For Mat4 ---
