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

//! Properties Inspector panel — displays and edits selected entity components.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{LinearRgba, Quaternion};
use khora_sdk::prelude::*;

pub struct PropertiesPanel {
    state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
}

impl PropertiesPanel {
    pub fn new(state: Arc<Mutex<EditorState>>, history: Arc<Mutex<CommandHistory>>) -> Self {
        Self {
            state,
            command_history: history,
        }
    }

    fn render_transform(
        ui: &mut dyn UiBuilder,
        snap: &mut TransformSnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let mut t = [snap.translation.x, snap.translation.y, snap.translation.z];
        let (rx, ry, rz) = snap.rotation.to_euler_xyz();
        let mut r = [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()];
        let mut s = [snap.scale.x, snap.scale.y, snap.scale.z];

        let mut changed = false;
        if ui.vec3_editor("Position", &mut t, 0.1) {
            snap.translation = khora_sdk::prelude::math::Vec3::new(t[0], t[1], t[2]);
            changed = true;
        }
        if ui.vec3_editor("Rotation", &mut r, 1.0) {
            snap.rotation =
                Quaternion::from_euler_xyz(r[0].to_radians(), r[1].to_radians(), r[2].to_radians());
            changed = true;
        }
        if ui.vec3_editor("Scale", &mut s, 0.01) {
            snap.scale = khora_sdk::prelude::math::Vec3::new(s[0], s[1], s[2]);
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetTransform(entity, *snap));
        }
    }

    fn render_camera(
        ui: &mut dyn UiBuilder,
        snap: &mut CameraSnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let proj_options = ["Perspective", "Orthographic"];
        let mut changed = false;

        if ui.combo_box("Projection", &mut snap.projection_index, &proj_options) {
            changed = true;
        }
        match snap.projection_index {
            0 => {
                let mut fov_deg = snap.fov_y_radians.to_degrees();
                if ui.drag_value_f32("FOV (deg)", &mut fov_deg, 0.5) {
                    snap.fov_y_radians = fov_deg.to_radians();
                    changed = true;
                }
            }
            1 => {
                if ui.drag_value_f32("Width", &mut snap.ortho_width, 0.1) {
                    changed = true;
                }
                if ui.drag_value_f32("Height", &mut snap.ortho_height, 0.1) {
                    changed = true;
                }
            }
            _ => {}
        }

        if ui.drag_value_f32("Aspect Ratio", &mut snap.aspect_ratio, 0.01) {
            changed = true;
        }
        if ui.drag_value_f32("Near", &mut snap.z_near, 0.01) {
            changed = true;
        }
        if ui.drag_value_f32("Far", &mut snap.z_far, 1.0) {
            changed = true;
        }
        if ui.checkbox(&mut snap.is_active, "Active") {
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetCamera(entity, *snap));
        }
    }

    fn render_light(
        ui: &mut dyn UiBuilder,
        snap: &mut LightSnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let kind_options = ["Directional", "Point", "Spot"];
        let mut changed = false;

        if ui.combo_box("Type", &mut snap.light_kind, &kind_options) {
            changed = true;
        }

        let mut color = [snap.color.r, snap.color.g, snap.color.b, snap.color.a];
        if ui.color_edit("Color", &mut color) {
            snap.color = LinearRgba {
                r: color[0],
                g: color[1],
                b: color[2],
                a: color[3],
            };
            changed = true;
        }

        if ui.drag_value_f32("Intensity", &mut snap.intensity, 0.1) {
            changed = true;
        }

        if snap.light_kind == 0 || snap.light_kind == 2 {
            let mut dir = [snap.direction.x, snap.direction.y, snap.direction.z];
            if ui.vec3_editor("Direction", &mut dir, 0.01) {
                snap.direction = khora_sdk::prelude::math::Vec3::new(dir[0], dir[1], dir[2]);
                changed = true;
            }
        }

        if (snap.light_kind == 1 || snap.light_kind == 2)
            && ui.drag_value_f32("Range", &mut snap.range, 0.1)
        {
            changed = true;
        }

        if snap.light_kind == 2 {
            let mut inner_deg = snap.inner_cone_angle.to_degrees();
            let mut outer_deg = snap.outer_cone_angle.to_degrees();
            if ui.drag_value_f32("Inner Cone (deg)", &mut inner_deg, 0.5) {
                snap.inner_cone_angle = inner_deg.to_radians();
                changed = true;
            }
            if ui.drag_value_f32("Outer Cone (deg)", &mut outer_deg, 0.5) {
                snap.outer_cone_angle = outer_deg.to_radians();
                changed = true;
            }
        }

        if ui.checkbox(&mut snap.shadow_enabled, "Shadows") {
            changed = true;
        }
        if snap.shadow_enabled {
            if ui.drag_value_f32("Shadow Bias", &mut snap.shadow_bias, 0.0001) {
                changed = true;
            }
            if ui.drag_value_f32("Normal Bias", &mut snap.shadow_normal_bias, 0.001) {
                changed = true;
            }
        }

        if ui.checkbox(&mut snap.enabled, "Enabled") {
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetLight(entity, *snap));
        }
    }

    fn render_rigid_body(
        ui: &mut dyn UiBuilder,
        snap: &mut RigidBodySnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let body_options = ["Dynamic", "Static", "Kinematic"];
        let mut changed = false;

        if ui.combo_box("Body Type", &mut snap.body_type_index, &body_options) {
            changed = true;
        }
        if ui.drag_value_f32("Mass", &mut snap.mass, 0.1) {
            changed = true;
        }
        if ui.checkbox(&mut snap.ccd_enabled, "CCD") {
            changed = true;
        }

        let mut lv = [
            snap.linear_velocity.x,
            snap.linear_velocity.y,
            snap.linear_velocity.z,
        ];
        if ui.vec3_editor("Linear Vel.", &mut lv, 0.1) {
            snap.linear_velocity = khora_sdk::prelude::math::Vec3::new(lv[0], lv[1], lv[2]);
            changed = true;
        }

        let mut av = [
            snap.angular_velocity.x,
            snap.angular_velocity.y,
            snap.angular_velocity.z,
        ];
        if ui.vec3_editor("Angular Vel.", &mut av, 0.1) {
            snap.angular_velocity = khora_sdk::prelude::math::Vec3::new(av[0], av[1], av[2]);
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetRigidBody(entity, *snap));
        }
    }

    fn render_collider(
        ui: &mut dyn UiBuilder,
        snap: &mut ColliderSnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let shape_options = ["Box", "Sphere", "Capsule"];
        let mut changed = false;

        if ui.combo_box("Shape", &mut snap.shape_index, &shape_options) {
            changed = true;
        }

        match snap.shape_index {
            0 => {
                let mut half = [
                    snap.box_half_extents.x,
                    snap.box_half_extents.y,
                    snap.box_half_extents.z,
                ];
                if ui.vec3_editor("Half Extents", &mut half, 0.01) {
                    snap.box_half_extents =
                        khora_sdk::prelude::math::Vec3::new(half[0], half[1], half[2]);
                    changed = true;
                }
            }
            1 => {
                if ui.drag_value_f32("Radius", &mut snap.sphere_radius, 0.01) {
                    changed = true;
                }
            }
            2 => {
                if ui.drag_value_f32("Radius", &mut snap.capsule_radius, 0.01) {
                    changed = true;
                }
                if ui.drag_value_f32("Half Height", &mut snap.capsule_half_height, 0.01) {
                    changed = true;
                }
            }
            _ => {}
        }

        if ui.drag_value_f32("Friction", &mut snap.friction, 0.01) {
            changed = true;
        }
        if ui.drag_value_f32("Restitution", &mut snap.restitution, 0.01) {
            changed = true;
        }
        if ui.checkbox(&mut snap.is_sensor, "Is Sensor") {
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetCollider(entity, *snap));
        }
    }

    fn render_audio(
        ui: &mut dyn UiBuilder,
        snap: &mut AudioSourceSnapshot,
        entity: EntityId,
        edits: &mut Vec<PropertyEdit>,
    ) {
        let mut changed = false;
        if ui.slider_f32("Volume", &mut snap.volume, 0.0, 1.0) {
            changed = true;
        }
        if ui.checkbox(&mut snap.looping, "Looping") {
            changed = true;
        }
        if ui.checkbox(&mut snap.autoplay, "Autoplay") {
            changed = true;
        }
        if changed {
            edits.push(PropertyEdit::SetAudioSource(entity, *snap));
        }
    }
}

impl EditorPanel for PropertiesPanel {
    fn id(&self) -> &str {
        "properties"
    }
    fn title(&self) -> &str {
        "Properties"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        // Undo/Redo buttons.
        let history = self.command_history.clone();
        ui.horizontal(&mut |ui| {
            if let Ok(hist) = history.lock() {
                let undo_label = if let Some(desc) = hist.undo_description() {
                    format!("Undo ({})", desc)
                } else {
                    "Undo".to_owned()
                };
                let redo_label = if let Some(desc) = hist.redo_description() {
                    format!("Redo ({})", desc)
                } else {
                    "Redo".to_owned()
                };
                ui.small_label(&undo_label);
                ui.small_label(&redo_label);
            }
        });

        ui.separator();

        let entity = match state.single_selected() {
            Some(e) => e,
            None => {
                if state.selection.is_empty() {
                    ui.label("Select an entity to inspect.");
                } else {
                    ui.label(&format!("{} entities selected", state.selection.len()));
                }
                return;
            }
        };

        let mut inspected = match state.inspected.clone() {
            Some(i) if i.entity == entity => i,
            _ => return,
        };

        let mut edits: Vec<PropertyEdit> = Vec::new();

        ui.heading(&format!("Entity {}", entity.index));
        let mut name = inspected.name.clone();
        if ui.text_edit_singleline(&mut name) && name != inspected.name {
            edits.push(PropertyEdit::SetName(entity, name.clone()));
            inspected.name = name;
        }

        ui.separator();

        if let Some(ref mut snap) = inspected.transform {
            ui.collapsing("Transform", true, &mut |ui| {
                Self::render_transform(ui, snap, entity, &mut edits);
            });
        }

        if let Some(ref mut snap) = inspected.camera {
            ui.collapsing("Camera", true, &mut |ui| {
                Self::render_camera(ui, snap, entity, &mut edits);
            });
        }

        if let Some(ref mut snap) = inspected.light {
            ui.collapsing("Light", true, &mut |ui| {
                Self::render_light(ui, snap, entity, &mut edits);
            });
        }

        if let Some(ref mut snap) = inspected.rigid_body {
            ui.collapsing("Rigid Body", true, &mut |ui| {
                Self::render_rigid_body(ui, snap, entity, &mut edits);
            });
        }

        if let Some(ref mut snap) = inspected.collider {
            ui.collapsing("Collider", true, &mut |ui| {
                Self::render_collider(ui, snap, entity, &mut edits);
            });
        }

        if let Some(ref mut snap) = inspected.audio_source {
            ui.collapsing("Audio Source", true, &mut |ui| {
                Self::render_audio(ui, snap, entity, &mut edits);
            });
        }

        // ── Add Component ──────────────────────────────
        ui.separator();
        ui.heading("Add Component");

        // Categorize available components
        let mut core_components = Vec::new();
        let mut physics_components = Vec::new();
        let mut audio_components = Vec::new();
        let mut ui_components = Vec::new();

        for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
            let already_present = inspected
                .present_component_types
                .contains(&reg.type_name.to_string());
            if already_present {
                continue;
            }
            let name = reg.type_name;
            let category = match name {
                "RigidBody"
                | "Collider"
                | "PhysicsMaterial"
                | "KinematicCharacterController"
                | "ActiveEvents"
                | "CollisionPairs"
                | "CollisionEvents"
                | "PhysicsDebugData" => &mut physics_components,
                "AudioSource" | "AudioListener" => &mut audio_components,
                "UiNode" | "UiTransform" | "UiStyle" | "UiColor" | "UiImage" | "UiBorder"
                | "UiInteraction" | "UiText" => &mut ui_components,
                _ => &mut core_components,
            };
            category.push(name.to_string());
        }

        if !core_components.is_empty() {
            ui.menu_button("\u{2B50} Core", &mut |ui| {
                for comp_name in &core_components {
                    if ui.button(comp_name.as_str()) {
                        state.pending_add_component = Some((entity, comp_name.clone()));
                        ui.close_menu();
                    }
                }
            });
        }
        if !physics_components.is_empty() {
            ui.menu_button("\u{1F533} Physics", &mut |ui| {
                for comp_name in &physics_components {
                    if ui.button(comp_name.as_str()) {
                        state.pending_add_component = Some((entity, comp_name.clone()));
                        ui.close_menu();
                    }
                }
            });
        }
        if !audio_components.is_empty() {
            ui.menu_button("\u{1F50A} Audio", &mut |ui| {
                for comp_name in &audio_components {
                    if ui.button(comp_name.as_str()) {
                        state.pending_add_component = Some((entity, comp_name.clone()));
                        ui.close_menu();
                    }
                }
            });
        }
        if !ui_components.is_empty() {
            ui.menu_button("\u{1F4CB} UI", &mut |ui| {
                for comp_name in &ui_components {
                    if ui.button(comp_name.as_str()) {
                        state.pending_add_component = Some((entity, comp_name.clone()));
                        ui.close_menu();
                    }
                }
            });
        }

        for edit in edits {
            state.push_edit(edit);
        }
    }
}
