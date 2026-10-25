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

use crate::ecs::FieldProblem;
use khora_core::math::Mat4;
use khora_core::problems::ProblemSeverity;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Defines the type of camera projection.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProjectionType {
    /// Perspective projection with field of view.
    Perspective {
        /// The vertical field of view in radians.
        fov_y_radians: f32,
    },
    /// Orthographic projection with a fixed vertical extent.
    ///
    /// Its width is not stored: it is `height` times the aspect of the image
    /// the camera fills, so the view widens with the window instead of
    /// stretching.
    Orthographic {
        /// The height of the orthographic view volume, in world units.
        height: f32,
    },
}

/// A component that defines a camera's projection parameters.
///
/// This component is used to configure how the 3D world is projected onto the 2D screen.
/// It supports both perspective and orthographic projections.
///
/// A camera has no aspect ratio of its own. The aspect is the shape of the
/// image it fills — the window, or the editor's viewport — and whoever
/// projects the camera reads it from that image every frame
/// ([`projection_matrix`](Self::projection_matrix) takes it as an argument).
/// A stored aspect could only disagree with the image, and an image drawn at
/// an aspect other than its own is stretched. A frame of a fixed shape on a
/// window of another shape is letterboxing: the camera fills a sub-rectangle
/// of the target, whose own shape then gives the aspect.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[component(domain = Render, check = Camera::check)]
pub struct Camera {
    /// The type of projection (perspective or orthographic).
    pub projection: ProjectionType,

    /// The distance to the near clipping plane.
    /// Objects closer than this will not be rendered.
    /// Should be a small positive value (e.g., 0.1).
    pub z_near: f32,

    /// The distance to the far clipping plane.
    /// Objects farther than this will not be rendered.
    /// Should be larger than `z_near` (e.g., 1000.0).
    pub z_far: f32,

    /// Whether this camera is the active/primary camera.
    /// Only one camera should be active at a time.
    pub is_active: bool,
}

impl Camera {
    /// Creates a new perspective camera with the given vertical field of view
    /// and clip planes.
    pub fn new_perspective(fov_y_radians: f32, z_near: f32, z_far: f32) -> Self {
        Self {
            projection: ProjectionType::Perspective { fov_y_radians },
            z_near,
            z_far,
            is_active: true,
        }
    }

    /// Creates a new orthographic camera showing `height` world units
    /// vertically; its width follows the image it fills.
    pub fn new_orthographic(height: f32, z_near: f32, z_far: f32) -> Self {
        Self {
            projection: ProjectionType::Orthographic { height },
            z_near,
            z_far,
            is_active: true,
        }
    }

    /// Creates a default perspective camera suitable for most 3D applications.
    ///
    /// - FOV: 60 degrees (~1.047 radians)
    /// - Near plane: 0.1
    /// - Far plane: 1000.0
    pub fn default_perspective() -> Self {
        Self::new_perspective(60.0_f32.to_radians(), 0.1, 1000.0)
    }

    /// Creates a default orthographic camera.
    ///
    /// - Height: 1080.0
    /// - Near plane: -1.0
    /// - Far plane: 1000.0
    pub fn default_orthographic() -> Self {
        Self::new_orthographic(1080.0, -1.0, 1000.0)
    }

    /// Calculates the projection matrix for this camera, filling an image
    /// whose width over height is `aspect`.
    ///
    /// This uses a right-handed coordinate system with a [0, 1] depth range,
    /// which is standard for modern rendering APIs like Vulkan and WebGPU.
    ///
    /// A camera whose values make no image is an `Err`, never a panic: the
    /// first of its [`problems`](Self::problems), or
    /// [`CameraProblem::Unprojectable`] when every field is usable but the
    /// projection still cannot be built (an overflow, or a bad `aspect`).
    /// The camera's values are never changed.
    pub fn projection_matrix(&self, aspect: f32) -> Result<Mat4, CameraProblem> {
        if let Some(problem) = self.problems().first() {
            return Err(*problem);
        }
        self.try_project(aspect).ok_or(CameraProblem::Unprojectable)
    }

    /// The projection at `aspect`, or `None` when the math refuses it.
    fn try_project(&self, aspect: f32) -> Option<Mat4> {
        match self.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                Mat4::try_perspective_rh_zo(fov_y_radians, aspect, self.z_near, self.z_far)
            }
            ProjectionType::Orthographic { height } => {
                let half_height = height / 2.0;
                let half_width = half_height * aspect;
                Mat4::try_orthographic_rh_zo(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    self.z_near,
                    self.z_far,
                )
            }
        }
    }

    /// Every problem in this camera's values, in field order
    /// (`projection`, `z_near`, `z_far`). Empty when the camera can be
    /// projected.
    ///
    /// Perspective: a non-finite `fov_y_radians`, `z_near` or `z_far` is
    /// [`CameraProblem::NotFinite`]; a field of view outside `(0, π)` is
    /// [`CameraProblem::FieldOfView`]; a near plane `<= 0` is
    /// [`CameraProblem::NearNotPositive`]; finite planes with `far <= near`
    /// are [`CameraProblem::FarNotBeyondNear`]. A NaN near is `NotFinite`
    /// only. Orthographic: a non-finite or non-positive `height` is
    /// `NotFinite` / [`CameraProblem::Height`]; a non-finite plane is
    /// `NotFinite`; `far <= near` is `FarNotBeyondNear`; a negative near is
    /// fine. When every field passes but the projection cannot be built at
    /// aspect 1, [`CameraProblem::Unprojectable`].
    pub fn problems(&self) -> Vec<CameraProblem> {
        let mut problems = Vec::new();
        match self.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                if !fov_y_radians.is_finite() {
                    problems.push(CameraProblem::NotFinite { field: FOV_FIELD });
                } else if !(fov_y_radians > 0.0 && fov_y_radians < std::f32::consts::PI) {
                    problems.push(CameraProblem::FieldOfView { fov_y_radians });
                }
                if !self.z_near.is_finite() {
                    problems.push(CameraProblem::NotFinite { field: NEAR_FIELD });
                } else if self.z_near <= 0.0 {
                    problems.push(CameraProblem::NearNotPositive { near: self.z_near });
                }
            }
            ProjectionType::Orthographic { height } => {
                if !height.is_finite() {
                    problems.push(CameraProblem::NotFinite {
                        field: HEIGHT_FIELD,
                    });
                } else if height <= 0.0 {
                    problems.push(CameraProblem::Height { height });
                }
                if !self.z_near.is_finite() {
                    problems.push(CameraProblem::NotFinite { field: NEAR_FIELD });
                }
            }
        }
        if !self.z_far.is_finite() {
            problems.push(CameraProblem::NotFinite { field: FAR_FIELD });
        } else if self.z_near.is_finite() && self.z_far <= self.z_near {
            problems.push(CameraProblem::FarNotBeyondNear {
                near: self.z_near,
                far: self.z_far,
            });
        }
        if problems.is_empty() && self.try_project(1.0).is_none() {
            problems.push(CameraProblem::Unprojectable);
        }
        problems
    }

    /// The camera's [`problems`](Self::problems) as [`FieldProblem`]s, each
    /// on the dotted path of its field in the serialized form, with severity
    /// `Error` and the problem's message.
    pub fn check(&self) -> Vec<FieldProblem> {
        self.problems()
            .into_iter()
            .map(|problem| FieldProblem {
                field: problem.field(),
                severity: ProblemSeverity::Error,
                message: problem.to_string(),
            })
            .collect()
    }
}

/// Why a [`Camera`]'s values make no image.
///
/// Each problem names its field ([`CameraProblem::field`], the dotted path
/// in the camera's serialized form) and says what is wrong in plain English
/// (`Display`): `near must be greater than 0 (it is 0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraProblem {
    /// A field is NaN or infinite.
    NotFinite {
        /// The dotted path of the field.
        field: &'static str,
    },
    /// A perspective near plane at or behind the eye (`z_near <= 0`).
    NearNotPositive {
        /// The near plane.
        near: f32,
    },
    /// The far plane is not beyond the near one (`z_far <= z_near`).
    FarNotBeyondNear {
        /// The near plane.
        near: f32,
        /// The far plane.
        far: f32,
    },
    /// A perspective field of view outside `(0, π)`.
    FieldOfView {
        /// The field of view, in radians.
        fov_y_radians: f32,
    },
    /// An orthographic height that is not positive.
    Height {
        /// The height.
        height: f32,
    },
    /// Every field is usable, but the projection still cannot be built.
    Unprojectable,
}

impl CameraProblem {
    /// The dotted path, in the camera's serialized form, of the field this
    /// problem is about: `"z_near"`, `"z_far"`,
    /// `"projection.Perspective.fov_y_radians"`,
    /// `"projection.Orthographic.height"`, or `"projection"` for
    /// [`CameraProblem::Unprojectable`].
    pub fn field(&self) -> &'static str {
        match self {
            Self::NotFinite { field } => field,
            Self::NearNotPositive { .. } => NEAR_FIELD,
            Self::FarNotBeyondNear { .. } => FAR_FIELD,
            Self::FieldOfView { .. } => FOV_FIELD,
            Self::Height { .. } => HEIGHT_FIELD,
            Self::Unprojectable => "projection",
        }
    }
}

impl std::fmt::Display for CameraProblem {
    /// The problem in plain English, naming the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFinite { field } => write!(f, "{field} must be a finite number"),
            Self::NearNotPositive { near } => {
                write!(f, "near must be greater than 0 (it is {near})")
            }
            Self::FarNotBeyondNear { near, far } => write!(
                f,
                "far must be greater than near (far is {far}, near is {near})"
            ),
            Self::FieldOfView { fov_y_radians } => write!(
                f,
                "field of view must be between 0 and π radians (it is {fov_y_radians})"
            ),
            Self::Height { height } => {
                write!(f, "height must be greater than 0 (it is {height})")
            }
            Self::Unprojectable => {
                write!(f, "these values are too extreme to make an image")
            }
        }
    }
}

/// The dotted paths of a camera's fields in its serialized form.
const NEAR_FIELD: &str = "z_near";
const FAR_FIELD: &str = "z_far";
const FOV_FIELD: &str = "projection.Perspective.fov_y_radians";
const HEIGHT_FIELD: &str = "projection.Orthographic.height";

impl Default for Camera {
    fn default() -> Self {
        Self::default_perspective()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::{Vec3, Vec4};
    use std::f32::consts::PI;

    /// Where `proj` puts the view-space point `p`, in normalised device
    /// coordinates.
    fn ndc(proj: Mat4, p: Vec3) -> Vec3 {
        let clip = proj * Vec4::new(p.x, p.y, p.z, 1.0);
        Vec3::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w)
    }

    /// How far, in pixels, from the centre of a `width`×`height` image `proj`
    /// draws the view-space point `p`: (horizontal, vertical).
    fn pixels_from_centre(proj: Mat4, p: Vec3, width: f32, height: f32) -> (f32, f32) {
        let n = ndc(proj, p);
        (n.x * width / 2.0, n.y * height / 2.0)
    }

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "{what}: {actual} where {expected} was expected"
        );
    }

    /// The default camera is a 60° perspective, near 0.1, far 1000, active.
    #[test]
    fn the_default_camera_is_a_sixty_degree_perspective() {
        let camera = Camera::default();
        match camera.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                assert_eq!(fov_y_radians, 60.0_f32.to_radians());
            }
            _ => panic!("Expected perspective projection"),
        }
        assert_eq!(camera.z_near, 0.1);
        assert_eq!(camera.z_far, 1000.0);
        assert!(camera.is_active);
    }

    /// `new_perspective` takes the field of view and the clip planes — no
    /// aspect: the aspect belongs to the image the camera fills.
    #[test]
    fn new_perspective_holds_its_fov_and_clip_planes() {
        let camera = Camera::new_perspective(PI / 3.0, 0.5, 500.0);
        assert_eq!(
            camera.projection,
            ProjectionType::Perspective {
                fov_y_radians: PI / 3.0
            }
        );
        assert_eq!(camera.z_near, 0.5);
        assert_eq!(camera.z_far, 500.0);
        assert!(camera.is_active);
    }

    /// `new_orthographic` takes the view volume's height and the clip
    /// planes; its width is the height times the target's aspect.
    #[test]
    fn new_orthographic_holds_its_height_and_clip_planes() {
        let camera = Camera::new_orthographic(10.0, -1.0, 1000.0);
        assert_eq!(
            camera.projection,
            ProjectionType::Orthographic { height: 10.0 }
        );
        assert_eq!(camera.z_near, -1.0);
        assert_eq!(camera.z_far, 1000.0);
        assert!(camera.is_active);
    }

    /// A perspective camera's projection is the right-handed, [0, 1]-depth
    /// perspective of its fov and planes at the aspect it is given.
    #[test]
    fn a_perspective_projection_uses_the_given_aspect() {
        let camera = Camera::new_perspective(PI / 4.0, 0.1, 100.0);
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 1.0, 0.5, 21.0 / 9.0] {
            assert_eq!(
                camera.projection_matrix(aspect).expect("a valid camera"),
                Mat4::perspective_rh_zo(PI / 4.0, aspect, 0.1, 100.0),
                "aspect {aspect}"
            );
        }
    }

    /// The same camera filling two differently shaped images projects
    /// differently: nothing on the camera pins the aspect.
    #[test]
    fn a_projection_follows_the_aspect_it_is_given() {
        let perspective = Camera::new_perspective(PI / 4.0, 0.1, 100.0);
        assert_ne!(
            perspective
                .projection_matrix(4.0 / 3.0)
                .expect("a valid camera"),
            perspective
                .projection_matrix(16.0 / 9.0)
                .expect("a valid camera")
        );
        let orthographic = Camera::new_orthographic(10.0, 0.1, 100.0);
        assert_ne!(
            orthographic
                .projection_matrix(4.0 / 3.0)
                .expect("a valid camera"),
            orthographic
                .projection_matrix(16.0 / 9.0)
                .expect("a valid camera")
        );
    }

    /// On a 1024×768 target, a point one unit right of the view axis and a
    /// point one unit above it, at the same depth, land the same number of
    /// pixels from the image centre — a sphere is drawn round, not as an egg.
    #[test]
    fn a_perspective_camera_draws_round_on_a_four_by_three_target() {
        let (width, height) = (1024.0, 768.0);
        let camera = Camera::new_perspective(PI / 4.0, 0.1, 100.0);
        let proj = camera
            .projection_matrix(width / height)
            .expect("a valid camera");

        let (right_x, right_y) = pixels_from_centre(proj, Vec3::new(1.0, 0.0, -5.0), width, height);
        let (up_x, up_y) = pixels_from_centre(proj, Vec3::new(0.0, 1.0, -5.0), width, height);

        assert_close(right_y, 0.0, "the right point stays on the horizon");
        assert_close(up_x, 0.0, "the up point stays on the vertical");
        assert!(right_x > 0.0 && up_y > 0.0, "{right_x}, {up_y}");
        assert!(
            (right_x - up_y).abs() < 1e-3,
            "one unit right is {right_x} px, one unit up is {up_y} px"
        );
    }

    /// The same holds for a tall target, an ultrawide one and a square one:
    /// roundness does not depend on the window's shape.
    #[test]
    fn a_perspective_camera_draws_round_on_any_target_shape() {
        let camera = Camera::new_perspective(PI / 3.0, 0.1, 100.0);
        for (width, height) in [(768.0, 1024.0), (2560.0, 1080.0), (800.0, 800.0)] {
            let proj = camera
                .projection_matrix(width / height)
                .expect("a valid camera");
            let (right_x, _) = pixels_from_centre(proj, Vec3::new(1.0, 0.0, -3.0), width, height);
            let (_, up_y) = pixels_from_centre(proj, Vec3::new(0.0, 1.0, -3.0), width, height);
            assert!(
                (right_x - up_y).abs() < 1e-3,
                "{width}x{height}: one unit right is {right_x} px, one unit up is {up_y} px"
            );
        }
    }

    /// An orthographic camera of height 10 on a 2:1 target spans 20 across:
    /// x = ±10 and y = ±5 reach the edges of the image.
    #[test]
    fn an_orthographic_camera_is_its_height_times_the_aspect_wide() {
        let camera = Camera::new_orthographic(10.0, 0.1, 100.0);
        let proj = camera.projection_matrix(2.0).expect("a valid camera");
        let z = -50.0;

        assert_close(ndc(proj, Vec3::new(10.0, 0.0, z)).x, 1.0, "x = 10");
        assert_close(ndc(proj, Vec3::new(-10.0, 0.0, z)).x, -1.0, "x = -10");
        assert_close(ndc(proj, Vec3::new(0.0, 5.0, z)).y, 1.0, "y = 5");
        assert_close(ndc(proj, Vec3::new(0.0, -5.0, z)).y, -1.0, "y = -5");
        assert_close(
            ndc(proj, Vec3::new(0.0, 0.0, z)).x,
            0.0,
            "the axis is the centre",
        );
    }

    /// An orthographic camera draws round too: a unit right and a unit up
    /// are the same number of pixels on a 1024×768 target.
    #[test]
    fn an_orthographic_camera_draws_round_on_a_four_by_three_target() {
        let (width, height) = (1024.0, 768.0);
        let camera = Camera::new_orthographic(10.0, 0.1, 100.0);
        let proj = camera
            .projection_matrix(width / height)
            .expect("a valid camera");
        let (right_x, _) = pixels_from_centre(proj, Vec3::new(1.0, 0.0, -5.0), width, height);
        let (_, up_y) = pixels_from_centre(proj, Vec3::new(0.0, 1.0, -5.0), width, height);
        assert!(
            (right_x - up_y).abs() < 1e-3,
            "one unit right is {right_x} px, one unit up is {up_y} px"
        );
    }

    /// Narrow and very wide fields of view still give a usable projection.
    #[test]
    fn extreme_fields_of_view_give_a_non_degenerate_projection() {
        for fov in [0.1, PI * 0.9] {
            let proj = Camera::new_perspective(fov, 0.1, 100.0)
                .projection_matrix(16.0 / 9.0)
                .expect("a valid camera");
            assert_ne!(proj, Mat4::IDENTITY, "fov {fov}");
            assert!(proj.determinant().abs() > 0.0001, "fov {fov}: degenerate");
        }
    }

    /// The active flag is a plain field: cleared, it stays cleared.
    #[test]
    fn the_active_flag_can_be_cleared() {
        let mut camera = Camera::default();
        assert!(camera.is_active);
        camera.is_active = false;
        assert!(!camera.is_active);
    }

    /// `Default` is `default_perspective`.
    #[test]
    fn the_default_camera_is_the_default_perspective() {
        assert_eq!(Camera::default(), Camera::default_perspective());
    }

    /// `default_orthographic` is an active orthographic camera.
    #[test]
    fn the_default_orthographic_camera_is_orthographic() {
        let camera = Camera::default_orthographic();
        assert!(
            matches!(camera.projection, ProjectionType::Orthographic { height } if height > 0.0),
            "{:?}",
            camera.projection
        );
        assert!(camera.is_active);
    }

    // --- A camera checks its own values -------------------------------------

    use crate::ecs::{FieldProblem, World};
    use crate::scene::registration_for;
    use khora_core::problems::ProblemSeverity;
    use std::any::TypeId;

    /// The serialized path of a perspective camera's field of view.
    const FOV_FIELD: &str = "projection.Perspective.fov_y_radians";
    /// The serialized path of an orthographic camera's height.
    const HEIGHT_FIELD: &str = "projection.Orthographic.height";
    /// The aspect the checks below project at: a common window shape.
    const WIDE: f32 = 16.0 / 9.0;

    fn perspective(fov_y_radians: f32, z_near: f32, z_far: f32) -> Camera {
        Camera::new_perspective(fov_y_radians, z_near, z_far)
    }

    fn orthographic(height: f32, z_near: f32, z_far: f32) -> Camera {
        Camera::new_orthographic(height, z_near, z_far)
    }

    /// The fields `check` names, in order.
    fn fields(problems: &[FieldProblem]) -> Vec<&'static str> {
        problems.iter().map(|problem| problem.field).collect()
    }

    /// `camera` has exactly the one problem `expected`: `problems` lists it
    /// alone, `check` reports it once as an `Error` on its field with its
    /// message, and `projection_matrix` refuses with it.
    fn assert_one_problem(camera: Camera, expected: CameraProblem) {
        assert_eq!(camera.problems(), vec![expected], "problems of {camera:?}");
        let check = camera.check();
        assert_eq!(check.len(), 1, "check of {camera:?}: {check:?}");
        assert_eq!(check[0].field, expected.field(), "field of {camera:?}");
        assert_eq!(
            check[0].severity,
            ProblemSeverity::Error,
            "severity of {camera:?}"
        );
        assert_eq!(
            check[0].message,
            expected.to_string(),
            "the message of {camera:?} is the problem's"
        );
        assert_eq!(
            camera.projection_matrix(WIDE),
            Err(expected),
            "projection of {camera:?}"
        );
    }

    /// `camera` is usable: no problem, nothing to check, a projection.
    fn assert_usable(camera: Camera) {
        assert!(
            camera.problems().is_empty(),
            "{camera:?}: {:?}",
            camera.problems()
        );
        assert!(
            camera.check().is_empty(),
            "{camera:?}: {:?}",
            camera.check()
        );
        assert!(
            camera.projection_matrix(WIDE).is_ok(),
            "{camera:?}: {:?}",
            camera.projection_matrix(WIDE)
        );
    }

    /// The default camera and ordinary perspectives — narrow, wide, with a
    /// tiny positive near plane (allowed, no warning) — have no problem.
    #[test]
    fn a_valid_perspective_camera_has_no_problem() {
        for camera in [
            Camera::default(),
            Camera::default_perspective(),
            perspective(PI / 4.0, 0.1, 100.0),
            perspective(0.1, 0.1, 100.0),
            perspective(PI * 0.9, 0.1, 100.0),
            perspective(PI / 3.0, 1e-4, 1000.0),
            perspective(PI / 3.0, 0.5, 0.6),
        ] {
            assert_usable(camera);
        }
    }

    /// The default orthographic camera (near −1) and orthographic cameras
    /// whose planes sit behind the eye have no problem: an orthographic near
    /// plane may be negative.
    #[test]
    fn a_valid_orthographic_camera_has_no_problem() {
        for camera in [
            Camera::default_orthographic(),
            orthographic(10.0, 0.1, 100.0),
            orthographic(1080.0, -1.0, 1000.0),
            orthographic(10.0, -50.0, -10.0),
            orthographic(1e-3, 0.0, 1.0),
        ] {
            assert_usable(camera);
        }
    }

    /// A perspective near plane at the eye is one problem, on `z_near`,
    /// whose message names the value.
    #[test]
    fn a_near_plane_at_zero_is_a_problem_on_z_near() {
        let camera = perspective(PI / 4.0, 0.0, 100.0);
        assert_one_problem(camera, CameraProblem::NearNotPositive { near: 0.0 });
        let check = camera.check();
        assert_eq!(check[0].field, "z_near");
        assert!(
            check[0].message.contains('0'),
            "the message names the value: {}",
            check[0].message
        );
    }

    /// A perspective near plane behind the eye — negative, or negative zero —
    /// is the same problem on `z_near`.
    #[test]
    fn a_near_plane_behind_the_eye_is_a_problem_on_z_near() {
        for near in [-0.0, -0.1, -10.0] {
            let camera = perspective(PI / 4.0, near, 100.0);
            assert_one_problem(camera, CameraProblem::NearNotPositive { near });
            assert_eq!(fields(&camera.check()), vec!["z_near"], "near {near}");
        }
        let message = perspective(PI / 4.0, -0.1, 100.0).check()[0]
            .message
            .clone();
        assert!(
            message.contains("-0.1"),
            "the message names the value: {message}"
        );
    }

    /// A far plane in front of the near one, or on it, is one problem on
    /// `z_far` — the field to fix — naming the far plane.
    #[test]
    fn a_near_plane_behind_the_far_is_a_problem_on_z_far() {
        let camera = perspective(PI / 4.0, 50.0, 10.0);
        assert_one_problem(
            camera,
            CameraProblem::FarNotBeyondNear {
                near: 50.0,
                far: 10.0,
            },
        );
        assert_eq!(fields(&camera.check()), vec!["z_far"]);
        let message = &camera.check()[0].message;
        assert!(
            message.contains("10"),
            "the message names the value: {message}"
        );

        let equal = perspective(PI / 4.0, 10.0, 10.0);
        assert_one_problem(
            equal,
            CameraProblem::FarNotBeyondNear {
                near: 10.0,
                far: 10.0,
            },
        );
    }

    /// A NaN near plane is not a number at all: it is reported once, as not
    /// finite — never also as "not positive", nor as "behind the far".
    #[test]
    fn a_nan_near_is_reported_once_as_not_finite() {
        let camera = perspective(PI / 4.0, f32::NAN, 100.0);
        assert_one_problem(camera, CameraProblem::NotFinite { field: "z_near" });
        assert_eq!(fields(&camera.check()), vec!["z_near"]);
    }

    /// An infinite (or NaN) far plane would poison the projection with NaN:
    /// it is one problem, not finite, on `z_far`.
    #[test]
    fn an_infinite_far_plane_is_a_problem() {
        for far in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            let camera = perspective(PI / 4.0, 0.1, far);
            assert_one_problem(camera, CameraProblem::NotFinite { field: "z_far" });
        }
    }

    /// A field of view closed to nothing, negative, or opened to a half-turn
    /// or more is one problem on the perspective's `fov_y_radians`.
    #[test]
    fn a_closed_or_reversed_field_of_view_is_a_problem() {
        for fov_y_radians in [0.0, -0.0, -0.1, PI, 4.0] {
            let camera = perspective(fov_y_radians, 0.1, 100.0);
            assert_one_problem(camera, CameraProblem::FieldOfView { fov_y_radians });
            assert_eq!(
                fields(&camera.check()),
                vec![FOV_FIELD],
                "fov {fov_y_radians}"
            );
        }
    }

    /// A NaN or infinite field of view is reported once, as not finite, on
    /// the perspective's `fov_y_radians`.
    #[test]
    fn a_non_finite_field_of_view_is_reported_once_as_not_finite() {
        for fov in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let camera = perspective(fov, 0.1, 100.0);
            assert_one_problem(camera, CameraProblem::NotFinite { field: FOV_FIELD });
        }
    }

    /// An orthographic camera with no height, a negative or a non-finite
    /// one, or with no depth between its planes, has a problem on that
    /// field.
    #[test]
    fn an_orthographic_camera_without_height_or_depth_is_a_problem() {
        for height in [0.0, -0.0, -1.0] {
            let camera = orthographic(height, 0.1, 100.0);
            assert_one_problem(camera, CameraProblem::Height { height });
            assert_eq!(
                fields(&camera.check()),
                vec![HEIGHT_FIELD],
                "height {height}"
            );
        }
        for height in [f32::NAN, f32::INFINITY] {
            assert_one_problem(
                orthographic(height, 0.1, 100.0),
                CameraProblem::NotFinite {
                    field: HEIGHT_FIELD,
                },
            );
        }
        for (near, far) in [(10.0, 10.0), (-1.0, -1.0), (100.0, 0.1)] {
            let camera = orthographic(10.0, near, far);
            assert_one_problem(camera, CameraProblem::FarNotBeyondNear { near, far });
            assert_eq!(fields(&camera.check()), vec!["z_far"], "{near}..{far}");
        }
        assert_one_problem(
            orthographic(10.0, f32::NAN, 100.0),
            CameraProblem::NotFinite { field: "z_near" },
        );
        assert_one_problem(
            orthographic(10.0, -1.0, f32::INFINITY),
            CameraProblem::NotFinite { field: "z_far" },
        );
    }

    /// Every problem is listed, in field order (projection, near, far), and
    /// the projection refuses with the first.
    #[test]
    fn every_problem_is_listed_in_field_order_and_the_first_is_refused() {
        let camera = perspective(0.0, 0.0, f32::NAN);
        let expected = vec![
            CameraProblem::FieldOfView { fov_y_radians: 0.0 },
            CameraProblem::NearNotPositive { near: 0.0 },
            CameraProblem::NotFinite { field: "z_far" },
        ];
        assert_eq!(camera.problems(), expected);
        assert_eq!(fields(&camera.check()), vec![FOV_FIELD, "z_near", "z_far"]);
        assert!(camera
            .check()
            .iter()
            .all(|problem| problem.severity == ProblemSeverity::Error));
        assert_eq!(camera.projection_matrix(WIDE), Err(expected[0]));

        let flat = orthographic(0.0, 5.0, 1.0);
        assert_eq!(
            flat.problems(),
            vec![
                CameraProblem::Height { height: 0.0 },
                CameraProblem::FarNotBeyondNear {
                    near: 5.0,
                    far: 1.0
                },
            ]
        );
    }

    /// A camera every field of which passes, but whose projection still
    /// overflows (a field of view so narrow its focal length is infinite),
    /// is one problem on `projection`.
    #[test]
    fn an_unprojectable_camera_is_a_problem_on_its_projection() {
        let fov = 1e-39_f32;
        assert!(
            Mat4::try_perspective_rh_zo(fov, 1.0, 0.1, 100.0).is_none(),
            "the case under test must overflow at aspect 1"
        );
        let camera = perspective(fov, 0.1, 100.0);
        assert_one_problem(camera, CameraProblem::Unprojectable);
        assert_eq!(fields(&camera.check()), vec!["projection"]);
    }

    /// A valid camera handed an aspect that makes no image — flat, negative,
    /// not a number — cannot be projected, and says so instead of
    /// panicking.
    #[test]
    fn a_bad_aspect_makes_a_valid_camera_unprojectable() {
        for camera in [
            Camera::default_perspective(),
            Camera::default_orthographic(),
        ] {
            for aspect in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert_eq!(
                    camera.projection_matrix(aspect),
                    Err(CameraProblem::Unprojectable),
                    "{camera:?} at aspect {aspect}"
                );
            }
        }
    }

    /// Over a grid of planes and fields of view — zero, negative zero,
    /// negatives, NaN, infinities, near == far, a closed, a half-turn and a
    /// subnormal field of view — for both projection kinds, a camera checks
    /// clean exactly when it can be projected.
    #[test]
    fn the_check_and_the_projection_agree() {
        let planes = [
            0.0,
            -0.0,
            -1.0,
            -50.0,
            1e-6,
            0.1,
            1.0,
            10.0,
            50.0,
            1000.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let fovs = [
            0.0,
            -0.0,
            -0.1,
            1e-39,
            1e-38,
            0.1,
            PI / 3.0,
            3.0,
            PI,
            4.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let heights = [
            0.0,
            -0.0,
            -1.0,
            1e-6,
            1.0,
            10.0,
            1080.0,
            f32::NAN,
            f32::INFINITY,
        ];
        let mut cameras = Vec::new();
        for near in planes {
            for far in planes {
                for fov in fovs {
                    cameras.push(perspective(fov, near, far));
                }
                for height in heights {
                    cameras.push(orthographic(height, near, far));
                }
            }
        }
        for camera in cameras {
            let clean = camera.check().is_empty();
            let projection = camera.projection_matrix(WIDE);
            assert_eq!(
                clean,
                projection.is_ok(),
                "{camera:?}: check {:?}, projection {projection:?}",
                camera.check()
            );
            assert_eq!(
                clean,
                camera.problems().is_empty(),
                "{camera:?}: check and problems disagree"
            );
        }
    }

    /// The bit patterns of every value a camera holds.
    fn bits(camera: &Camera) -> (u32, u32, u32, bool) {
        let projection = match camera.projection {
            ProjectionType::Perspective { fov_y_radians } => fov_y_radians.to_bits(),
            ProjectionType::Orthographic { height } => height.to_bits(),
        };
        (
            projection,
            camera.z_near.to_bits(),
            camera.z_far.to_bits(),
            camera.is_active,
        )
    }

    /// Checking or projecting a broken camera changes none of its values:
    /// the authored value stays exactly what was authored, NaN and negative
    /// zero included.
    #[test]
    fn a_broken_camera_keeps_its_values() {
        for camera in [
            perspective(PI / 4.0, 0.0, 100.0),
            perspective(PI / 4.0, -0.0, 100.0),
            perspective(PI / 4.0, f32::NAN, f32::INFINITY),
            perspective(0.0, 50.0, 10.0),
            orthographic(0.0, 10.0, 10.0),
            orthographic(f32::NAN, -1.0, f32::NEG_INFINITY),
        ] {
            let before = bits(&camera);
            let _ = camera.problems();
            let _ = camera.check();
            let _ = camera.projection_matrix(WIDE);
            assert_eq!(bits(&camera), before, "{camera:?}");
        }
    }

    /// Every field a problem names is a path to a value in the camera's
    /// serialized form — the JSON the inspector walks — so the inspector can
    /// mark the row it shows the value on.
    #[test]
    fn field_paths_match_the_serialized_form() {
        let registration =
            registration_for(TypeId::of::<Camera>()).expect("Camera registers itself");
        let broken = [
            perspective(0.0, 0.0, f32::NAN),
            perspective(PI / 4.0, 50.0, 10.0),
            perspective(f32::NAN, f32::NAN, f32::INFINITY),
            perspective(1e-39, 0.1, 100.0),
            orthographic(0.0, 10.0, 10.0),
            orthographic(f32::NAN, f32::NAN, f32::INFINITY),
        ];
        let mut seen = Vec::new();
        for camera in broken {
            let mut world = World::new();
            let entity = world.spawn(camera);
            let json = (registration.to_json)(&world, entity).expect("a camera serializes");
            let check = camera.check();
            assert!(!check.is_empty(), "{camera:?} is broken");
            for problem in check {
                let mut value = &json;
                for key in problem.field.split('.') {
                    value = value.get(key).unwrap_or_else(|| {
                        panic!(
                            "`{}` of {camera:?} does not resolve in its serialized form {json}",
                            problem.field
                        )
                    });
                }
                seen.push(problem.field);
            }
        }
        for field in [FOV_FIELD, HEIGHT_FIELD, "z_near", "z_far", "projection"] {
            assert!(seen.contains(&field), "no problem on `{field}` was checked");
        }
    }
}
