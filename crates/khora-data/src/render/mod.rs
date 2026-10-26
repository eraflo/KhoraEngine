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

//! Render-data layer — the per-frame extracted scene representation.
//!
//! Per CLAD this module is a **data** responsibility. The per-frame
//! projection of the scene is now owned by
//! [`RenderFlow`](crate::flow::RenderFlow), which publishes a
//! [`RenderWorld`] into the
//! [`LaneBus`](khora_core::lane::LaneBus) during the Substrate Pass.
//!
//! Render lanes consume that view from the bus; they no longer query the
//! ECS World directly.

mod editor_view;
mod frame_graph;
mod gizmo;
mod grid;
mod shining;
mod wireframe;
mod world;

pub use editor_view::EditorViewportOverride;
pub use frame_graph::{
    pass_order, registered_pass_order, submit_frame_graph, FrameGraph, OverlayPassSlot,
    PassContribution, PassDescriptor, PassOrderError, PassRegistration, ResourceId, ScenePassSlot,
    ShadowPassSlot, SharedFrameGraph, SkyboxPassSlot, TransparentEncoder, TransparentPassSlot,
    UiPassSlot,
};
pub use gizmo::{GizmoFrame, GizmoView};
pub use grid::{GridConfig, GridView};
pub use shining::{shining_lights, ShiningLight};
pub use wireframe::{WireframeConfig, WireframeView};
pub use world::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

use khora_core::{
    lane::ColorTargetSize,
    math::Mat4,
    renderer::api::{frame::FrameContext, resource::ViewInfo},
    Runtime,
};

use crate::ecs::{Camera, CameraProblem, GlobalTransform, World};

/// Shadow result for a single light: view-projection matrix + atlas layer index.
pub type ShadowResult = (khora_core::math::Mat4, i32);

/// The aspect a camera is projected at when no colour target is known (a
/// GPU-free run has no image to take a shape from).
pub const FALLBACK_ASPECT: f32 = 1.0;

/// The aspect of the image this frame renders into: the
/// [`ColorTargetSize`] the engine publishes in the [`FrameContext`] once the
/// frame's target is acquired, or [`FALLBACK_ASPECT`] when there is none.
///
/// Every camera projection takes its aspect from here, so a camera fills the
/// window — or the editor's viewport — without being stretched, whatever its
/// shape and however often it is resized.
pub fn target_aspect(runtime: &Runtime) -> f32 {
    runtime
        .resources
        .get::<std::sync::Arc<FrameContext>>()
        .and_then(|fctx| fctx.get::<ColorTargetSize>())
        .map_or(FALLBACK_ASPECT, |size| size.aspect())
}

/// The view matrix (world → view) of a camera placed at `global`: the inverse
/// of its rigid pose (rotation and translation). The entity's scale is
/// ignored — a camera's zoom is its projection's job, never its transform's.
///
/// Every view of a scene camera is built here — [`extract_active_camera_view`]
/// and [`primary_view`] alike — so what the renderer is handed and what the
/// flows slice their data around are the same view.
pub fn camera_view_matrix(global: &GlobalTransform) -> Mat4 {
    Mat4::from_quat(global.0.rotation().inverse()) * Mat4::from_translation(-global.0.translation())
}

/// Extracts the first **active** camera as a [`ViewInfo`] suitable for
/// pushing to the renderer, projected for an image whose width over height
/// is `aspect` (usually [`target_aspect`]). Its view is the
/// [`camera_view_matrix`] of the camera's entity: scale is ignored.
///
/// The first active camera is chosen **before** it is checked:
/// - `None` — no entity has an active camera (editor edit mode, empty
///   scene, etc.); callers can fall back to a default `ViewInfo`.
/// - `Some(Err(problem))` — the active camera's values make no image (see
///   [`Camera::problems`](crate::ecs::Camera::problems)); it renders
///   nothing, and no other camera is picked in its place.
/// - `Some(Ok(view))` — the active camera's view.
///
/// Lives outside the [`RenderFlow`](crate::flow::RenderFlow) because it is
/// also used by the editor for non-render concerns (camera prepare, gizmo
/// space, etc.).
pub fn extract_active_camera_view(
    world: &World,
    aspect: f32,
) -> Option<Result<ViewInfo, CameraProblem>> {
    world
        .query::<(&Camera, &GlobalTransform)>()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, global_transform)| {
            let projection = camera.projection_matrix(aspect)?;
            Ok(ViewInfo::new(
                camera_view_matrix(global_transform),
                projection,
                global_transform.0.translation(),
            ))
        })
}

/// Returns the primary [`ExtractedView`] for the frame: the first active
/// scene `Camera`, projected at the [`target_aspect`], or — if none is
/// active — the editor's [`EditorViewportOverride`]. An active camera whose
/// values make no image gives `None`: it renders nothing, and the override
/// does not stand in for it.
///
/// Shared by [`RenderFlow`](crate::flow::RenderFlow) and
/// [`ShadowFlow`](crate::flow::ShadowFlow) so both flows agree on which
/// view their data is built around (CSM frustum slicing in particular
/// needs the same camera the lit pass will sample shadows from).
pub fn primary_view(world: &World, runtime: &Runtime) -> Option<ExtractedView> {
    match extract_active_camera_view(world, target_aspect(runtime)) {
        Some(Ok(view)) => {
            return Some(ExtractedView {
                view_proj: view.view_projection_matrix(),
                position: view.camera_position,
            })
        }
        Some(Err(_)) => return None,
        None => {}
    }
    runtime
        .resources
        .get::<EditorViewportOverride>()
        .and_then(|o| o.get())
}

/// Bit-level fingerprint of the [`EditorViewportOverride`] as seen by
/// [`primary_view`]: distinguishes "no override" from "override present"
/// and any change in the override's value (editor camera moves do not
/// mutate the ECS, so flows that consult `primary_view` must fold this
/// into their cache key or they would serve stale views while the editor
/// camera flies).
pub(crate) fn editor_override_fingerprint(runtime: &Runtime) -> u64 {
    use std::hash::{Hash, Hasher};

    let Some(view) = runtime
        .resources
        .get::<EditorViewportOverride>()
        .and_then(|o| o.get())
    else {
        return 0;
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Presence marker, so an override that happens to hash to 0 still
    // differs from "no override".
    1u8.hash(&mut hasher);
    for col in &view.view_proj.cols {
        col.x.to_bits().hash(&mut hasher);
        col.y.to_bits().hash(&mut hasher);
        col.z.to_bits().hash(&mut hasher);
        col.w.to_bits().hash(&mut hasher);
    }
    view.position.x.to_bits().hash(&mut hasher);
    view.position.y.to_bits().hash(&mut hasher);
    view.position.z.to_bits().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::Camera;
    use khora_core::math::Vec3;
    use std::f32::consts::FRAC_PI_4;

    /// A world holding one active perspective camera at `position`, and that
    /// camera.
    fn world_with_camera(position: Vec3) -> (World, Camera) {
        let camera = Camera::new_perspective(FRAC_PI_4, 0.1, 100.0);
        let mut world = World::new();
        world.spawn((GlobalTransform::at_position(position), camera));
        (world, camera)
    }

    fn assert_mat_close(actual: Mat4, expected: Mat4, what: &str) {
        for (a, e) in actual.cols.iter().zip(expected.cols.iter()) {
            for (x, y) in [(a.x, e.x), (a.y, e.y), (a.z, e.z), (a.w, e.w)] {
                assert!(
                    (x - y).abs() < 1e-5,
                    "{what}: {actual:?} where {expected:?} was expected"
                );
            }
        }
    }

    /// With no target known, the aspect is square.
    #[test]
    fn the_fallback_aspect_is_one() {
        assert_eq!(FALLBACK_ASPECT, 1.0);
    }

    /// A runtime that holds no `FrameContext` (a GPU-free run) knows no
    /// target, so its aspect is the fallback.
    #[test]
    fn a_runtime_without_a_frame_context_has_the_fallback_aspect() {
        assert_eq!(target_aspect(&Runtime::default()), FALLBACK_ASPECT);
    }

    /// `extract_active_camera_view` builds the camera's projection with the
    /// aspect it is handed — the target's — not one stored on the camera.
    #[test]
    fn the_active_camera_view_is_projected_with_the_given_aspect() {
        let (world, camera) = world_with_camera(Vec3::new(1.0, 2.0, 3.0));
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 0.75] {
            let view = extract_active_camera_view(&world, aspect)
                .expect("an active camera")
                .expect("a valid camera");
            assert_eq!(
                view.projection_matrix,
                camera.projection_matrix(aspect).expect("a valid camera"),
                "aspect {aspect}"
            );
            assert_eq!(view.camera_position, Vec3::new(1.0, 2.0, 3.0));
        }
    }

    /// Two aspects give two projections of the same camera.
    #[test]
    fn the_active_camera_view_changes_with_the_aspect() {
        let (world, _) = world_with_camera(Vec3::ZERO);
        let wide = extract_active_camera_view(&world, 16.0 / 9.0)
            .expect("an active camera")
            .expect("a valid camera");
        let square = extract_active_camera_view(&world, 1.0)
            .expect("an active camera")
            .expect("a valid camera");
        assert_ne!(wide.projection_matrix, square.projection_matrix);
    }

    /// `primary_view` with no `FrameContext` in the runtime projects the
    /// active camera at [`FALLBACK_ASPECT`].
    #[test]
    fn the_primary_view_without_a_frame_context_uses_the_fallback_aspect() {
        let position = Vec3::new(0.0, 1.0, 5.0);
        let (world, camera) = world_with_camera(position);
        let view = primary_view(&world, &Runtime::default()).expect("an active camera");
        let expected = camera
            .projection_matrix(FALLBACK_ASPECT)
            .expect("a valid camera")
            * Mat4::from_translation(-position);
        assert_mat_close(view.view_proj, expected, "view_proj");
        assert_eq!(view.position, position);
    }

    // --- A camera's view ignores its entity's scale -------------------------

    use khora_core::math::Quaternion;

    /// The translation every posed camera below sits at.
    const POSE_TRANSLATION: Vec3 = Vec3::new(1.5, -2.0, 4.0);

    /// The rotation every posed camera below is turned by.
    fn pose_rotation() -> Quaternion {
        Quaternion::from_axis_angle(Vec3::new(1.0, 2.0, 0.5).normalize(), 0.7)
    }

    /// A `GlobalTransform` built as translation · rotation · scale.
    fn posed(translation: Vec3, rotation: Quaternion, scale: Vec3) -> GlobalTransform {
        GlobalTransform::new(
            Mat4::from_translation(translation)
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(scale),
        )
    }

    /// A world holding one active perspective camera at `global`, and that
    /// camera.
    fn world_with_camera_at(global: GlobalTransform) -> (World, Camera) {
        let camera = Camera::new_perspective(FRAC_PI_4, 0.1, 100.0);
        let mut world = World::new();
        world.spawn((global, camera));
        (world, camera)
    }

    fn assert_vec_close(actual: Vec3, expected: Vec3, what: &str) {
        for (x, y) in [
            (actual.x, expected.x),
            (actual.y, expected.y),
            (actual.z, expected.z),
        ] {
            assert!(
                (x - y).abs() < 1e-5,
                "{what}: {actual:?} where {expected:?} was expected"
            );
        }
    }

    /// The scales the scaled-camera tests run over: uniform, non-uniform.
    const SCALES: [Vec3; 2] = [Vec3::new(2.0, 2.0, 2.0), Vec3::new(0.5, 3.0, 1.5)];

    /// The view the renderer is handed (`extract_active_camera_view`) and the
    /// view the lit pass and the shadow cascades are built around
    /// (`primary_view`) agree for a camera on a scaled entity: same
    /// view-projection at the same aspect, same position.
    #[test]
    fn a_scaled_camera_gives_the_same_view_proj_on_both_paths() {
        for scale in SCALES {
            let (world, _) = world_with_camera_at(posed(POSE_TRANSLATION, pose_rotation(), scale));
            let extracted = extract_active_camera_view(&world, FALLBACK_ASPECT)
                .expect("an active camera")
                .expect("a valid camera");
            let primary = primary_view(&world, &Runtime::default()).expect("an active camera");
            assert_mat_close(
                extracted.projection_matrix * extracted.view_matrix,
                primary.view_proj,
                &format!("view_proj at scale {scale:?}"),
            );
            assert_vec_close(
                extracted.camera_position,
                primary.position,
                &format!("position at scale {scale:?}"),
            );
            assert_vec_close(
                primary.position,
                POSE_TRANSLATION,
                &format!("position is the translation at scale {scale:?}"),
            );
        }
    }

    /// Scaling a camera's entity does not change what it sees: its view is
    /// the view of the same pose without the scale.
    #[test]
    fn a_scaled_camera_sees_what_the_unscaled_pose_sees() {
        let (unscaled_world, _) =
            world_with_camera_at(posed(POSE_TRANSLATION, pose_rotation(), Vec3::ONE));
        let unscaled = extract_active_camera_view(&unscaled_world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("a valid camera");
        for scale in SCALES {
            let (world, _) = world_with_camera_at(posed(POSE_TRANSLATION, pose_rotation(), scale));
            let scaled = extract_active_camera_view(&world, FALLBACK_ASPECT)
                .expect("an active camera")
                .expect("a valid camera");
            assert_mat_close(
                scaled.view_matrix,
                unscaled.view_matrix,
                &format!("view at scale {scale:?}"),
            );
        }
    }

    /// An unscaled camera (rotation + translation) keeps the view it had:
    /// the inverse of its world matrix, on both paths.
    #[test]
    fn an_unscaled_camera_keeps_the_inverse_of_its_pose_as_its_view() {
        let global = posed(POSE_TRANSLATION, pose_rotation(), Vec3::ONE);
        let inverse = global.to_matrix().inverse().expect("an invertible pose");
        let (world, camera) = world_with_camera_at(global);

        let extracted = extract_active_camera_view(&world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("a valid camera");
        assert_mat_close(extracted.view_matrix, inverse, "extracted view");
        assert_vec_close(
            extracted.camera_position,
            POSE_TRANSLATION,
            "extracted position",
        );

        let primary = primary_view(&world, &Runtime::default()).expect("an active camera");
        assert_mat_close(
            primary.view_proj,
            camera
                .projection_matrix(FALLBACK_ASPECT)
                .expect("a valid camera")
                * inverse,
            "primary view_proj",
        );
        assert_vec_close(primary.position, POSE_TRANSLATION, "primary position");
    }

    /// `camera_view_matrix` of a scaled transform takes the camera's position
    /// to the view-space origin and is rigid: determinant one, and a unit
    /// direction stays a unit direction.
    #[test]
    fn the_view_of_a_scaled_camera_is_rigid_and_centred_on_it() {
        let unit_directions = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(1.0, -1.0, 2.0).normalize(),
        ];
        for scale in SCALES {
            let view = camera_view_matrix(&posed(POSE_TRANSLATION, pose_rotation(), scale));
            assert_vec_close(
                view.transform_point(POSE_TRANSLATION),
                Vec3::ZERO,
                &format!("camera position in view space at scale {scale:?}"),
            );
            assert!(
                (view.determinant() - 1.0).abs() < 1e-5,
                "determinant {} at scale {scale:?}",
                view.determinant()
            );
            for direction in unit_directions {
                let length = view.transform_vector(direction).length();
                assert!(
                    (length - 1.0).abs() < 1e-5,
                    "{direction:?} has length {length} in view space at scale {scale:?}"
                );
            }
        }
    }

    /// A mirrored camera (one negative scale axis) still gives the same
    /// view-projection and position on both paths.
    #[test]
    fn a_mirrored_camera_gives_the_same_view_proj_on_both_paths() {
        let (world, _) = world_with_camera_at(posed(
            POSE_TRANSLATION,
            pose_rotation(),
            Vec3::new(-1.0, 1.0, 1.0),
        ));
        let extracted = extract_active_camera_view(&world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("a valid camera");
        let primary = primary_view(&world, &Runtime::default()).expect("an active camera");
        assert_mat_close(
            extracted.projection_matrix * extracted.view_matrix,
            primary.view_proj,
            "mirrored view_proj",
        );
        assert_vec_close(
            extracted.camera_position,
            primary.position,
            "mirrored position",
        );
    }

    /// `camera_view_matrix` is the view `extract_active_camera_view` hands the
    /// renderer, so the two cannot drift apart.
    #[test]
    fn the_extracted_view_is_the_camera_view_matrix() {
        for scale in SCALES {
            let global = posed(POSE_TRANSLATION, pose_rotation(), scale);
            let (world, _) = world_with_camera_at(global);
            let extracted = extract_active_camera_view(&world, FALLBACK_ASPECT)
                .expect("an active camera")
                .expect("a valid camera");
            assert_mat_close(
                extracted.view_matrix,
                camera_view_matrix(&global),
                &format!("view at scale {scale:?}"),
            );
        }
    }

    /// A camera under a tiny uniform scale (a millimetre asset nested in a
    /// centimetre one) keeps its rotation: its view is that of the pose
    /// without the scale.
    #[test]
    fn a_camera_under_a_tiny_uniform_scale_keeps_its_rotation() {
        let unscaled = camera_view_matrix(&posed(POSE_TRANSLATION, pose_rotation(), Vec3::ONE));
        let tiny = camera_view_matrix(&posed(
            POSE_TRANSLATION,
            pose_rotation(),
            Vec3::new(1e-6, 1e-6, 1e-6),
        ));
        assert_mat_close(tiny, unscaled, "view under a 1e-6 uniform scale");
    }

    /// A camera whose entity is flattened along one axis (scale 0 on Z) has
    /// a finite view, and keeps the rotation its two other axes still carry.
    #[test]
    fn a_camera_flattened_on_one_axis_keeps_its_rotation() {
        let unscaled = camera_view_matrix(&posed(POSE_TRANSLATION, pose_rotation(), Vec3::ONE));
        let flat = camera_view_matrix(&posed(
            POSE_TRANSLATION,
            pose_rotation(),
            Vec3::new(1.0, 1.0, 0.0),
        ));
        for col in flat.cols {
            assert!(
                col.x.is_finite() && col.y.is_finite() && col.z.is_finite() && col.w.is_finite(),
                "a flattened camera has a non-finite view: {flat:?}"
            );
        }
        assert_mat_close(flat, unscaled, "view of a camera flattened on Z");
    }

    /// A posed camera under a parent stretched to twice its height has a
    /// sheared world basis. It looks exactly where its world forward axis
    /// points: every point straight ahead of it along that axis (the camera
    /// looks down −Z) lands on the view's −Z axis, at its distance.
    #[test]
    fn a_camera_under_a_stretched_parent_looks_down_its_world_forward_axis() {
        let parent = Mat4::from_scale(Vec3::new(1.0, 2.0, 1.0));
        let child = posed(POSE_TRANSLATION, pose_rotation(), Vec3::ONE);
        let global = GlobalTransform::new(parent * child.to_matrix());
        let position = global.0.translation();
        let world_forward = global.0.forward().normalize();
        assert!(
            global.0.up().dot(global.0.forward()).abs() > 0.05,
            "the camera's world basis under test must be sheared"
        );

        let view = camera_view_matrix(&global);

        for distance in [1.0f32, 5.0, 40.0] {
            let ahead = position - world_forward * distance;
            let seen = view.transform_point(ahead);
            assert!(
                seen.x.abs() < 1e-4 * distance && seen.y.abs() < 1e-4 * distance,
                "a point {distance} ahead on the camera's world forward axis lands at \
                 {seen:?}, off the view's -Z axis"
            );
            assert!(
                (seen.z + distance).abs() < 1e-4 * distance,
                "a point {distance} ahead lands at depth {} instead of {}",
                seen.z,
                -distance
            );
        }
    }

    // --- Which camera both paths pick ---------------------------------------

    /// Past an inactive camera, both paths pick the same, first active camera
    /// even when a second active one follows it.
    #[test]
    fn both_paths_pick_the_first_active_camera_past_an_inactive_one() {
        let first_active = Vec3::new(0.0, 0.0, 5.0);
        let mut world = World::new();
        let mut inactive = Camera::new_perspective(FRAC_PI_4, 0.1, 100.0);
        inactive.is_active = false;
        world.spawn((
            GlobalTransform::at_position(Vec3::new(9.0, 9.0, 9.0)),
            inactive,
        ));
        world.spawn((
            GlobalTransform::at_position(first_active),
            Camera::new_perspective(FRAC_PI_4, 0.1, 100.0),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(-7.0, 0.0, 0.0)),
            Camera::new_perspective(FRAC_PI_4, 0.1, 100.0),
        ));

        let extracted = extract_active_camera_view(&world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("a valid camera");
        let primary = primary_view(&world, &Runtime::default()).expect("an active camera");
        assert_eq!(extracted.camera_position, first_active);
        assert_eq!(primary.position, first_active);
        assert_mat_close(
            extracted.projection_matrix * extracted.view_matrix,
            primary.view_proj,
            "view_proj of the first active camera",
        );
    }

    /// With no active camera, the renderer is handed no scene view, and the
    /// flows fall back to the editor's override view, unchanged.
    #[test]
    fn with_no_active_camera_the_primary_view_is_the_editor_override() {
        let mut world = World::new();
        let mut inactive = Camera::new_perspective(FRAC_PI_4, 0.1, 100.0);
        inactive.is_active = false;
        world.spawn((
            GlobalTransform::at_position(Vec3::new(1.0, 2.0, 3.0)),
            inactive,
        ));

        let override_view = ExtractedView {
            view_proj: Mat4::from_translation(Vec3::new(4.0, 5.0, 6.0)),
            position: Vec3::new(-4.0, -5.0, -6.0),
        };
        let editor = EditorViewportOverride::new();
        editor.set(Some(override_view.clone()));
        let mut runtime = Runtime::default();
        runtime.resources.insert(editor);

        assert!(extract_active_camera_view(&world, FALLBACK_ASPECT).is_none());
        let primary = primary_view(&world, &runtime).expect("the editor override");
        assert_eq!(primary.view_proj, override_view.view_proj);
        assert_eq!(primary.position, override_view.position);
    }

    // --- A broken active camera ---------------------------------------------

    /// A camera whose near plane sits at its eye: its values make no image.
    fn broken_camera() -> Camera {
        Camera::new_perspective(FRAC_PI_4, 0.0, 100.0)
    }

    /// The first active camera is chosen before it is checked: when it is
    /// broken, it is reported as broken, and the valid active camera after
    /// it is not picked in its place.
    #[test]
    fn a_broken_active_camera_is_reported_not_skipped() {
        let mut world = World::new();
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 0.0, 5.0)),
            broken_camera(),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(-7.0, 0.0, 0.0)),
            Camera::new_perspective(FRAC_PI_4, 0.1, 100.0),
        ));

        match extract_active_camera_view(&world, FALLBACK_ASPECT) {
            Some(Err(problem)) => {
                assert_eq!(problem, CameraProblem::NearNotPositive { near: 0.0 })
            }
            Some(Ok(view)) => panic!(
                "a broken first active camera was skipped for the camera at {:?}",
                view.camera_position
            ),
            None => panic!("a broken active camera is still an active camera"),
        }
    }

    /// A broken camera that is not active is not the one chosen: the active
    /// camera after it gives the view.
    #[test]
    fn a_broken_inactive_camera_is_passed_over() {
        let active_at = Vec3::new(0.0, 2.0, 5.0);
        let mut world = World::new();
        let mut inactive = broken_camera();
        inactive.is_active = false;
        world.spawn((GlobalTransform::at_position(Vec3::ZERO), inactive));
        world.spawn((
            GlobalTransform::at_position(active_at),
            Camera::new_perspective(FRAC_PI_4, 0.1, 100.0),
        ));

        let view = extract_active_camera_view(&world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("the active camera is valid");
        assert_eq!(view.camera_position, active_at);
    }

    /// No active camera — an empty world, or only inactive cameras, broken
    /// or not — is `None`, as before.
    #[test]
    fn no_active_camera_is_none() {
        assert!(extract_active_camera_view(&World::new(), FALLBACK_ASPECT).is_none());

        let mut world = World::new();
        let mut inactive_valid = Camera::new_perspective(FRAC_PI_4, 0.1, 100.0);
        inactive_valid.is_active = false;
        let mut inactive_broken = broken_camera();
        inactive_broken.is_active = false;
        world.spawn((GlobalTransform::at_position(Vec3::ZERO), inactive_valid));
        world.spawn((GlobalTransform::at_position(Vec3::ONE), inactive_broken));
        assert!(extract_active_camera_view(&world, FALLBACK_ASPECT).is_none());
    }

    /// A broken active camera renders nothing: `primary_view` is `None`, and
    /// the editor's override — the fallback for *no* active camera — is not
    /// used in its place.
    #[test]
    fn primary_view_of_a_broken_camera_ignores_the_editor_override() {
        let mut world = World::new();
        world.spawn((GlobalTransform::at_position(Vec3::ZERO), broken_camera()));

        let editor = EditorViewportOverride::new();
        editor.set(Some(ExtractedView {
            view_proj: Mat4::from_translation(Vec3::new(4.0, 5.0, 6.0)),
            position: Vec3::new(-4.0, -5.0, -6.0),
        }));
        let mut runtime = Runtime::default();
        runtime.resources.insert(editor);

        let primary = primary_view(&world, &runtime);
        assert!(
            primary.is_none(),
            "a broken active camera fell back to a view: {primary:?}"
        );
    }
}
