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

//! The ECS side of the gizmos: reads the selection out of the world, hands the
//! geometry and drag math what they need, and writes the resulting transforms
//! back.

use super::interact::{gizmo_basis, GizmoBasis, GizmoDelta, GizmoTransform};
use super::selection::{
    generate_selection_gizmos, gizmo_world_size, manipulator, GizmoKind, SelectionGizmo,
};
use khora_sdk::editor_ui::{EditorState, GizmoLineInstance};
use khora_sdk::khora_core::math::{Mat4, Vec3};
use khora_sdk::khora_core::renderer::api::gpu_scene::mesh::Mesh;
use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::renderer::light::LightType;
use khora_sdk::khora_data::render::camera_view_matrix;
use khora_sdk::prelude::ecs::{AudioSource, Camera, EntityId, GlobalTransform, Light, Transform};
use khora_sdk::GameWorld;
use khora_sdk::HandleComponent;

/// Everything a manipulator needs to know about the current selection.
pub struct SelectionFrame {
    /// World-space centre the handles are drawn and dragged around.
    pub pivot: Vec3,
    /// World size of the manipulator at that pivot.
    pub size: f32,
    /// Handle directions for the active tool.
    pub basis: GizmoBasis,
}

/// The frame the manipulator occupies for the current selection, or `None` when
/// nothing is selected.
///
/// The pivot is the mean of the selected origins, so a multi-selection rotates
/// and scales as one body rather than each entity about itself.
pub fn selection_frame(
    world: &GameWorld,
    editor_state: &EditorState,
    view_info: &ViewInfo,
) -> Option<SelectionFrame> {
    let mut sum = Vec3::ZERO;
    let mut count = 0.0_f32;
    let mut rotation = None;

    for &entity in &editor_state.selection {
        let Some(transform) = world.get_component::<Transform>(entity) else {
            continue;
        };
        sum = sum + world_origin(world, entity);
        count += 1.0;
        // The scale handles align to the object, which only means anything for
        // a single selection; with several, the first one's frame stands in.
        rotation.get_or_insert(transform.rotation);
    }

    (count > 0.0).then(|| {
        let pivot = sum / count;
        SelectionFrame {
            pivot,
            size: gizmo_world_size(view_info, pivot),
            basis: gizmo_basis(editor_state.gizmo_mode, rotation.unwrap_or_default()),
        }
    })
}

/// Collects gizmo line instances for all selected entities.
///
/// `viewport_aspect` is the width / height of the image a selected camera
/// would fill; its frustum is drawn at that aspect.
pub fn collect_gizmo_lines(
    world: &GameWorld,
    editor_state: &EditorState,
    view_info: &ViewInfo,
    viewport_aspect: f32,
) -> Vec<GizmoLineInstance> {
    let frame = selection_frame(world, editor_state, view_info);
    let mut entries: Vec<SelectionGizmo> = Vec::new();

    for &entity_id in &editor_state.selection {
        let Some(transform) = world.get_component::<Transform>(entity_id) else {
            continue;
        };
        // Drawn where the entity actually renders: a child sits at its
        // propagated global transform, not at its parent-relative one.
        let world_matrix = world
            .get_component::<GlobalTransform>(entity_id)
            .map(|global| global.0 .0)
            .unwrap_or_else(|| {
                Mat4::from_translation(transform.translation)
                    * Mat4::from_quat(transform.rotation)
                    * Mat4::from_scale(transform.scale)
            });

        // A camera's frustum is what it renders: its own projection at the
        // viewport's aspect, placed by the pose its view is built from —
        // position and rotation, never the entity's scale.
        let mut gizmo_transform = world_matrix;
        let kind = if let Some(camera) = world.get_component::<Camera>(entity_id) {
            gizmo_transform = world
                .get_component::<GlobalTransform>(entity_id)
                .and_then(|global| camera_view_matrix(global).inverse())
                .unwrap_or_else(|| {
                    Mat4::from_translation(transform.translation)
                        * Mat4::from_quat(transform.rotation)
                });
            match frustum_projection(camera, viewport_aspect) {
                Some(projection) => GizmoKind::Camera { projection },
                // Clip planes that make no image (mid-edit in the inspector):
                // the camera is marked, with no frustum to draw.
                None => GizmoKind::Empty,
            }
        } else if let Some(light) = world.get_component::<Light>(entity_id) {
            match &light.light_type {
                LightType::Directional(_) => GizmoKind::DirectionalLight,
                LightType::Point(p) => range_gizmo(p.range),
                LightType::Spot(s) => range_gizmo(s.range),
            }
        } else if world.get_component::<AudioSource>(entity_id).is_some() {
            GizmoKind::Audio
        } else if world
            .get_component::<HandleComponent<Mesh>>(entity_id)
            .is_some()
        {
            GizmoKind::Mesh
        } else {
            GizmoKind::Empty
        };

        entries.push(SelectionGizmo {
            transform: gizmo_transform,
            kind,
            size: gizmo_world_size(view_info, world_matrix.cols[3].truncate()),
        });
    }

    let mut lines = generate_selection_gizmos(&entries, editor_state.gizmo_mode);

    // One manipulator, at the selection's shared pivot — the same pivot a drag
    // resolves against, so the handles are where the gesture actually happens.
    if let Some(frame) = frame {
        lines.extend(manipulator(
            frame.pivot,
            &frame.basis,
            editor_state.gizmo_mode,
            frame.size,
        ));
    }

    lines
}

/// The transforms the selected entities have right now, to be held for the
/// duration of a drag.
///
/// A drag resolves against these rather than against the live values, so the
/// entity always ends up where the cursor says it should be instead of
/// integrating a chain of per-frame deltas.
pub fn capture_starts(
    world: &GameWorld,
    editor_state: &EditorState,
) -> Vec<(EntityId, GizmoTransform)> {
    editor_state
        .selection
        .iter()
        .filter_map(|&entity| {
            let transform = world.get_component::<Transform>(entity)?;
            Some((
                entity,
                GizmoTransform {
                    translation: transform.translation,
                    rotation: transform.rotation,
                    scale: transform.scale,
                },
            ))
        })
        .collect()
}

/// Writes a drag's result back onto every entity it started with.
///
/// The delta is world-space and `Transform` is parent-relative, so a *child* of
/// a moved or rotated parent is manipulated in its parent's frame rather than
/// the world's. Correct for the root entities that make up almost every
/// selection; reparenting-aware manipulation is a separate piece of work.
pub fn apply_delta(
    world: &mut GameWorld,
    delta: GizmoDelta,
    starts: &[(EntityId, GizmoTransform)],
) {
    for &(entity, start) in starts {
        let result = delta.apply(start);
        let Some(mut transform) = world.get_component_mut::<Transform>(entity) else {
            continue;
        };
        transform.translation = result.translation;
        transform.rotation = result.rotation;
        transform.scale = result.scale;

        // Says so, rather than leaving the physics sync to infer it from a
        // difference. A body the simulation owns is otherwise put straight back
        // where it was, and a drag under a centimetre used to vanish entirely.
        let _ = world
            .inner_world_mut()
            .add_component(entity, khora_sdk::khora_data::ecs::Teleported);
    }
}

/// The projection `camera` renders with at `aspect`, or `None` when its
/// values make no image (a selected camera's values are whatever the user is
/// typing; the camera's own check says what is wrong).
fn frustum_projection(camera: &Camera, aspect: f32) -> Option<Mat4> {
    camera.projection_matrix(aspect).ok()
}

/// The gizmo of a light that reaches `range`: its range sphere, or — when the
/// range is not a finite positive distance (mid-edit in the inspector, or a
/// broken scene) — the mark of a selection with nothing to draw.
fn range_gizmo(range: f32) -> GizmoKind {
    if range.is_finite() && range > 0.0 {
        GizmoKind::PointLight { radius: range }
    } else {
        GizmoKind::Empty
    }
}

/// World-space origin of `entity`, preferring the propagated global transform.
fn world_origin(world: &GameWorld, entity: EntityId) -> Vec3 {
    if let Some(global) = world.get_component::<GlobalTransform>(entity) {
        return global.0 .0.cols[3].truncate();
    }
    world
        .get_component::<Transform>(entity)
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gizmo::interact::{GizmoAxis, GizmoDrag};
    use khora_sdk::editor_ui::GizmoMode;
    use khora_sdk::khora_core::math::{Quaternion, Ray};

    /// A Move drag grabbed on the selection's pivot writes the dragged offset
    /// into every selected entity's `Transform`, flags each one `Teleported`,
    /// and resolves against the captured starts, so a second update does not
    /// add to the first.
    #[test]
    fn move_drag_moves_the_selected_entities_in_the_world() {
        let mut world = GameWorld::new();
        let a = world.spawn(Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quaternion::IDENTITY,
            Vec3::ONE,
        ));
        let b = world.spawn(Transform::new(
            Vec3::new(-4.0, 0.5, 7.0),
            Quaternion::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0),
        ));

        let mut state = EditorState {
            gizmo_mode: GizmoMode::Move,
            ..Default::default()
        };
        state.selection.insert(a);
        state.selection.insert(b);

        let starts = capture_starts(&world, &state);
        assert_eq!(starts.len(), 2);

        let basis = gizmo_basis(GizmoMode::Move, Quaternion::IDENTITY);
        let cursor = |y: f32| Ray::new(Vec3::new(1.0, y, 10.0), Vec3::new(0.0, 0.0, -1.0));
        let mut drag = GizmoDrag::begin(
            GizmoMode::Move,
            GizmoAxis::Y,
            Vec3::new(1.0, 2.0, 3.0),
            &basis,
            0.25,
            &cursor(2.5),
        )
        .expect("grab resolves");

        let first = drag.update(&cursor(3.5)).expect("still perpendicular");
        apply_delta(&mut world, first, &starts);
        let second = drag.update(&cursor(4.0)).expect("still perpendicular");
        apply_delta(&mut world, second, &starts);

        let ta = world
            .get_component::<Transform>(a)
            .expect("a keeps its Transform");
        let tb = world
            .get_component::<Transform>(b)
            .expect("b keeps its Transform");
        assert!(
            (ta.translation - Vec3::new(1.0, 3.5, 3.0)).length() < 1e-4,
            "a at {:?}",
            ta.translation
        );
        assert!(
            (tb.translation - Vec3::new(-4.0, 2.0, 7.0)).length() < 1e-4,
            "b at {:?}",
            tb.translation
        );
        assert_eq!(tb.scale, Vec3::new(2.0, 2.0, 2.0));
        for entity in [a, b] {
            assert!(
                world
                    .inner_world()
                    .get::<khora_sdk::khora_data::ecs::Teleported>(entity)
                    .is_some(),
                "a dragged entity is flagged Teleported"
            );
        }
    }

    /// The editor camera the gizmos are sized for: up and to the side,
    /// looking at the origin.
    fn editor_view() -> ViewInfo {
        let eye = Vec3::new(30.0, 20.0, 30.0);
        ViewInfo::new(
            Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y).expect("a valid editor view"),
            Mat4::perspective_rh_zo(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0),
            eye,
        )
    }

    /// The lines the gizmo draws for `camera`, selected alone, posed by
    /// `transform` (its `GlobalTransform` the same matrix), with the viewport
    /// at `aspect`.
    fn camera_gizmo_lines(
        camera: Camera,
        transform: Transform,
        aspect: f32,
    ) -> Vec<GizmoLineInstance> {
        let mut world = GameWorld::new();
        let global = GlobalTransform::new(
            Mat4::from_translation(transform.translation)
                * Mat4::from_quat(transform.rotation)
                * Mat4::from_scale(transform.scale),
        );
        let entity = world.spawn((camera, transform, global));
        let mut state = EditorState {
            gizmo_mode: GizmoMode::Select,
            ..Default::default()
        };
        state.selection.insert(entity);
        collect_gizmo_lines(&world, &state, &editor_view(), aspect)
    }

    /// `true` if some line starts or ends at `p`.
    fn has_endpoint(lines: &[GizmoLineInstance], p: Vec3) -> bool {
        lines.iter().any(|line| {
            [line.start, line.end]
                .iter()
                .any(|e| (Vec3::new(e[0], e[1], e[2]) - p).length() < 1e-3)
        })
    }

    /// The four corners `(±half_w, ±half_h, z)` of a view-space plane.
    fn plane_corners(half_w: f32, half_h: f32, z: f32) -> [Vec3; 4] {
        [
            Vec3::new(-half_w, -half_h, z),
            Vec3::new(half_w, -half_h, z),
            Vec3::new(half_w, half_h, z),
            Vec3::new(-half_w, half_h, z),
        ]
    }

    fn assert_has_corners(lines: &[GizmoLineInstance], corners: &[Vec3], what: &str) {
        for &corner in corners {
            assert!(
                has_endpoint(lines, corner),
                "{what}: no line reaches {corner:?}"
            );
        }
    }

    /// Fov 60°, near 0.5, far 20, a 2:1 viewport: the frustum drawn is the
    /// camera's own, to its real far plane, at the viewport's aspect — not a
    /// fixed 45°/16:9 stand-in.
    #[test]
    fn a_selected_camera_draws_its_own_frustum_at_the_viewport_aspect() {
        let camera = Camera::new_perspective(60.0_f32.to_radians(), 0.5, 20.0);
        let lines = camera_gizmo_lines(camera, Transform::identity(), 2.0);

        let tan = 30.0_f32.to_radians().tan();
        assert_has_corners(
            &lines,
            &plane_corners(tan * 20.0 * 2.0, tan * 20.0, -20.0),
            "far plane",
        );
        assert_has_corners(
            &lines,
            &plane_corners(tan * 0.5 * 2.0, tan * 0.5, -0.5),
            "near plane",
        );
    }

    /// The same camera in a square viewport: the far plane keeps its height
    /// and is half as wide as in a 2:1 one.
    #[test]
    fn the_frustum_width_follows_the_viewport_aspect() {
        let camera = Camera::new_perspective(60.0_f32.to_radians(), 0.5, 20.0);
        let square = camera_gizmo_lines(camera, Transform::identity(), 1.0);

        let half_h = 30.0_f32.to_radians().tan() * 20.0;
        assert_has_corners(
            &square,
            &plane_corners(half_h, half_h, -20.0),
            "square viewport, far plane",
        );
        assert!(
            !has_endpoint(&square, Vec3::new(half_h * 2.0, half_h, -20.0)),
            "a square viewport does not draw the 2:1 far plane"
        );
    }

    /// An orthographic camera 4 units tall in a 2:1 viewport sees a box
    /// 8 × 4 between its clip planes.
    #[test]
    fn a_selected_orthographic_camera_draws_a_box() {
        let camera = Camera::new_orthographic(4.0, 0.5, 5.0);
        let lines = camera_gizmo_lines(camera, Transform::identity(), 2.0);
        assert_has_corners(&lines, &plane_corners(4.0, 2.0, -0.5), "near plane");
        assert_has_corners(&lines, &plane_corners(4.0, 2.0, -5.0), "far plane");
    }

    /// The frustum is placed by the camera's rotation and translation only,
    /// as the render path builds the camera's view: scaling the camera
    /// entity does not stretch the volume it sees.
    #[test]
    fn a_scaled_camera_draws_the_same_frustum_as_an_unscaled_one() {
        let camera = Camera::new_perspective(60.0_f32.to_radians(), 0.5, 20.0);
        let translation = Vec3::new(4.0, 1.5, -6.0);
        let rotation = Quaternion::from_axis_angle(Vec3::Y, 0.7);
        let pose = Mat4::from_translation(translation) * Mat4::from_quat(rotation);

        let tan = 30.0_f32.to_radians().tan();
        let expected: Vec<Vec3> = plane_corners(tan * 20.0 * 2.0, tan * 20.0, -20.0)
            .into_iter()
            .chain(plane_corners(tan * 0.5 * 2.0, tan * 0.5, -0.5))
            .map(|corner| pose.transform_point(corner))
            .collect();

        let unscaled = camera_gizmo_lines(
            camera,
            Transform::new(translation, rotation, Vec3::ONE),
            2.0,
        );
        assert_has_corners(&unscaled, &expected, "unscaled camera");

        let scaled = camera_gizmo_lines(
            camera,
            Transform::new(translation, rotation, Vec3::new(3.0, 3.0, 3.0)),
            2.0,
        );
        assert_has_corners(&scaled, &expected, "camera scaled ×3");
    }

    /// The engine's own default orthographic camera — 1080 units tall,
    /// near −1, far 1000 — in a 16:9 viewport sees a box 1920 × 1080 between
    /// view depths +1 and −1000; selecting it draws that box.
    #[test]
    fn the_default_orthographic_camera_draws_its_frustum() {
        let lines = camera_gizmo_lines(
            Camera::default_orthographic(),
            Transform::identity(),
            16.0 / 9.0,
        );
        let reaches = |p: Vec3| {
            let tolerance = 1e-4 * p.length().max(1.0);
            lines.iter().any(|line| {
                [line.start, line.end]
                    .iter()
                    .any(|e| (Vec3::new(e[0], e[1], e[2]) - p).length() < tolerance)
            })
        };
        for corner in plane_corners(960.0, 540.0, 1.0)
            .into_iter()
            .chain(plane_corners(960.0, 540.0, -1000.0))
        {
            assert!(reaches(corner), "no line reaches {corner:?}");
        }
    }

    /// A camera whose clip planes make no projection — near at zero or
    /// behind the eye, far not beyond near — is what a user passes through
    /// while dragging those fields in the inspector, with the camera
    /// selected. The editor must keep running, and draw no line to infinity.
    #[test]
    fn a_selected_camera_with_unusable_clip_planes_keeps_the_editor_running() {
        let mut failures = Vec::new();
        for (near, far) in [(0.0, 20.0), (-1.0, 20.0), (5.0, 5.0), (20.0, 0.5)] {
            let camera = Camera::new_perspective(60.0_f32.to_radians(), near, far);
            match std::panic::catch_unwind(|| {
                camera_gizmo_lines(camera, Transform::identity(), 2.0)
            }) {
                Err(_) => failures.push(format!("near {near}, far {far}: panicked")),
                Ok(lines) => {
                    if lines
                        .iter()
                        .flat_map(|line| line.start.into_iter().chain(line.end))
                        .any(|c| !c.is_finite())
                    {
                        failures.push(format!("near {near}, far {far}: non-finite line"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The colour the gizmo draws a point or spot light's range sphere in.
    const RANGE_SPHERE_COLOR: [f32; 4] = [1.0, 0.7, 0.3, 1.0];

    /// The lines the gizmo draws for `light`, selected alone with the pointer
    /// tool, standing at a point off the origin.
    fn light_gizmo_lines(light: Light) -> Vec<GizmoLineInstance> {
        let mut world = GameWorld::new();
        let transform = Transform::new(Vec3::new(2.0, 1.0, -3.0), Quaternion::IDENTITY, Vec3::ONE);
        let global = GlobalTransform::new(Mat4::from_translation(transform.translation));
        let entity = world.spawn((light, transform, global));
        let mut state = EditorState {
            gizmo_mode: GizmoMode::Select,
            ..Default::default()
        };
        state.selection.insert(entity);
        collect_gizmo_lines(&world, &state, &editor_view(), 16.0 / 9.0)
    }

    /// The lines of `lines` that belong to a light's range sphere.
    fn range_sphere_lines(lines: &[GizmoLineInstance]) -> usize {
        lines
            .iter()
            .filter(|line| line.color == RANGE_SPHERE_COLOR)
            .count()
    }

    /// A point or spot light whose range is zero, negative, NaN or infinite
    /// lights nothing a sphere could bound — a user passes through such values
    /// while typing in the inspector, with the light selected. The gizmo draws
    /// no range sphere for it, and no line it draws reaches a non-finite point.
    /// A light with a usable range still draws its sphere.
    #[test]
    fn a_light_with_an_unusable_range_draws_no_range_sphere() {
        use khora_sdk::khora_core::renderer::light::{PointLight, SpotLight};

        let point = |range: f32| {
            Light::new(LightType::Point(PointLight {
                range,
                ..PointLight::default()
            }))
        };
        let spot = |range: f32| {
            Light::new(LightType::Spot(SpotLight {
                range,
                ..SpotLight::default()
            }))
        };

        for (kind, light) in [("point", point(5.0)), ("spot", spot(5.0))] {
            assert!(
                range_sphere_lines(&light_gizmo_lines(light)) > 0,
                "a {kind} light of range 5 draws its range sphere"
            );
        }

        let mut failures = Vec::new();
        for range in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            for (kind, light) in [("point", point(range)), ("spot", spot(range))] {
                match std::panic::catch_unwind(|| light_gizmo_lines(light)) {
                    Err(_) => failures.push(format!("{kind} light, range {range}: panicked")),
                    Ok(lines) => {
                        let sphere = range_sphere_lines(&lines);
                        if sphere > 0 {
                            failures.push(format!(
                                "{kind} light, range {range}: {sphere} range-sphere lines drawn"
                            ));
                        }
                        if lines
                            .iter()
                            .flat_map(|line| line.start.into_iter().chain(line.end))
                            .any(|c| !c.is_finite())
                        {
                            failures.push(format!("{kind} light, range {range}: non-finite line"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The lines the gizmo draws for `light`, selected alone with the pointer
    /// tool, on an entity scaled by `scale`.
    fn scaled_light_gizmo_lines(light: Light, scale: Vec3) -> Vec<GizmoLineInstance> {
        let mut world = GameWorld::new();
        let transform = Transform::new(Vec3::new(2.0, 1.0, -3.0), Quaternion::IDENTITY, scale);
        let global = GlobalTransform::new(
            Mat4::from_translation(transform.translation) * Mat4::from_scale(scale),
        );
        let entity = world.spawn((light, transform, global));
        let mut state = EditorState {
            gizmo_mode: GizmoMode::Select,
            ..Default::default()
        };
        state.selection.insert(entity);
        collect_gizmo_lines(&world, &state, &editor_view(), 16.0 / 9.0)
    }

    /// A point light whose range is a usable distance, yet so large that
    /// the entity's scale carries its sphere past the largest float, draws no
    /// line to a non-finite point.
    #[test]
    fn a_huge_range_under_a_scaled_entity_draws_no_line_to_infinity() {
        use khora_sdk::khora_core::renderer::light::PointLight;

        let light = Light::new(LightType::Point(PointLight {
            range: f32::MAX,
            ..PointLight::default()
        }));
        let lines = scaled_light_gizmo_lines(light, Vec3::new(2.0, 2.0, 2.0));

        let non_finite = lines
            .iter()
            .filter(|line| {
                line.start
                    .into_iter()
                    .chain(line.end)
                    .any(|c| !c.is_finite())
            })
            .count();
        assert_eq!(
            non_finite, 0,
            "{non_finite} line(s) reach a non-finite point"
        );
    }

    /// A shadow-casting point or spot light of range 0.05 is only warned
    /// about (it casts no shadow, but lights): its range sphere is drawn.
    #[test]
    fn a_light_with_a_tiny_range_draws_its_range_sphere() {
        use khora_sdk::khora_core::renderer::light::{PointLight, SpotLight};

        for (kind, light) in [
            (
                "point",
                Light::new(LightType::Point(PointLight {
                    range: 0.05,
                    shadow_enabled: true,
                    ..PointLight::default()
                })),
            ),
            (
                "spot",
                Light::new(LightType::Spot(SpotLight {
                    range: 0.05,
                    shadow_enabled: true,
                    ..SpotLight::default()
                })),
            ),
        ] {
            assert!(
                range_sphere_lines(&light_gizmo_lines(light)) > 0,
                "a {kind} light of range 0.05 draws its range sphere"
            );
        }
    }
}
