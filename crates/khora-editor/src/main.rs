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

//! Khora Engine Editor — Phase 4+5 (Properties Inspector, Console, Asset Browser, Status Bar).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::Quaternion;
use khora_sdk::prelude::*;
use khora_sdk::{Application, Engine, EngineContext, GameWorld, InputEvent};

// ── Scene Tree Panel ───────────────────────────────────────────────

/// Displays the entity hierarchy extracted from the ECS World.
struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
}

impl SceneTreePanel {
    fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self { state }
    }

    /// Recursively render a single scene node and its children.
    fn render_node(ui: &mut dyn UiBuilder, node: &SceneNode, state: &mut EditorState) {
        let icon = match node.icon {
            EntityIcon::Camera => "\u{1F3A5} ",
            EntityIcon::Light => "\u{1F4A1} ",
            EntityIcon::Mesh => "\u{1F9CA} ",
            EntityIcon::Audio => "\u{1F50A} ",
            EntityIcon::Empty => "\u{25CB} ",
        };

        let selected = state.is_selected(node.entity);
        let is_renaming = state.renaming_entity == Some(node.entity);

        if is_renaming {
            // Inline rename text box.
            ui.horizontal(&mut |ui| {
                ui.label(icon);
                if ui.text_edit_singleline(&mut state.rename_buffer) {
                    // Text changed — will confirm on Enter/focus loss.
                }
            });
            // Confirm rename on Enter or if user clicks elsewhere (next frame).
            // For simplicity, we commit on every frame the buffer changes.
        } else {
            let label = format!("{}{}", icon, node.name);

            if node.children.is_empty() {
                // Leaf node — selectable label.
                if ui.selectable_label(selected, &label) {
                    if state.ctrl_held {
                        state.toggle_select(node.entity);
                    } else {
                        state.select(node.entity);
                    }
                }

                // Double-click to rename.
                if ui.is_last_item_double_clicked() {
                    state.renaming_entity = Some(node.entity);
                    state.rename_buffer = node.name.clone();
                }

                // Right-click context menu.
                let entity = node.entity;
                ui.context_menu_last(&mut |ui| {
                    if ui.button("Rename") {
                        state.renaming_entity = Some(entity);
                        state.rename_buffer = node.name.clone();
                    }
                    if ui.button("Duplicate") {
                        state.pending_duplicate = Some(entity);
                    }
                    ui.separator();
                    if ui.button("Delete") {
                        state.pending_delete = Some(entity);
                    }
                });
            } else {
                // Branch node — collapsible with children.
                let entity = node.entity;
                let children = node.children.clone();

                // Show the parent as a selectable + collapsible.
                if ui.selectable_label(selected, &label) {
                    if state.ctrl_held {
                        state.toggle_select(entity);
                    } else {
                        state.select(entity);
                    }
                }

                // Double-click to rename.
                if ui.is_last_item_double_clicked() {
                    state.renaming_entity = Some(entity);
                    state.rename_buffer = node.name.clone();
                }

                // Right-click context menu.
                ui.context_menu_last(&mut |ui| {
                    if ui.button("Rename") {
                        state.renaming_entity = Some(entity);
                        state.rename_buffer = node.name.clone();
                    }
                    if ui.button("Duplicate") {
                        state.pending_duplicate = Some(entity);
                    }
                    ui.separator();
                    if ui.button("Delete") {
                        state.pending_delete = Some(entity);
                    }
                });

                let id = format!("tree_{}", entity.index);
                ui.indent(&id, &mut |ui| {
                    for child in &children {
                        Self::render_node(ui, child, state);
                    }
                });
            }
        }
    }
}

impl EditorPanel for SceneTreePanel {
    fn id(&self) -> &str {
        "scene_tree"
    }
    fn title(&self) -> &str {
        "Scene Tree"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        // ── Toolbar: search bar + "Add" button ──
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut state.search_filter);
        });

        ui.horizontal(&mut |ui| {
            if ui.small_button("+ Empty") {
                state.pending_spawn = Some("Empty".to_owned());
            }
            if ui.small_button("+ Cube") {
                state.pending_spawn = Some("Cube".to_owned());
            }
            if ui.small_button("+ Light") {
                state.pending_spawn = Some("Light".to_owned());
            }
            if ui.small_button("+ Camera") {
                state.pending_spawn = Some("Camera".to_owned());
            }
        });

        ui.separator();

        // ── Entity count ──
        ui.small_label(&format!("{} entities", state.entity_count));
        ui.spacing(4.0);

        // ── Scene tree ──
        let filter = state.search_filter.clone();
        let roots = state.scene_roots.clone();

        ui.scroll_area("scene_tree_scroll", &mut |ui| {
            if roots.is_empty() {
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "Scene is empty");
            } else {
                for node in &roots {
                    if !filter.is_empty() && !Self::matches_filter(node, &filter) {
                        continue;
                    }
                    Self::render_node(ui, node, &mut state);
                }
            }
        });

        // ── Rename confirmation ──
        if let Some(entity) = state.renaming_entity {
            ui.spacing(4.0);
            ui.horizontal(&mut |ui| {
                if ui.small_button("\u{2714} Rename") {
                    let new_name = state.rename_buffer.clone();
                    if !new_name.is_empty() {
                        state.pending_rename = Some((entity, new_name));
                    }
                    state.renaming_entity = None;
                    state.rename_buffer.clear();
                }
                if ui.small_button("\u{2716} Cancel") {
                    state.renaming_entity = None;
                    state.rename_buffer.clear();
                }
            });
        }
    }
}

impl SceneTreePanel {
    /// Check if a node or any of its descendants match the filter.
    fn matches_filter(node: &SceneNode, filter: &str) -> bool {
        let filter_lower = filter.to_lowercase();
        if node.name.to_lowercase().contains(&filter_lower) {
            return true;
        }
        node.children.iter().any(|c| Self::matches_filter(c, &filter_lower))
    }
}

// ── Properties Panel (Phase 4) ─────────────────────────────────────

struct PropertiesPanel {
    state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
}

impl PropertiesPanel {
    fn new(state: Arc<Mutex<EditorState>>, history: Arc<Mutex<CommandHistory>>) -> Self {
        Self {
            state,
            command_history: history,
        }
    }

    fn render_transform(ui: &mut dyn UiBuilder, snap: &mut TransformSnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
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
            snap.rotation = Quaternion::from_euler_xyz(
                r[0].to_radians(),
                r[1].to_radians(),
                r[2].to_radians(),
            );
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

    fn render_camera(ui: &mut dyn UiBuilder, snap: &mut CameraSnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
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

    fn render_light(ui: &mut dyn UiBuilder, snap: &mut LightSnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
        let kind_options = ["Directional", "Point", "Spot"];
        let mut changed = false;

        if ui.combo_box("Type", &mut snap.light_kind, &kind_options) {
            changed = true;
        }

        let mut color = [snap.color.r, snap.color.g, snap.color.b, snap.color.a];
        if ui.color_edit("Color", &mut color) {
            snap.color = LinearRgba { r: color[0], g: color[1], b: color[2], a: color[3] };
            changed = true;
        }

        if ui.drag_value_f32("Intensity", &mut snap.intensity, 0.1) {
            changed = true;
        }

        // Direction (Directional and Spot)
        if snap.light_kind == 0 || snap.light_kind == 2 {
            let mut dir = [snap.direction.x, snap.direction.y, snap.direction.z];
            if ui.vec3_editor("Direction", &mut dir, 0.01) {
                snap.direction = khora_sdk::prelude::math::Vec3::new(dir[0], dir[1], dir[2]);
                changed = true;
            }
        }

        // Range (Point and Spot)
        if snap.light_kind == 1 || snap.light_kind == 2 {
            if ui.drag_value_f32("Range", &mut snap.range, 0.1) {
                changed = true;
            }
        }

        // Cone angles (Spot only)
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

    fn render_rigid_body(ui: &mut dyn UiBuilder, snap: &mut RigidBodySnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
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

        let mut lv = [snap.linear_velocity.x, snap.linear_velocity.y, snap.linear_velocity.z];
        if ui.vec3_editor("Linear Vel.", &mut lv, 0.1) {
            snap.linear_velocity = khora_sdk::prelude::math::Vec3::new(lv[0], lv[1], lv[2]);
            changed = true;
        }

        let mut av = [snap.angular_velocity.x, snap.angular_velocity.y, snap.angular_velocity.z];
        if ui.vec3_editor("Angular Vel.", &mut av, 0.1) {
            snap.angular_velocity = khora_sdk::prelude::math::Vec3::new(av[0], av[1], av[2]);
            changed = true;
        }

        if changed {
            edits.push(PropertyEdit::SetRigidBody(entity, *snap));
        }
    }

    fn render_collider(ui: &mut dyn UiBuilder, snap: &mut ColliderSnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
        let shape_options = ["Box", "Sphere", "Capsule"];
        let mut changed = false;

        if ui.combo_box("Shape", &mut snap.shape_index, &shape_options) {
            changed = true;
        }

        match snap.shape_index {
            0 => {
                let mut half = [snap.box_half_extents.x, snap.box_half_extents.y, snap.box_half_extents.z];
                if ui.vec3_editor("Half Extents", &mut half, 0.01) {
                    snap.box_half_extents = khora_sdk::prelude::math::Vec3::new(half[0], half[1], half[2]);
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

    fn render_audio(ui: &mut dyn UiBuilder, snap: &mut AudioSourceSnapshot, entity: EntityId, edits: &mut Vec<PropertyEdit>) {
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
                // Only display, actual undo/redo via Ctrl+Z/Y in inputs.
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

        // Clone inspected data for editing.
        let mut inspected = match state.inspected.clone() {
            Some(i) if i.entity == entity => i,
            _ => return,
        };

        let mut edits: Vec<PropertyEdit> = Vec::new();

        // Entity name.
        ui.heading(&format!("Entity {}", entity.index));
        let mut name = inspected.name.clone();
        if ui.text_edit_singleline(&mut name) {
            if name != inspected.name {
                edits.push(PropertyEdit::SetName(entity, name.clone()));
                inspected.name = name;
            }
        }

        ui.separator();

        // ── Transform ──
        if let Some(ref mut snap) = inspected.transform {
            ui.collapsing("Transform", true, &mut |ui| {
                Self::render_transform(ui, snap, entity, &mut edits);
            });
        }

        // ── Camera ──
        if let Some(ref mut snap) = inspected.camera {
            ui.collapsing("Camera", true, &mut |ui| {
                Self::render_camera(ui, snap, entity, &mut edits);
            });
        }

        // ── Light ──
        if let Some(ref mut snap) = inspected.light {
            ui.collapsing("Light", true, &mut |ui| {
                Self::render_light(ui, snap, entity, &mut edits);
            });
        }

        // ── RigidBody ──
        if let Some(ref mut snap) = inspected.rigid_body {
            ui.collapsing("Rigid Body", true, &mut |ui| {
                Self::render_rigid_body(ui, snap, entity, &mut edits);
            });
        }

        // ── Collider ──
        if let Some(ref mut snap) = inspected.collider {
            ui.collapsing("Collider", true, &mut |ui| {
                Self::render_collider(ui, snap, entity, &mut edits);
            });
        }

        // ── AudioSource ──
        if let Some(ref mut snap) = inspected.audio_source {
            ui.collapsing("Audio Source", true, &mut |ui| {
                Self::render_audio(ui, snap, entity, &mut edits);
            });
        }

        // Push edits into editor state for apply-back.
        for edit in edits {
            state.push_edit(edit);
        }
    }
}

// ── Asset Browser Panel (Phase 5) ──────────────────────────────────

struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    search_filter: String,
}

impl AssetBrowserPanel {
    fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self {
            state,
            search_filter: String::new(),
        }
    }
}

impl EditorPanel for AssetBrowserPanel {
    fn id(&self) -> &str {
        "asset_browser"
    }
    fn title(&self) -> &str {
        "Assets"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Toolbar: search + import button.
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut self.search_filter);
            if ui.small_button("Import…") {
                log::info!("Asset Browser: Import requested (not yet implemented)");
            }
        });
        ui.separator();

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let filter_lower = self.search_filter.to_lowercase();

        // Clone and group assets by type (from EditorState).
        struct CategorizedAssets {
            meshes: Vec<(usize, AssetEntry)>,
            materials: Vec<(usize, AssetEntry)>,
            textures: Vec<(usize, AssetEntry)>,
            shaders: Vec<(usize, AssetEntry)>,
            audio: Vec<(usize, AssetEntry)>,
            fonts: Vec<(usize, AssetEntry)>,
            scenes: Vec<(usize, AssetEntry)>,
            other: Vec<(usize, AssetEntry)>,
        }

        let mut cats = CategorizedAssets {
            meshes: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            shaders: Vec::new(),
            audio: Vec::new(),
            fonts: Vec::new(),
            scenes: Vec::new(),
            other: Vec::new(),
        };

        for (idx, entry) in state.asset_entries.iter().enumerate() {
            // Apply text filter.
            if !filter_lower.is_empty()
                && !entry.name.to_lowercase().contains(&filter_lower)
                && !entry.asset_type.to_lowercase().contains(&filter_lower)
            {
                continue;
            }

            let ty = entry.asset_type.to_lowercase();
            let item = (idx, entry.clone());
            if ty.contains("mesh") || ty.contains("gltf") || ty.contains("obj") {
                cats.meshes.push(item);
            } else if ty.contains("material") {
                cats.materials.push(item);
            } else if ty.contains("texture") || ty.contains("image") || ty.contains("png") || ty.contains("jpg") {
                cats.textures.push(item);
            } else if ty.contains("shader") || ty.contains("wgsl") {
                cats.shaders.push(item);
            } else if ty.contains("audio") || ty.contains("wav") || ty.contains("ogg") || ty.contains("mp3") {
                cats.audio.push(item);
            } else if ty.contains("font") || ty.contains("ttf") || ty.contains("otf") {
                cats.fonts.push(item);
            } else if ty.contains("scene") {
                cats.scenes.push(item);
            } else {
                cats.other.push(item);
            }
        }

        let no_assets = state.asset_entries.is_empty();
        let selected = state.selected_asset;

        // Use a closure to render categories with selection support.
        let render_category = |ui: &mut dyn UiBuilder,
                               icon: &str,
                               label: &str,
                               entries: &[(usize, AssetEntry)],
                               sel: &mut Option<usize>| {
            let header = format!("{} {} ({})", icon, label, entries.len());
            ui.collapsing(&header, !entries.is_empty(), &mut |ui| {
                if entries.is_empty() {
                    ui.colored_label([0.5, 0.5, 0.5, 1.0], "Empty");
                } else {
                    for (idx, entry) in entries {
                        let is_selected = *sel == Some(*idx);
                        ui.horizontal(&mut |ui| {
                            if ui.selectable_label(is_selected, &entry.name) {
                                *sel = Some(*idx);
                            }
                            ui.small_label(&format!("({})", entry.asset_type));
                        });
                    }
                }
            });
        };

        let mut sel_copy = selected;

        ui.scroll_area("asset_browser_scroll", &mut |ui| {
            if no_assets && filter_lower.is_empty() {
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "No assets loaded.");
                ui.spacing(4.0);
                ui.small_label("Use the VFS to register assets, or drag files into the project.");
                return;
            }

            render_category(ui, "\u{1F4E6}", "Meshes", &cats.meshes, &mut sel_copy);
            render_category(ui, "\u{1F3A8}", "Materials", &cats.materials, &mut sel_copy);
            render_category(ui, "\u{1F5BC}", "Textures", &cats.textures, &mut sel_copy);
            render_category(ui, "\u{2728}", "Shaders", &cats.shaders, &mut sel_copy);
            render_category(ui, "\u{1F3B5}", "Audio", &cats.audio, &mut sel_copy);
            render_category(ui, "\u{1F524}", "Fonts", &cats.fonts, &mut sel_copy);
            render_category(ui, "\u{1F3AC}", "Scenes", &cats.scenes, &mut sel_copy);
            if !cats.other.is_empty() {
                render_category(ui, "\u{1F4C4}", "Other", &cats.other, &mut sel_copy);
            }
        });

        // Show selected asset details.
        if let Some(idx) = sel_copy {
            if let Some(entry) = state.asset_entries.get(idx) {
                ui.separator();
                ui.small_label(&format!("Name: {}", entry.name));
                ui.small_label(&format!("Type: {}", entry.asset_type));
                ui.small_label(&format!("Path: {}", entry.source_path));
            }
        }

        state.selected_asset = sel_copy;
    }
}

// ── Console Panel (Phase 5) ────────────────────────────────────────

struct ConsolePanel {
    state: Arc<Mutex<EditorState>>,
    show_info: bool,
    show_warn: bool,
    show_error: bool,
    show_debug: bool,
    filter_text: String,
}

impl ConsolePanel {
    fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self {
            state,
            show_info: true,
            show_warn: true,
            show_error: true,
            show_debug: false,
            filter_text: String::new(),
        }
    }
}

impl EditorPanel for ConsolePanel {
    fn id(&self) -> &str {
        "console"
    }
    fn title(&self) -> &str {
        "Console"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Filter toolbar.
        ui.horizontal(&mut |ui| {
            ui.checkbox(&mut self.show_error, "\u{274C} Error");
            ui.checkbox(&mut self.show_warn, "\u{26A0} Warn");
            ui.checkbox(&mut self.show_info, "\u{2139} Info");
            ui.checkbox(&mut self.show_debug, "\u{1F41B} Debug");
        });
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut self.filter_text);
        });
        ui.separator();

        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let filter_lower = self.filter_text.to_lowercase();
        let show_info = self.show_info;
        let show_warn = self.show_warn;
        let show_error = self.show_error;
        let show_debug = self.show_debug;

        // Clone entries to release the lock before rendering.
        let entries: Vec<LogEntry> = state.log_entries.clone();
        drop(state);

        ui.scroll_area("console_scroll", &mut |ui| {
            if entries.is_empty() {
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "No log entries.");
                return;
            }

            for entry in entries.iter().rev().take(500) {
                // Level filter.
                let show = match entry.level {
                    LogLevel::Error => show_error,
                    LogLevel::Warn => show_warn,
                    LogLevel::Info => show_info,
                    LogLevel::Debug | LogLevel::Trace => show_debug,
                };
                if !show {
                    continue;
                }

                // Text filter.
                if !filter_lower.is_empty()
                    && !entry.message.to_lowercase().contains(&filter_lower)
                    && !entry.target.to_lowercase().contains(&filter_lower)
                {
                    continue;
                }

                let (color, prefix) = match entry.level {
                    LogLevel::Error => ([1.0, 0.3, 0.3, 1.0], "[ERROR]"),
                    LogLevel::Warn => ([1.0, 0.8, 0.2, 1.0], "[WARN] "),
                    LogLevel::Info => ([0.8, 0.8, 0.8, 1.0], "[INFO] "),
                    LogLevel::Debug => ([0.5, 0.7, 1.0, 1.0], "[DEBUG]"),
                    LogLevel::Trace => ([0.5, 0.5, 0.5, 1.0], "[TRACE]"),
                };

                let line = format!("{} {}: {}", prefix, entry.target, entry.message);
                ui.colored_label(color, &line);
            }
        });
    }
}

// ── 3D Viewport Panel ──────────────────────────────────────────────

struct ViewportPanel {
    handle: ViewportTextureHandle,
    state: Arc<Mutex<EditorState>>,
}

impl ViewportPanel {
    fn new(handle: ViewportTextureHandle, state: Arc<Mutex<EditorState>>) -> Self {
        Self { handle, state }
    }
}

impl EditorPanel for ViewportPanel {
    fn id(&self) -> &str {
        "viewport"
    }
    fn title(&self) -> &str {
        "Viewport"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let w = ui.available_width();
        let h = ui.available_height();
        if w > 1.0 && h > 1.0 {
            ui.viewport_image(self.handle, [w, h]);
            // Track hover state so the editor camera only activates inside the viewport.
            let hovered = ui.is_last_item_hovered();
            if let Ok(mut state) = self.state.lock() {
                state.viewport_hovered = hovered;
            }
        } else {
            ui.label("Viewport (no space)");
        }
    }
}

// ── Editor Application ─────────────────────────────────────────────

struct EditorApp {
    camera: Arc<Mutex<EditorCamera>>,
    editor_state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
    log_handle: Arc<Mutex<Vec<LogEntry>>>,
    /// Shared reference to the editor shell (for status bar updates).
    shell: Option<Arc<Mutex<Box<dyn EditorShell>>>>,
    /// Is the middle mouse button currently held?
    middle_down: bool,
    /// Is the right mouse button currently held?
    right_down: bool,
    /// Is Shift held?
    shift_held: bool,
    /// Is Ctrl held?
    ctrl_held: bool,
    /// Previous cursor position (for deltas).
    prev_cursor: Option<(f32, f32)>,
    /// For FPS calculation.
    last_frame_time: Instant,
}

impl EditorApp {
    /// Extracts a scene tree snapshot from the ECS World into EditorState.
    fn extract_scene_tree(world: &GameWorld, state: &mut EditorState) {
        // Collect all entity data.
        let entities: Vec<EntityId> = world.iter_entities().collect();
        state.entity_count = entities.len();

        // Build node info for each entity using per-entity lookups.
        let mut nodes: std::collections::HashMap<EntityId, SceneNode> =
            std::collections::HashMap::new();
        let mut parent_map: std::collections::HashMap<EntityId, EntityId> =
            std::collections::HashMap::new();

        for &entity in &entities {
            let name = world
                .get_component::<Name>(entity)
                .map(|n| n.as_str().to_owned())
                .unwrap_or_else(|| format!("Entity {}", entity.index));

            let icon = if world.get_component::<Camera>(entity).is_some() {
                EntityIcon::Camera
            } else if world.get_component::<Light>(entity).is_some() {
                EntityIcon::Light
            } else if world.get_component::<AudioSource>(entity).is_some() {
                EntityIcon::Audio
            } else if world.get_component::<MaterialComponent>(entity).is_some() {
                EntityIcon::Mesh
            } else {
                EntityIcon::Empty
            };

            if let Some(parent) = world.get_component::<Parent>(entity) {
                parent_map.insert(entity, parent.0);
            }

            nodes.insert(
                entity,
                SceneNode {
                    entity,
                    name,
                    icon,
                    children: Vec::new(),
                },
            );
        }

        // Reparent: attach children to their parents.
        let child_parent_pairs: Vec<(EntityId, EntityId)> =
            parent_map.iter().map(|(&c, &p)| (c, p)).collect();

        for (child_id, parent_id) in &child_parent_pairs {
            if let Some(child_node) = nodes.remove(child_id) {
                if let Some(parent_node) = nodes.get_mut(parent_id) {
                    parent_node.children.push(child_node);
                } else {
                    // Parent not found — treat as root.
                    nodes.insert(*child_id, child_node);
                }
            }
        }

        // Remaining nodes are roots.
        let mut roots: Vec<SceneNode> = nodes.into_values().collect();
        roots.sort_by_key(|n| n.entity.index);

        state.scene_roots = roots;
    }

    /// Process pending spawn requests from the scene tree.
    fn process_spawns(world: &mut GameWorld, state: &mut EditorState) {
        if let Some(request) = state.pending_spawn.take() {
            let entity = match request.as_str() {
                "Cube" => world.spawn((
                    Transform::identity(),
                    GlobalTransform::identity(),
                    Name::new("Cube"),
                )),
                "Light" => world.spawn((
                    Transform::identity(),
                    GlobalTransform::identity(),
                    Name::new("Light"),
                    Light::directional(),
                )),
                "Camera" => {
                    let cam = Camera::new_perspective(
                        std::f32::consts::FRAC_PI_4,
                        16.0 / 9.0,
                        0.1,
                        1000.0,
                    );
                    world.spawn((
                        Transform::identity(),
                        GlobalTransform::identity(),
                        Name::new("Camera"),
                        cam,
                    ))
                }
                _ => world.spawn((
                    Transform::identity(),
                    GlobalTransform::identity(),
                    Name::new(&request),
                )),
            };
            // Auto-select the newly created entity.
            state.select(entity);
            log::info!("Spawned entity {:?} ({})", entity, request);
        }
    }

    /// Duplicate an entity by cloning its known components.
    fn duplicate_entity(world: &mut GameWorld, entity: EntityId, state: &mut EditorState) {
        // Read source entity components.
        let name = world
            .get_component::<Name>(entity)
            .map(|n| format!("{} (Copy)", n.as_str()));
        let transform = world.get_component::<Transform>(entity).copied();
        let camera = world.get_component::<Camera>(entity).cloned();
        let light = world.get_component::<Light>(entity).cloned();
        let rigid_body = world.get_component::<RigidBody>(entity).cloned();
        let collider = world.get_component::<Collider>(entity).cloned();
        let audio_source = world.get_component::<AudioSource>(entity).cloned();

        // Spawn with base components.
        let new_entity = world.spawn((
            transform.unwrap_or_else(Transform::identity),
            GlobalTransform::identity(),
            Name::new(&name.unwrap_or_else(|| "Copy".to_owned())),
        ));

        // Attach optional components.
        if let Some(cam) = camera {
            world.add_component(new_entity, cam);
        }
        if let Some(l) = light {
            world.add_component(new_entity, l);
        }
        if let Some(rb) = rigid_body {
            world.add_component(new_entity, rb);
        }
        if let Some(col) = collider {
            world.add_component(new_entity, col);
        }
        if let Some(audio) = audio_source {
            world.add_component(new_entity, audio);
        }

        state.select(new_entity);
        log::info!("Duplicated entity {:?} → {:?}", entity, new_entity);
    }

    /// Extract component data for the single selected entity.
    fn extract_inspected(world: &GameWorld, state: &mut EditorState) {
        let entity = match state.single_selected() {
            Some(e) => e,
            None => {
                state.inspected = None;
                return;
            }
        };

        let name = world
            .get_component::<Name>(entity)
            .map(|n| n.as_str().to_owned())
            .unwrap_or_else(|| format!("Entity {}", entity.index));

        let transform = world.get_component::<Transform>(entity).map(|t| TransformSnapshot {
            translation: t.translation,
            rotation: t.rotation,
            scale: t.scale,
        });

        let camera = world.get_component::<Camera>(entity).map(|c| {
            let (projection_index, fov_y_radians, ortho_width, ortho_height) = match c.projection {
                ProjectionType::Perspective { fov_y_radians } => (0, fov_y_radians, 10.0, 10.0),
                ProjectionType::Orthographic { width, height } => (1, std::f32::consts::FRAC_PI_4, width, height),
            };
            CameraSnapshot {
                projection_index,
                fov_y_radians,
                ortho_width,
                ortho_height,
                aspect_ratio: c.aspect_ratio,
                z_near: c.z_near,
                z_far: c.z_far,
                is_active: c.is_active,
            }
        });

        let light = world.get_component::<Light>(entity).map(|l| {
            match &l.light_type {
                LightType::Directional(d) => LightSnapshot {
                    light_kind: 0,
                    direction: d.direction,
                    color: d.color,
                    intensity: d.intensity,
                    range: 0.0,
                    inner_cone_angle: 0.0,
                    outer_cone_angle: 0.0,
                    shadow_enabled: d.shadow_enabled,
                    shadow_bias: d.shadow_bias,
                    shadow_normal_bias: d.shadow_normal_bias,
                    enabled: l.enabled,
                },
                LightType::Point(p) => LightSnapshot {
                    light_kind: 1,
                    direction: khora_sdk::prelude::math::Vec3::ZERO,
                    color: p.color,
                    intensity: p.intensity,
                    range: p.range,
                    inner_cone_angle: 0.0,
                    outer_cone_angle: 0.0,
                    shadow_enabled: p.shadow_enabled,
                    shadow_bias: p.shadow_bias,
                    shadow_normal_bias: p.shadow_normal_bias,
                    enabled: l.enabled,
                },
                LightType::Spot(s) => LightSnapshot {
                    light_kind: 2,
                    direction: s.direction,
                    color: s.color,
                    intensity: s.intensity,
                    range: s.range,
                    inner_cone_angle: s.inner_cone_angle,
                    outer_cone_angle: s.outer_cone_angle,
                    shadow_enabled: s.shadow_enabled,
                    shadow_bias: s.shadow_bias,
                    shadow_normal_bias: s.shadow_normal_bias,
                    enabled: l.enabled,
                },
            }
        });

        let rigid_body = world.get_component::<RigidBody>(entity).map(|rb| {
            let body_type_index = match rb.body_type {
                BodyType::Dynamic => 0,
                BodyType::Static => 1,
                BodyType::Kinematic => 2,
            };
            RigidBodySnapshot {
                body_type_index,
                mass: rb.mass,
                ccd_enabled: rb.ccd_enabled,
                linear_velocity: rb.linear_velocity,
                angular_velocity: rb.angular_velocity,
            }
        });

        let collider = world.get_component::<Collider>(entity).map(|col| {
            let (shape_index, box_half_extents, sphere_radius, capsule_radius, capsule_half_height) =
                match &col.shape {
                    ColliderShape::Box(he) => (0, *he, 0.5, 0.5, 0.5),
                    ColliderShape::Sphere(r) => (1, khora_sdk::prelude::math::Vec3::ONE, *r, 0.5, 0.5),
                    ColliderShape::Capsule(half_h, r) => (2, khora_sdk::prelude::math::Vec3::ONE, 0.5, *r, *half_h),
                };
            ColliderSnapshot {
                shape_index,
                box_half_extents,
                sphere_radius,
                capsule_radius,
                capsule_half_height,
                friction: col.friction,
                restitution: col.restitution,
                is_sensor: col.is_sensor,
            }
        });

        let audio_source = world.get_component::<AudioSource>(entity).map(|a| AudioSourceSnapshot {
            volume: a.volume,
            looping: a.looping,
            autoplay: a.autoplay,
        });

        state.inspected = Some(InspectedEntity {
            entity,
            name,
            transform,
            camera,
            light,
            rigid_body,
            collider,
            audio_source,
        });
    }

    /// Apply pending property edits back to the ECS world.
    fn apply_edits(world: &mut GameWorld, state: &mut EditorState) {
        let edits = state.drain_edits();
        for edit in edits {
            match edit {
                PropertyEdit::SetName(entity, ref new_name) => {
                    if let Some(name) = world.get_component_mut::<Name>(entity) {
                        *name = Name::new(new_name);
                    }
                }
                PropertyEdit::SetTransform(entity, snap) => {
                    if let Some(t) = world.get_component_mut::<Transform>(entity) {
                        t.translation = snap.translation;
                        t.rotation = snap.rotation;
                        t.scale = snap.scale;
                    }
                }
                PropertyEdit::SetCamera(entity, snap) => {
                    if let Some(c) = world.get_component_mut::<Camera>(entity) {
                        c.projection = match snap.projection_index {
                            0 => ProjectionType::Perspective {
                                fov_y_radians: snap.fov_y_radians,
                            },
                            _ => ProjectionType::Orthographic {
                                width: snap.ortho_width,
                                height: snap.ortho_height,
                            },
                        };
                        c.aspect_ratio = snap.aspect_ratio;
                        c.z_near = snap.z_near;
                        c.z_far = snap.z_far;
                        c.is_active = snap.is_active;
                    }
                }
                PropertyEdit::SetLight(entity, snap) => {
                    if let Some(l) = world.get_component_mut::<Light>(entity) {
                        l.enabled = snap.enabled;
                        l.light_type = match snap.light_kind {
                            0 => LightType::Directional(DirectionalLight {
                                direction: snap.direction,
                                color: snap.color,
                                intensity: snap.intensity,
                                shadow_enabled: snap.shadow_enabled,
                                shadow_bias: snap.shadow_bias,
                                shadow_normal_bias: snap.shadow_normal_bias,
                            }),
                            1 => LightType::Point(PointLight {
                                color: snap.color,
                                intensity: snap.intensity,
                                range: snap.range,
                                shadow_enabled: snap.shadow_enabled,
                                shadow_bias: snap.shadow_bias,
                                shadow_normal_bias: snap.shadow_normal_bias,
                            }),
                            _ => LightType::Spot(SpotLight {
                                direction: snap.direction,
                                color: snap.color,
                                intensity: snap.intensity,
                                range: snap.range,
                                inner_cone_angle: snap.inner_cone_angle,
                                outer_cone_angle: snap.outer_cone_angle,
                                shadow_enabled: snap.shadow_enabled,
                                shadow_bias: snap.shadow_bias,
                                shadow_normal_bias: snap.shadow_normal_bias,
                            }),
                        };
                    }
                }
                PropertyEdit::SetRigidBody(entity, snap) => {
                    if let Some(rb) = world.get_component_mut::<RigidBody>(entity) {
                        rb.body_type = match snap.body_type_index {
                            0 => BodyType::Dynamic,
                            1 => BodyType::Static,
                            _ => BodyType::Kinematic,
                        };
                        rb.mass = snap.mass;
                        rb.ccd_enabled = snap.ccd_enabled;
                        rb.linear_velocity = snap.linear_velocity;
                        rb.angular_velocity = snap.angular_velocity;
                    }
                }
                PropertyEdit::SetCollider(entity, snap) => {
                    if let Some(col) = world.get_component_mut::<Collider>(entity) {
                        col.shape = match snap.shape_index {
                            0 => ColliderShape::Box(snap.box_half_extents),
                            1 => ColliderShape::Sphere(snap.sphere_radius),
                            _ => ColliderShape::Capsule(snap.capsule_half_height, snap.capsule_radius),
                        };
                        col.friction = snap.friction;
                        col.restitution = snap.restitution;
                        col.is_sensor = snap.is_sensor;
                    }
                }
                PropertyEdit::SetAudioSource(entity, snap) => {
                    if let Some(a) = world.get_component_mut::<AudioSource>(entity) {
                        a.volume = snap.volume;
                        a.looping = snap.looping;
                        a.autoplay = snap.autoplay;
                    }
                }
            }
        }
    }

    /// Delete all currently selected entities.
    fn delete_selection(&mut self, world: &mut GameWorld) {
        if let Ok(mut state) = self.editor_state.lock() {
            let to_delete: Vec<EntityId> = state.selection.iter().copied().collect();
            for entity in &to_delete {
                world.despawn(*entity);
            }
            if !to_delete.is_empty() {
                log::info!("Deleted {} entities", to_delete.len());
            }
            state.clear_selection();
            state.inspected = None;
        }
    }

    /// Process pending menu actions from the shell (File > New Scene, Edit > Undo, etc.).
    fn process_menu_actions(&mut self, world: &mut GameWorld) {
        let action = self
            .editor_state
            .lock()
            .ok()
            .and_then(|mut s| s.pending_menu_action.take());

        if let Some(action) = action {
            match action.as_str() {
                "new_scene" => {
                    // Despawn all entities to clear the scene.
                    if let Ok(mut state) = self.editor_state.lock() {
                        let all: Vec<EntityId> = world.iter_entities().collect();
                        for e in &all {
                            world.despawn(*e);
                        }
                        state.clear_selection();
                        state.inspected = None;
                        state.scene_roots.clear();
                        state.entity_count = 0;
                        log::info!("New scene created (cleared {} entities)", all.len());
                    }
                }
                "undo" => {
                    if let Ok(mut history) = self.command_history.lock() {
                        if let Some(edit) = history.undo() {
                            if let Ok(mut state) = self.editor_state.lock() {
                                state.push_edit(edit);
                            }
                        }
                    }
                }
                "redo" => {
                    if let Ok(mut history) = self.command_history.lock() {
                        if let Some(edit) = history.redo() {
                            if let Ok(mut state) = self.editor_state.lock() {
                                state.push_edit(edit);
                            }
                        }
                    }
                }
                "delete" => {
                    self.delete_selection(world);
                }
                "quit" => {
                    log::info!("Quit requested from menu");
                    // The event loop will handle shutdown via CloseRequested.
                    std::process::exit(0);
                }
                other => {
                    log::info!("Unhandled menu action: {}", other);
                }
            }
        }
    }
}

impl Application for EditorApp {
    fn new(context: EngineContext) -> Self {
        // Retrieve the shared editor camera.
        let camera = context
            .services
            .get::<Arc<Mutex<EditorCamera>>>()
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(EditorCamera::default())));

        let viewport_handle = context
            .services
            .get::<ViewportTextureHandle>()
            .copied()
            .unwrap_or(PRIMARY_VIEWPORT);

        // Create shared editor state and command history.
        let editor_state = Arc::new(Mutex::new(EditorState::default()));
        let command_history = Arc::new(Mutex::new(CommandHistory::default()));

        // Set up log capture — read the shared handle for log_entries sync.
        let (capture, log_handle) = EditorLogCapture::new();
        // Ignore error if logger is already set (e.g. in tests).
        let _ = log::set_boxed_logger(Box::new(capture));
        log::set_max_level(log::LevelFilter::Debug);

        // Retrieve the abstract editor shell and register panels.
        let shell_ref = context
            .services
            .get::<Arc<Mutex<Box<dyn EditorShell>>>>()
            .cloned();

        if let Some(ref shell) = shell_ref {
            if let Ok(mut shell) = shell.lock() {
                // Give the shell access to editor state for toolbar/menu interactions.
                shell.set_editor_state(editor_state.clone());

                shell.register_panel(
                    PanelLocation::Left,
                    Box::new(SceneTreePanel::new(editor_state.clone())),
                );
                shell.register_panel(
                    PanelLocation::Right,
                    Box::new(PropertiesPanel::new(
                        editor_state.clone(),
                        command_history.clone(),
                    )),
                );
                shell.register_panel(PanelLocation::Bottom, Box::new(AssetBrowserPanel::new(editor_state.clone())));
                shell.register_panel(
                    PanelLocation::Bottom,
                    Box::new(ConsolePanel::new(editor_state.clone())),
                );
                shell.register_panel(
                    PanelLocation::Center,
                    Box::new(ViewportPanel::new(viewport_handle, editor_state.clone())),
                );
                log::info!("EditorApp: panels registered with shell.");
            }
        } else {
            log::warn!("EditorApp: no EditorShell found in ServiceRegistry.");
        }

        Self {
            camera,
            editor_state,
            command_history,
            log_handle,
            shell: shell_ref,
            middle_down: false,
            right_down: false,
            shift_held: false,
            ctrl_held: false,
            prev_cursor: None,
            last_frame_time: Instant::now(),
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        // Read viewport hover state (set by ViewportPanel on the previous frame).
        let viewport_hovered = self
            .editor_state
            .lock()
            .map(|s| s.viewport_hovered)
            .unwrap_or(false);

        // ── Process input ──
        for input in inputs {
            match input {
                InputEvent::MouseButtonPressed { button } => match button {
                    MouseButton::Middle => self.middle_down = true,
                    MouseButton::Right => self.right_down = true,
                    _ => {}
                },
                InputEvent::MouseButtonReleased { button } => match button {
                    MouseButton::Middle => {
                        self.middle_down = false;
                        self.prev_cursor = None;
                    }
                    MouseButton::Right => {
                        self.right_down = false;
                        self.prev_cursor = None;
                    }
                    _ => {}
                },
                InputEvent::KeyPressed { key_code } => {
                    if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                        self.shift_held = true;
                    }
                    if key_code == "ControlLeft" || key_code == "ControlRight" {
                        self.ctrl_held = true;
                    }
                    // Gizmo mode shortcuts (Q/W/E/R).
                    if !self.ctrl_held {
                        if let Ok(mut state) = self.editor_state.lock() {
                            match key_code.as_str() {
                                "KeyQ" => state.gizmo_mode = GizmoMode::Select,
                                "KeyW" => state.gizmo_mode = GizmoMode::Move,
                                "KeyE" => state.gizmo_mode = GizmoMode::Rotate,
                                "KeyR" => state.gizmo_mode = GizmoMode::Scale,
                                _ => {}
                            }
                        }
                    }
                    // Delete selected entities.
                    if key_code == "Delete" {
                        self.delete_selection(world);
                    }
                    // Undo: Ctrl+Z
                    if key_code == "KeyZ" && self.ctrl_held {
                        if let Ok(mut history) = self.command_history.lock() {
                            if let Some(edit) = history.undo() {
                                if let Ok(mut state) = self.editor_state.lock() {
                                    state.push_edit(edit);
                                }
                            }
                        }
                    }
                    // Redo: Ctrl+Y
                    if key_code == "KeyY" && self.ctrl_held {
                        if let Ok(mut history) = self.command_history.lock() {
                            if let Some(edit) = history.redo() {
                                if let Ok(mut state) = self.editor_state.lock() {
                                    state.push_edit(edit);
                                }
                            }
                        }
                    }
                }
                InputEvent::KeyReleased { key_code } => {
                    if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                        self.shift_held = false;
                    }
                    if key_code == "ControlLeft" || key_code == "ControlRight" {
                        self.ctrl_held = false;
                    }
                }
                InputEvent::MouseMoved { x, y } => {
                    // Only orbit/pan when the 3D viewport is hovered.
                    if viewport_hovered {
                        if let Some((px, py)) = self.prev_cursor {
                            let dx = x - px;
                            let dy = y - py;

                            if let Ok(mut cam) = self.camera.lock() {
                                if self.middle_down && self.shift_held {
                                    cam.pan(dx, dy);
                                } else if self.middle_down || self.right_down {
                                    cam.orbit(dx, dy);
                                }
                            }
                        }
                    }
                    self.prev_cursor = Some((*x, *y));
                }
                InputEvent::MouseWheelScrolled { delta_y, .. } => {
                    // Only zoom when the 3D viewport is hovered.
                    if viewport_hovered {
                        if let Ok(mut cam) = self.camera.lock() {
                            cam.zoom(*delta_y);
                        }
                    }
                }
            }
        }

        // ── Process menu actions ──
        self.process_menu_actions(world);

        // ── Apply pending property edits ──
        if let Ok(mut state) = self.editor_state.lock() {
            Self::apply_edits(world, &mut state);
        }

        // ── Sync editor state from ECS ──
        if let Ok(mut state) = self.editor_state.lock() {
            state.ctrl_held = self.ctrl_held;

            // Process any pending spawn requests.
            Self::process_spawns(world, &mut state);

            // Process pending rename.
            if let Some((entity, new_name)) = state.pending_rename.take() {
                if let Some(name) = world.get_component_mut::<Name>(entity) {
                    *name = Name::new(&new_name);
                    log::info!("Renamed entity {:?} to '{}'", entity, new_name);
                }
            }

            // Process pending delete (from context menu).
            if let Some(entity) = state.pending_delete.take() {
                world.despawn(entity);
                state.selection.remove(&entity);
                if state.inspected.as_ref().map_or(false, |i| i.entity == entity) {
                    state.inspected = None;
                }
                log::info!("Deleted entity {:?}", entity);
            }

            // Process pending duplicate.
            if let Some(entity) = state.pending_duplicate.take() {
                Self::duplicate_entity(world, entity, &mut state);
            }

            // Extract fresh scene tree.
            Self::extract_scene_tree(world, &mut state);

            // Extract inspected entity data.
            Self::extract_inspected(world, &mut state);

            // Sync log entries from the capture.
            if let Ok(log_entries) = self.log_handle.lock() {
                state.log_entries.clone_from(&log_entries);
            }

            // Update status bar.
            let now = Instant::now();
            let dt = now.duration_since(self.last_frame_time).as_secs_f32();
            self.last_frame_time = now;
            state.status.frame_time_ms = dt * 1000.0;
            state.status.fps = if dt > 0.0 { 1.0 / dt } else { 0.0 };
            state.status.entity_count = state.entity_count;

            // Push status data to the shell for rendering.
            let status_copy = state.status.clone();
            drop(state); // Release EditorState lock before locking shell.
            if let Some(ref shell) = self.shell {
                if let Ok(mut shell) = shell.lock() {
                    shell.set_status(status_copy);
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    // Note: EditorLogCapture is set up inside EditorApp::new() via Application trait.
    Engine::run::<EditorApp>()?;
    Ok(())
}
