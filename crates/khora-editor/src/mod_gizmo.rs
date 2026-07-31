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

//! Gizmo rendering and manipulation for the editor viewport.
//!
//! The geometry and the drag math live in `khora-core`
//! (`ui::editor::{gizmo, gizmo_interact}`); this module is the ECS side of it —
//! it reads the selection out of the world, hands the pure code what it needs,
//! and writes the resulting transforms back.

use khora_sdk::editor_ui::gizmo::manipulator;
use khora_sdk::editor_ui::{
    generate_selection_gizmos, gizmo_basis, gizmo_world_size, EditorState, GizmoBasis, GizmoDelta,
    GizmoKind, GizmoLineInstance, GizmoTransform, SelectionGizmo,
};
use khora_sdk::khora_core::math::{Mat4, Vec3};
use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::renderer::api::scene::mesh::Mesh;
use khora_sdk::khora_core::renderer::light::LightType;
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
pub fn collect_gizmo_lines(
    world: &GameWorld,
    editor_state: &EditorState,
    view_info: &ViewInfo,
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

        let kind = if world.get_component::<Camera>(entity_id).is_some() {
            GizmoKind::Camera {
                fov_y: std::f32::consts::FRAC_PI_4,
                aspect: 16.0 / 9.0,
                near: 0.1,
                far: 1000.0,
            }
        } else if let Some(light) = world.get_component::<Light>(entity_id) {
            match &light.light_type {
                LightType::Directional(_) => GizmoKind::DirectionalLight,
                LightType::Point(p) => GizmoKind::PointLight { radius: p.range },
                LightType::Spot(s) => GizmoKind::PointLight { radius: s.range },
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
            transform: world_matrix,
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
pub fn capture_starts(world: &GameWorld, editor_state: &EditorState) -> Vec<(EntityId, GizmoTransform)> {
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
        let Some(transform) = world.get_component_mut::<Transform>(entity) else {
            continue;
        };
        transform.translation = result.translation;
        transform.rotation = result.rotation;
        transform.scale = result.scale;
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
