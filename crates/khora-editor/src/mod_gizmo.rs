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

//! Gizmo rendering for the editor viewport.

use khora_sdk::editor_ui::{generate_selection_gizmos, EditorState, GizmoKind, GizmoLineInstance};
use khora_sdk::khora_core::math::Mat4;
use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::renderer::api::scene::mesh::Mesh;
use khora_sdk::khora_core::renderer::light::LightType;
use khora_sdk::prelude::ecs::{AudioSource, Camera, Light, Transform};
use khora_sdk::HandleComponent;
use khora_sdk::GameWorld;

/// Collects gizmo line instances for all selected entities.
pub fn collect_gizmo_lines(
    world: &GameWorld,
    editor_state: &EditorState,
    _view_info: &ViewInfo,
) -> Vec<GizmoLineInstance> {
    let mut entries: Vec<(Mat4, GizmoKind)> = Vec::new();

    for &entity_id in &editor_state.selection {
        let Some(transform) = world.get_component::<Transform>(entity_id) else {
            continue;
        };
        let global_transform = Mat4::from_translation(transform.translation)
            * Mat4::from_quat(transform.rotation)
            * Mat4::from_scale(transform.scale);

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
        } else if world.get_component::<HandleComponent<Mesh>>(entity_id).is_some() {
            GizmoKind::Mesh
        } else {
            GizmoKind::Empty
        };

        entries.push((global_transform, kind));
    }

    generate_selection_gizmos(&entries, editor_state.gizmo_mode)
}
