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

use super::palette as pal;
use super::theme::apply_theme;
use super::ui_builder::EguiUiBuilder;
use khora_core::ui::editor::panel::{EditorPanel, PanelLocation};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::editor::state::{EditorState, GizmoMode, PlayMode, StatusBarData};
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
    pub fn resolve_viewport_texture(
        &self,
        handle: ViewportTextureHandle,
    ) -> Option<egui::TextureId> {
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
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("open".to_owned());
                        }
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("save".to_owned());
                        }
                    }
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("save_as".to_owned());
                        }
                    }
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
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("preferences".to_owned());
                        }
                    }
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Reset Layout").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("reset_layout".to_owned());
                        }
                    }
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Documentation").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("documentation".to_owned());
                        }
                    }
                    ui.close();
                }
                if ui.button("About Khora Engine").clicked() {
                    if let Some(s) = state {
                        if let Ok(mut s) = s.lock() {
                            s.pending_menu_action = Some("about".to_owned());
                        }
                    }
                    ui.close();
                }
            });
        });
    }

    fn render_toolbar(ui: &mut egui::Ui, state: &Option<Arc<Mutex<EditorState>>>) {
        // Toolbar background — slightly lighter than panel fill
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, pal::TAB_BAR_BG);
        // Bottom border
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0, pal::BORDER),
        );

        ui.horizontal(|ui| {
            ui.add_space(10.0);

            // Khora 4-point star logo
            let (logo_rect, _) =
                ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            paint_star(ui.painter(), logo_rect.center(), 9.0, pal::PRIMARY);
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Khora")
                    .strong()
                    .size(12.5)
                    .color(pal::TEXT),
            );
            ui.add_space(12.0);

            // Vertical separator
            let vr = egui::Rect::from_center_size(
                ui.next_widget_position() + egui::vec2(0.0, 0.0),
                egui::vec2(1.0, 18.0),
            );
            ui.painter().rect_filled(vr, 0.0, pal::BORDER);
            ui.add_space(12.0);

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

            let btn_size = egui::vec2(68.0, 22.0);

            {
                let active = current_mode == GizmoMode::Select;
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Select")
                                .size(11.5)
                                .color(if active { pal::PRIMARY } else { pal::TEXT_DIM }),
                        )
                        .fill(if active { pal::PRIMARY_DIM } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::NONE)
                        .min_size(btn_size),
                    )
                    .on_hover_text("Select  [Q]")
                    .clicked()
                {
                    set_mode(GizmoMode::Select);
                }
            }
            ui.add_space(2.0);
            {
                let active = current_mode == GizmoMode::Move;
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Move")
                                .size(11.5)
                                .color(if active { pal::PRIMARY } else { pal::TEXT_DIM }),
                        )
                        .fill(if active { pal::PRIMARY_DIM } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::NONE)
                        .min_size(btn_size),
                    )
                    .on_hover_text("Move  [W]")
                    .clicked()
                {
                    set_mode(GizmoMode::Move);
                }
            }
            ui.add_space(2.0);
            {
                let active = current_mode == GizmoMode::Rotate;
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Rotate")
                                .size(11.5)
                                .color(if active { pal::PRIMARY } else { pal::TEXT_DIM }),
                        )
                        .fill(if active { pal::PRIMARY_DIM } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::NONE)
                        .min_size(btn_size),
                    )
                    .on_hover_text("Rotate  [E]")
                    .clicked()
                {
                    set_mode(GizmoMode::Rotate);
                }
            }
            ui.add_space(2.0);
            {
                let active = current_mode == GizmoMode::Scale;
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Scale")
                                .size(11.5)
                                .color(if active { pal::PRIMARY } else { pal::TEXT_DIM }),
                        )
                        .fill(if active { pal::PRIMARY_DIM } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::NONE)
                        .min_size(btn_size),
                    )
                    .on_hover_text("Scale  [R]")
                    .clicked()
                {
                    set_mode(GizmoMode::Scale);
                }
            }

            ui.add_space(12.0);
            // Vertical separator
            let vr2 = egui::Rect::from_center_size(
                ui.next_widget_position() + egui::vec2(0.0, 0.0),
                egui::vec2(1.0, 18.0),
            );
            ui.painter().rect_filled(vr2, 0.0, pal::BORDER);
            ui.add_space(12.0);

            // Push transport controls to the right for a cleaner toolbar rhythm.
            ui.add_space((ui.available_width() - 170.0).max(8.0));

            // Play / Pause / Stop
            let play_mode = state
                .as_ref()
                .and_then(|s| s.lock().ok())
                .map(|s| s.play_mode)
                .unwrap_or(PlayMode::Editing);

            let is_editing = play_mode == PlayMode::Editing;
            let is_playing = play_mode == PlayMode::Playing;
            let is_paused = play_mode == PlayMode::Paused;

            // Play button — active when editing or paused
            let play_label = if is_paused { "▶ Resume" } else { "▶ Play" };
            let play_btn = ui.add_enabled(
                is_editing || is_paused,
                egui::Button::new(egui::RichText::new(play_label).size(11.5).color(
                    if is_editing || is_paused {
                        pal::PLAY_GREEN
                    } else {
                        pal::DISABLED
                    },
                ))
                .min_size(egui::vec2(82.0, 22.0)),
            );
            if play_btn.clicked() {
                if let Some(s) = state {
                    if let Ok(mut s) = s.lock() {
                        s.pending_menu_action = Some("play".to_owned());
                    }
                }
            }
            ui.add_space(2.0);

            // Pause button — active when playing
            let pause_btn = ui.add_enabled(
                is_playing,
                egui::Button::new(egui::RichText::new("⏸").size(11.5))
                    .min_size(egui::vec2(28.0, 22.0)),
            );
            if pause_btn.clicked() {
                if let Some(s) = state {
                    if let Ok(mut s) = s.lock() {
                        s.pending_menu_action = Some("pause".to_owned());
                    }
                }
            }
            ui.add_space(2.0);

            // Stop button — active when playing or paused
            let stop_btn = ui.add_enabled(
                is_playing || is_paused,
                egui::Button::new(egui::RichText::new("⏹").size(11.5).color(
                    if is_playing || is_paused {
                        pal::STOP_RED
                    } else {
                        pal::DISABLED
                    },
                ))
                .min_size(egui::vec2(28.0, 22.0)),
            );
            if stop_btn.clicked() {
                if let Some(s) = state {
                    if let Ok(mut s) = s.lock() {
                        s.pending_menu_action = Some("stop".to_owned());
                    }
                }
            }
        });
    }
}

/// Draws a 4-pointed diamond/star shape (Khora brand icon) on a painter.
fn paint_star(painter: &egui::Painter, center: egui::Pos2, size: f32, color: egui::Color32) {
    use egui::epaint::{PathShape, PathStroke};
    use egui::Pos2;
    let s = size;
    let t = s * 0.28;
    let points = vec![
        Pos2::new(center.x, center.y - s),
        Pos2::new(center.x + t, center.y - t),
        Pos2::new(center.x + s, center.y),
        Pos2::new(center.x + t, center.y + t),
        Pos2::new(center.x, center.y + s),
        Pos2::new(center.x - t, center.y + t),
        Pos2::new(center.x - s, center.y),
        Pos2::new(center.x - t, center.y - t),
    ];
    painter.add(egui::Shape::Path(PathShape {
        points,
        closed: true,
        fill: color,
        stroke: PathStroke::NONE,
    }));
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
        egui::TopBottomPanel::top("editor_menu_bar")
            .exact_height(26.0)
            .show(&ctx, |ui| {
                // Dark top-bar background
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 0.0, pal::TOOLBAR_BG);
                ui.painter().line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    egui::Stroke::new(1.0, pal::BORDER),
                );
                Self::render_menu_bar(ui, es);
            });

        // ── Toolbar ────────────────────────────────────
        let es = &self.editor_state;
        egui::TopBottomPanel::top("editor_toolbar")
            .exact_height(32.0)
            .show(&ctx, |ui| {
                Self::render_toolbar(ui, es);
            });

        // ── Status Bar (thin bottom strip) ─────────────
        {
            let status = &self.status;
            let (gizmo_mode, project_label) = self
                .editor_state
                .as_ref()
                .and_then(|s| s.lock().ok())
                .map(|s| {
                    let label = s
                        .project_name
                        .as_deref()
                        .map(|n| format!("{} — Khora v0.1", n))
                        .unwrap_or_else(|| "Khora v0.1".to_owned());
                    (s.gizmo_mode, label)
                })
                .unwrap_or((GizmoMode::Select, "Khora v0.1".to_owned()));

            let gizmo_label = match gizmo_mode {
                GizmoMode::Select => "⬚ Select",
                GizmoMode::Move => "✥ Move",
                GizmoMode::Rotate => "↻ Rotate",
                GizmoMode::Scale => "⤡ Scale",
            };

            egui::TopBottomPanel::bottom("editor_status_bar")
                .exact_height(22.0)
                .show(&ctx, |ui| {
                    let r = ui.max_rect();
                    ui.painter().rect_filled(r, 0.0, pal::STATUS_BAR_BG);
                    ui.painter().line_segment(
                        [r.left_top(), r.right_top()],
                        egui::Stroke::new(1.0, pal::BORDER),
                    );
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.small(
                            egui::RichText::new(format!("{:.0} fps", status.fps))
                                .color(pal::FPS_GREEN),
                        );
                        ui.separator();
                        ui.small(
                            egui::RichText::new(format!("{:.1} ms", status.frame_time_ms))
                                .color(pal::TEXT_DIM),
                        );
                        ui.separator();
                        ui.small(
                            egui::RichText::new(format!("{} entities", status.entity_count))
                                .color(pal::TEXT_DIM),
                        );
                        ui.separator();
                        ui.small(
                            egui::RichText::new(format!("{:.0} MB", status.memory_used_mb))
                                .color(pal::TEXT_DIM),
                        );

                        ui.add_space(6.0);

                        // Gizmo mode badge
                        let mode_color = pal::PRIMARY;
                        let mode_bg = pal::MODE_BG;
                        egui::Frame::NONE
                            .fill(mode_bg)
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(5, 1))
                            .show(ui, |ui| {
                                ui.small(
                                    egui::RichText::new(gizmo_label)
                                        .color(mode_color)
                                        .size(10.0),
                                );
                            });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.small(egui::RichText::new(&project_label).color(pal::TEXT_MUTED));
                            ui.add_space(8.0);
                            ui.small(
                                egui::RichText::new("Orbit: MMB  Pan: RMB  Zoom: Scroll")
                                    .color(pal::HINT_TEXT),
                            );
                        });
                    });
                });
        }

        // Shared reference for viewport texture mapping (disjoint borrow).
        let vt = &self.viewport_textures;

        // ── Bottom strip (tabbed) ──────────────────────
        //
        // MUST be declared before SidePanel::left/right so egui allocates
        // vertical space correctly and the resize handle is reachable.
        if !self.bottom_panels.is_empty() {
            let active_tab = &mut self.active_bottom_tab;
            let panels = &mut self.bottom_panels;

            egui::TopBottomPanel::bottom("editor_bottom")
                .default_height(200.0)
                .min_height(80.0)
                .max_height(600.0)
                .resizable(true)
                .show(&ctx, |ui| {
                    // Top resize grip — faint line
                    let grip_rect = {
                        let r = ui.available_rect_before_wrap();
                        egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 3.0))
                    };
                    ui.painter().rect_filled(grip_rect, 0.0, pal::BORDER);

                    // Tab bar — pill-style
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        for (i, panel) in panels.iter().enumerate() {
                            let active = *active_tab == i;
                            let tab_bg = if active {
                                pal::PRIMARY_DIM
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            let tab_text = if active { pal::PRIMARY } else { pal::TEXT_DIM };
                            let tab_btn = ui.add(
                                egui::Button::new(
                                    egui::RichText::new(panel.title())
                                        .size(11.5)
                                        .color(tab_text),
                                )
                                .fill(tab_bg)
                                .stroke(if active {
                                    egui::Stroke::new(1.0, pal::PRIMARY_BORDER)
                                } else {
                                    egui::Stroke::NONE
                                }),
                            );
                            if tab_btn.clicked() {
                                *active_tab = i;
                            }
                            ui.add_space(2.0);
                        }
                    });
                    ui.add(egui::Separator::default().spacing(2.0));

                    // Active tab content
                    if let Some(panel) = panels.get_mut(*active_tab) {
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Left sidebar ───────────────────────────────
        if !self.left_panels.is_empty() {
            egui::SidePanel::left("editor_left")
                .default_width(250.0)
                .width_range(120.0..=500.0)
                .resizable(true)
                .show(&ctx, |ui| {
                    for panel in &mut self.left_panels {
                        ui.add_space(6.0);
                        // Panel header row with accent line
                        ui.horizontal(|ui| {
                            // Blue accent bar
                            let bar_h = 14.0;
                            let (bar_rect, _) = ui
                                .allocate_exact_size(egui::vec2(2.0, bar_h), egui::Sense::hover());
                            ui.painter().rect_filled(
                                bar_rect,
                                egui::CornerRadius::same(1),
                                pal::PRIMARY,
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(panel.title())
                                    .strong()
                                    .size(11.0)
                                    .color(pal::TEXT_BRIGHT),
                            );
                        });
                        ui.add(egui::Separator::default().spacing(3.0));
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
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let bar_h = 14.0;
                            let (bar_rect, _) = ui
                                .allocate_exact_size(egui::vec2(2.0, bar_h), egui::Sense::hover());
                            ui.painter().rect_filled(
                                bar_rect,
                                egui::CornerRadius::same(1),
                                pal::ACCENT,
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(panel.title())
                                    .strong()
                                    .size(11.0)
                                    .color(pal::TEXT_BRIGHT),
                            );
                        });
                        ui.add(egui::Separator::default().spacing(3.0));
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
