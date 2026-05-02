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

//! Scene Tree panel — displays the entity hierarchy extracted from ECS.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::*;

/// Displays the entity hierarchy extracted from the ECS World.
pub struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
}

impl SceneTreePanel {
    pub fn new(state: Arc<Mutex<EditorState>>) -> Self {
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
            ui.horizontal(&mut |ui| {
                ui.label(icon);
                if ui.text_edit_singleline(&mut state.rename_buffer) {
                    // Text changed.
                }
                // Enter to confirm, Escape to cancel
                if ui.is_last_item_enter_pressed() {
                    let name = state.rename_buffer.clone();
                    state.pending_rename = Some((node.entity, name));
                    state.renaming_entity = None;
                }
                if ui.is_last_item_escape_pressed() {
                    state.renaming_entity = None;
                    state.rename_buffer.clear();
                }
            });
        } else {
            let entity = node.entity;
            let name = node.name.clone();
            let label = format!("{}{}", icon, name);
            let has_children = !node.children.is_empty();

            // Selection + double-click + context menu (shared for leaf and parent)
            if ui.selectable_label(selected, &label) {
                if state.ctrl_held {
                    state.toggle_select(entity);
                } else {
                    state.select(entity);
                }
            }

            if ui.is_last_item_double_clicked() {
                state.renaming_entity = Some(entity);
                state.rename_buffer = name.clone();
            }

            ui.context_menu_last(&mut |ui| {
                if ui.button("Rename") {
                    state.renaming_entity = Some(entity);
                    state.rename_buffer = name.clone();
                    ui.close_menu();
                }
                if ui.button("Duplicate") {
                    state.pending_duplicate = Some(entity);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Delete") {
                    state.pending_delete = Some(entity);
                    ui.close_menu();
                }
            });

            // Render children indented below the parent.
            if has_children {
                let children = node.children.clone();
                let id = format!("tree_{}", entity.index);
                ui.indent(&id, &mut |ui| {
                    for child in &children {
                        Self::render_node(ui, child, state);
                    }
                });
            }
        }
    }

    /// Check if a node or any of its descendants match the filter.
    fn matches_filter(node: &SceneNode, filter: &str) -> bool {
        let filter_lower = filter.to_lowercase();
        if node.name.to_lowercase().contains(&filter_lower) {
            return true;
        }
        node.children
            .iter()
            .any(|c| Self::matches_filter(c, &filter_lower))
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

        // ── Toolbar: search bar ──
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut state.search_filter);
        });

        ui.separator();

        ui.small_label(&format!("{} entities", state.entity_count));
        ui.spacing(4.0);

        let filter = state.search_filter.clone();
        let roots = state.scene_roots.clone();

        let mut pending: Option<String> = None;

        ui.scroll_area("scene_tree_scroll", &mut |ui| {
            if roots.is_empty() {
                ui.colored_label(EditorTheme::default().text_muted, "Scene is empty");
            } else {
                for node in &roots {
                    if !filter.is_empty() && !Self::matches_filter(node, &filter) {
                        continue;
                    }
                    Self::render_node(ui, node, &mut state);
                }
            }

            // Right-click on the panel background to spawn entities.
            ui.context_menu_panel(&mut |ui| {
                ui.label("Add Entity");
                ui.separator();
                if ui.button("\u{25CB} Empty") {
                    pending = Some("Empty".to_owned());
                    ui.close_menu();
                }
                ui.menu_button("\u{25A1} Geometry", &mut |ui| {
                    if ui.button("Cube") {
                        pending = Some("Cube".to_owned());
                        ui.close_menu();
                    }
                    if ui.button("Sphere") {
                        pending = Some("Sphere".to_owned());
                        ui.close_menu();
                    }
                    if ui.button("Plane") {
                        pending = Some("Plane".to_owned());
                        ui.close_menu();
                    }
                });
                ui.menu_button("\u{1F4A1} Light", &mut |ui| {
                    if ui.button("Directional Light") {
                        pending = Some("Light".to_owned());
                        ui.close_menu();
                    }
                });
                if ui.button("\u{1F3A5} Camera") {
                    pending = Some("Camera".to_owned());
                    ui.close_menu();
                }
            });
        });

        if pending.is_some() {
            state.pending_spawn = pending;
        }

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
