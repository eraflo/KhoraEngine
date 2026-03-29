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

//! Concrete [`EditorShell`] backed by egui native panels.
//!
//! Layout:
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │  Menu Bar (File | Edit | View | Build | Help)        │
//! ├──────────────────────────────────────────────────────┤
//! │  Toolbar  [Select] [Move] [Rotate] [Scale]  ▶ ⏸ ⏹   │
//! ├────────┬──────────────────────────┬──────────────────┤
//! │ Left   │       Center             │     Right        │
//! │ panels │       panels             │     panels       │
//! ├────────┴──────────────────────────┴──────────────────┤
//! │  Bottom panels (tabbed)                              │
//! └──────────────────────────────────────────────────────┘
//! ```

use super::theme::apply_theme;
use super::ui_builder::EguiUiBuilder;
use khora_core::ui::editor::panel::{EditorPanel, PanelLocation};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::editor::state::{EditorState, GizmoMode, StatusBarData};
use khora_core::ui::editor::theme::EditorTheme;
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Egui-backed editor shell using native `SidePanel`, `TopBottomPanel`, and
/// `CentralPanel` for the dock layout.
pub struct EguiEditorShell {
    ctx: egui::Context,
    left_panels: Vec<Box<dyn EditorPanel>>,
    right_panels: Vec<Box<dyn EditorPanel>>,
    bottom_panels: Vec<Box<dyn EditorPanel>>,
    center_panels: Vec<Box<dyn EditorPanel>>,
    theme: EditorTheme,
    theme_applied: bool,
    active_bottom_tab: usize,
    /// Maps abstract viewport handles to egui texture IDs.
    viewport_textures: HashMap<ViewportTextureHandle, egui::TextureId>,
    /// Status bar data (FPS, frame time, entity count, memory).
    status: StatusBarData,
    /// Shared editor state for toolbar/menu interactions.
    editor_state: Option<Arc<Mutex<EditorState>>>,
}

impl EguiEditorShell {
    /// Creates a new shell using the given egui context (shared with `EguiOverlay`).
    pub fn new(ctx: egui::Context, theme: EditorTheme) -> Self {
        Self {
            ctx,
            left_panels: Vec::new(),
            right_panels: Vec::new(),
            bottom_panels: Vec::new(),
            center_panels: Vec::new(),
            theme,
            theme_applied: false,
            active_bottom_tab: 0,
            viewport_textures: HashMap::new(),
            status: StatusBarData::default(),
            editor_state: None,
        }
    }

    /// Registers an abstract viewport handle → egui texture ID mapping.
    ///
    /// Called by the render system after `overlay.register_viewport_texture()`.
    pub fn register_viewport_texture(
        &mut self,
        handle: ViewportTextureHandle,
        egui_id: egui::TextureId,
    ) {
        self.viewport_textures.insert(handle, egui_id);
    }

    /// Returns the egui texture ID for a given viewport handle, if registered.
    pub fn resolve_viewport_texture(&self, handle: ViewportTextureHandle) -> Option<egui::TextureId> {
        self.viewport_textures.get(&handle).copied()
    }

    fn render_menu_bar(ui: &mut egui::Ui, state: &Option<Arc<Mutex<EditorState>>>) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Scene").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("new_scene".to_owned());
                        }
                    }
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    log::info!("Menu: Open (not yet implemented)");
                    ui.close();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    log::info!("Menu: Save (not yet implemented)");
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    log::info!("Menu: Save As (not yet implemented)");
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("quit".to_owned());
                        }
                    }
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                if ui.button("Undo  (Ctrl+Z)").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("undo".to_owned());
                        }
                    }
                    ui.close();
                }
                if ui.button("Redo  (Ctrl+Y)").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("redo".to_owned());
                        }
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Delete  (Del)").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("delete".to_owned());
                        }
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Preferences…").clicked() {
                    log::info!("Menu: Preferences (not yet implemented)");
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Reset Layout").clicked() {
                    log::info!("Menu: Reset Layout (not yet implemented)");
                    ui.close();
                }
            });

            ui.menu_button("Build", |ui| {
                if ui.button("Build & Run").clicked() {
                    log::info!("Menu: Build & Run (not yet implemented)");
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Documentation").clicked() {
                    log::info!("Menu: Documentation (not yet implemented)");
                    ui.close();
                }
                if ui.button("About Khora Engine").clicked() {
                    log::info!("Menu: About Khora Engine v0.1");
                    ui.close();
                }
            });
        });
    }

    fn render_toolbar(ui: &mut egui::Ui, state: &Option<Arc<Mutex<EditorState>>>) {
        ui.horizontal(|ui| {
            ui.label("🔧");
            ui.separator();

            let current_mode = state
                .as_ref()
                .and_then(|s| s.lock().ok())
                .map(|s| s.gizmo_mode)
                .unwrap_or(GizmoMode::Select);

            let set_mode = |mode: GizmoMode| {
                if let Some(s) = state {
                    if let Ok(mut s) = s.lock() {
                        s.gizmo_mode = mode;
                    }
                }
            };

            if ui
                .selectable_label(current_mode == GizmoMode::Select, "⬚ Select")
                .on_hover_text("Select tool (Q)")
                .clicked()
            {
                set_mode(GizmoMode::Select);
            }
            if ui
                .selectable_label(current_mode == GizmoMode::Move, "✥ Move")
                .on_hover_text("Move tool (W)")
                .clicked()
            {
                set_mode(GizmoMode::Move);
            }
            if ui
                .selectable_label(current_mode == GizmoMode::Rotate, "↻ Rotate")
                .on_hover_text("Rotate tool (E)")
                .clicked()
            {
                set_mode(GizmoMode::Rotate);
            }
            if ui
                .selectable_label(current_mode == GizmoMode::Scale, "⤡ Scale")
                .on_hover_text("Scale tool (R)")
                .clicked()
            {
                set_mode(GizmoMode::Scale);
            }

            ui.separator();
            ui.add_space(ui.available_width() - 120.0);
            ui.add_enabled(false, egui::Button::new("▶ Play"));
            ui.add_enabled(false, egui::Button::new("⏸"));
            ui.add_enabled(false, egui::Button::new("⏹"));
        });
    }
}

impl EditorShell for EguiEditorShell {
    fn register_panel(&mut self, location: PanelLocation, panel: Box<dyn EditorPanel>) {
        log::info!(
            "EditorShell: registered panel '{}' at {:?}",
            panel.id(),
            location
        );
        match location {
            PanelLocation::Left => self.left_panels.push(panel),
            PanelLocation::Right => self.right_panels.push(panel),
            PanelLocation::Bottom => self.bottom_panels.push(panel),
            PanelLocation::Center => self.center_panels.push(panel),
        }
    }

    fn remove_panel(&mut self, id: &str) -> bool {
        let remove_from = |v: &mut Vec<Box<dyn EditorPanel>>| -> bool {
            if let Some(pos) = v.iter().position(|p| p.id() == id) {
                v.remove(pos);
                true
            } else {
                false
            }
        };
        remove_from(&mut self.left_panels)
            || remove_from(&mut self.right_panels)
            || remove_from(&mut self.bottom_panels)
            || remove_from(&mut self.center_panels)
    }

    fn set_theme(&mut self, theme: EditorTheme) {
        self.theme = theme;
        self.theme_applied = false;
    }

    fn set_status(&mut self, data: StatusBarData) {
        self.status = data;
    }

    fn set_editor_state(&mut self, state: Arc<Mutex<EditorState>>) {
        self.editor_state = Some(state);
    }

    fn show_frame(&mut self) {
        // Apply theme once (or when changed).
        if !self.theme_applied {
            apply_theme(&self.ctx, &self.theme);
            self.theme_applied = true;
        }

        // Clone the context (cheap Arc clone) to avoid borrow conflicts
        // between `ctx.show()` calls and `&mut self` field accesses.
        let ctx = self.ctx.clone();

        // ── Menu Bar ───────────────────────────────────
        let es = &self.editor_state;
        egui::TopBottomPanel::top("editor_menu_bar").show(&ctx, |ui| {
            Self::render_menu_bar(ui, es);
        });

        // ── Toolbar ────────────────────────────────────
        let es = &self.editor_state;
        egui::TopBottomPanel::top("editor_toolbar").show(&ctx, |ui| {
            Self::render_toolbar(ui, es);
        });

        // ── Status Bar (thin bottom strip) ─────────────
        {
            let status = &self.status;
            egui::TopBottomPanel::bottom("editor_status_bar")
                .exact_height(22.0)
                .show(&ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        ui.small(format!("FPS: {:.0}", status.fps));
                        ui.separator();
                        ui.small(format!("{:.1} ms", status.frame_time_ms));
                        ui.separator();
                        ui.small(format!("Entities: {}", status.entity_count));
                        ui.separator();
                        ui.small(format!("Mem: {:.1} MB", status.memory_used_mb));
                        // Push remaining to the right.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.small("Khora Engine v0.1");
                        });
                    });
                });
        }

        // Shared reference for viewport texture mapping (disjoint borrow).
        let vt = &self.viewport_textures;

        // ── Left sidebar ───────────────────────────────
        if !self.left_panels.is_empty() {
            egui::SidePanel::left("editor_left")
                .default_width(250.0)
                .width_range(120.0..=500.0)
                .resizable(true)
                .show(&ctx, |ui| {
                    for panel in &mut self.left_panels {
                        ui.heading(panel.title());
                        ui.separator();
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Right sidebar ──────────────────────────────
        if !self.right_panels.is_empty() {
            egui::SidePanel::right("editor_right")
                .default_width(300.0)
                .width_range(150.0..=600.0)
                .resizable(true)
                .show(&ctx, |ui| {
                    for panel in &mut self.right_panels {
                        ui.heading(panel.title());
                        ui.separator();
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Bottom strip (tabbed) ──────────────────────
        if !self.bottom_panels.is_empty() {
            let active_tab = &mut self.active_bottom_tab;
            let panels = &mut self.bottom_panels;

            egui::TopBottomPanel::bottom("editor_bottom")
                .default_height(200.0)
                .height_range(80.0..=500.0)
                .resizable(true)
                .show(&ctx, |ui| {
                    // Tab bar
                    ui.horizontal(|ui| {
                        for (i, panel) in panels.iter().enumerate() {
                            if ui.selectable_label(*active_tab == i, panel.title()).clicked() {
                                *active_tab = i;
                            }
                        }
                    });
                    ui.separator();

                    // Active tab content
                    if let Some(panel) = panels.get_mut(*active_tab) {
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Central viewport ───────────────────────────
        egui::CentralPanel::default().show(&ctx, |ui| {
            if self.center_panels.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("3D Viewport");
                });
            } else {
                for panel in &mut self.center_panels {
                    let mut builder = EguiUiBuilder::new(ui, vt);
                    panel.ui(&mut builder);
                }
            }
        });
    }
}
