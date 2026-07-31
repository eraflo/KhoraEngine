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

//! 3D Viewport panel — displays the offscreen render texture.

use std::path::{Path, MAIN_SEPARATOR_STR};
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::math::Vec3;

pub struct ViewportPanel {
    handle: ViewportTextureHandle,
    state: Arc<Mutex<EditorState>>,
    camera: Arc<Mutex<EditorCamera>>,
    theme: UiTheme,
}

/// Snapshot of the few status fields the stats card paints, copied while
/// the EditorState mutex is held so we don't keep it locked during paint.
struct StatsSnap {
    fps: f32,
    frame_time_ms: f32,
    entity_count: usize,
    memory_used_mb: f32,
    draw_calls: u32,
    triangles: u64,
    vram_mb: f32,
}

impl ViewportPanel {
    pub fn new(
        handle: ViewportTextureHandle,
        state: Arc<Mutex<EditorState>>,
        camera: Arc<Mutex<EditorCamera>>,
        theme: UiTheme,
    ) -> Self {
        Self {
            handle,
            state,
            camera,
            theme,
        }
    }

    fn has_camera_node(nodes: &[SceneNode]) -> bool {
        for node in nodes {
            if node.icon == EntityIcon::Camera || Self::has_camera_node(&node.children) {
                return true;
            }
        }
        false
    }

    fn paint_camera_preview(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        // Keep the preview legible but avoid clutter on small viewports.
        if viewport_size[0] < 320.0 || viewport_size[1] < 220.0 {
            return;
        }

        let min_dim = viewport_size[0].min(viewport_size[1]);
        let scale = (min_dim / 700.0).clamp(0.75, 1.30);

        let preview_w = (viewport_size[0] * 0.26).clamp(120.0 * scale, 260.0 * scale);
        let preview_h = (preview_w * 0.62).clamp(84.0 * scale, 170.0 * scale);
        let margin = 10.0 * scale;

        let min = [
            viewport_min[0] + viewport_size[0] - preview_w - margin,
            viewport_min[1] + viewport_size[1] - preview_h - margin,
        ];
        let size = [preview_w, preview_h];

        // Background panel.
        ui.paint_rect_filled(min, size, [0.05, 0.06, 0.09, 0.86], 6.0 * scale);

        // Border.
        let x0 = min[0];
        let y0 = min[1];
        let x1 = min[0] + size[0];
        let y1 = min[1] + size[1];
        let border = [0.24, 0.28, 0.36, 1.0];
        let border_w = (1.0 * scale).clamp(1.0, 2.0);
        ui.paint_line([x0, y0], [x1, y0], border, border_w);
        ui.paint_line([x1, y0], [x1, y1], border, border_w);
        ui.paint_line([x1, y1], [x0, y1], border, border_w);
        ui.paint_line([x0, y1], [x0, y0], border, border_w);

        // Fake frame content area to make the placeholder more informative.
        let content_min = [x0 + 8.0 * scale, y0 + 34.0 * scale];
        let content_size = [
            (size[0] - 16.0 * scale).max(8.0),
            (size[1] - 42.0 * scale).max(8.0),
        ];
        ui.paint_rect_filled(
            content_min,
            content_size,
            [0.08, 0.11, 0.16, 0.95],
            4.0 * scale,
        );

        // Crosshair inside preview frame.
        let cx = content_min[0] + content_size[0] * 0.5;
        let cy = content_min[1] + content_size[1] * 0.5;
        ui.paint_line(
            [content_min[0] + 6.0 * scale, cy],
            [content_min[0] + content_size[0] - 6.0 * scale, cy],
            [0.30, 0.38, 0.50, 1.0],
            (1.0 * scale).clamp(1.0, 2.0),
        );
        ui.paint_line(
            [cx, content_min[1] + 6.0 * scale],
            [cx, content_min[1] + content_size[1] - 6.0 * scale],
            [0.30, 0.38, 0.50, 1.0],
            (1.0 * scale).clamp(1.0, 2.0),
        );

        ui.paint_text(
            [x0 + 8.0 * scale, y0 + 8.0 * scale],
            [0.88, 0.91, 0.95, 1.0],
            "Camera Preview",
        );
        ui.paint_text(
            [x0 + 8.0 * scale, y0 + 22.0 * scale],
            [0.62, 0.67, 0.75, 1.0],
            "MVP placeholder",
        );
    }

    fn paint_axis_gizmo(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let theme = &self.theme;
        let min_dim = viewport_size[0].min(viewport_size[1]);
        let scale = (min_dim / 700.0).clamp(0.75, 1.55);

        // Top-right corner. This is where every 3D tool puts the view-
        // orientation gizmo, and muscle memory is worth more here than
        // novelty. The transport lives at the bottom centre, so nothing
        // competes for the corner.
        let plate_half = 34.0 * scale;
        let length = 22.0 * scale;
        let margin = 12.0 * scale;
        let center = [
            viewport_min[0] + viewport_size[0] - margin - plate_half,
            viewport_min[1] + margin + plate_half,
        ];

        // A translucent puck so the gizmo stays legible over any scene without
        // hiding it.
        ui.paint_circle_filled(
            center,
            plate_half,
            khora_tool_ui::widgets::paint::tint(theme.background, 0.7),
        );
        ui.paint_circle_stroke(center, plate_half, theme.border, 1.0);

        let (right, up) = if let Ok(cam) = self.camera.lock() {
            (cam.right(), cam.up())
        } else {
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
        };

        let line_w = (2.0 * scale).clamp(1.5, 3.2);
        let label_offset = 5.0 * scale;
        let show_labels = min_dim >= 240.0;

        // Per-axis paint: line out from center + colored knob at the tip
        // with the axis label centered on the knob (compass-puck look).
        let paint_axis = |ui: &mut dyn UiBuilder,
                          axis: Vec3,
                          label: &str,
                          color: [f32; 4],
                          center: [f32; 2],
                          right: Vec3,
                          up: Vec3,
                          length: f32,
                          line_w: f32,
                          label_offset: f32,
                          show_labels: bool| {
            let sx = axis.dot(right);
            let sy = axis.dot(up);
            let end = [center[0] + sx * length, center[1] - sy * length];
            // Line from origin to knob center.
            ui.paint_line(center, end, color, line_w);
            // Knob.
            let knob_r = 7.0 * scale;
            ui.paint_circle_filled(end, knob_r, color);
            ui.paint_circle_stroke(end, knob_r, [0.05, 0.07, 0.10, 1.0], 1.0);
            if show_labels {
                ui.paint_text_styled(
                    [end[0], end[1] - knob_r * 0.5 - 1.0],
                    label,
                    10.0,
                    [0.02, 0.03, 0.06, 1.0],
                    FontFamilyHint::Monospace,
                    TextAlign::Center,
                );
            }
            let _ = label_offset;
        };

        paint_axis(
            ui,
            Vec3::new(1.0, 0.0, 0.0),
            "X",
            [0.95, 0.32, 0.28, 1.0],
            center,
            right,
            up,
            length,
            line_w,
            label_offset,
            show_labels,
        );
        paint_axis(
            ui,
            Vec3::new(0.0, 1.0, 0.0),
            "Y",
            [0.34, 0.88, 0.43, 1.0],
            center,
            right,
            up,
            length,
            line_w,
            label_offset,
            show_labels,
        );
        paint_axis(
            ui,
            Vec3::new(0.0, 0.0, 1.0),
            "Z",
            [0.35, 0.63, 0.97, 1.0],
            center,
            right,
            up,
            length,
            line_w,
            label_offset,
            show_labels,
        );
    }
}

impl EditorPanel for ViewportPanel {
    fn id(&self) -> &str {
        "khora.editor.viewport"
    }
    fn title(&self) -> &str {
        "Viewport"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Always reset the viewport's hovered/rect state at the start of
        // the frame — otherwise the values from the previous Scene-mode
        // frame leak into DCC mode and the input pipeline routes mouse
        // events into the engine when the viewport isn't even visible.
        if let Ok(mut state) = self.state.lock() {
            state.viewport_hovered = false;
            state.viewport_screen_rect = None;
        }

        // No mode check here: the workbench keeps a dock layout per workspace,
        // so this panel is only ever laid out in the one that names it.
        let w = ui.available_width();
        let h = ui.available_height();
        if w > 1.0 && h > 1.0 {
            if let Some(min) = ui.viewport_image(self.handle, [w, h]) {
                let hovered = ui.is_last_item_hovered();
                // Drop target: any asset tile dragged onto the viewport. The
                // asset browser tags its drag payload with `ASSET_DRAG_TAG`
                // (high 32 bits) so we can tell our drops apart from the
                // scene-tree reparent flow's `EntityId`-packed payloads. We
                // dispatch by the asset's declared type.
                if let Some(payload) = ui.dnd_take_drop_payload() {
                    // Resolve index and epoch under one lock: the payload's
                    // epoch stamp is only meaningful against the same read of
                    // `asset_entries` the index will address.
                    let entry = self.state.lock().ok().and_then(|s| {
                        let idx =
                            crate::panels::asset_browser::unpack_asset_drag(payload, s.asset_epoch)?;
                        s.asset_entries.get(idx as usize).cloned()
                    });
                    if let Some(entry) = entry {
                        self.dispatch_asset_drop(ui, &entry, min, [w, h]);
                    }
                }
                let mut show_camera_preview = false;
                if let Ok(mut state) = self.state.lock() {
                    state.viewport_hovered = hovered;
                    state.viewport_screen_rect = Some([min[0], min[1], w, h]);
                    show_camera_preview = Self::has_camera_node(&state.scene_roots);
                }

                if w >= 170.0 && h >= 140.0 {
                    self.paint_axis_gizmo(ui, min, [w, h]);
                }

                if show_camera_preview {
                    self.paint_camera_preview(ui, min, [w, h]);
                }

                // ── Branded overlays (Phase I) ────────────
                self.paint_tool_pill(ui, min);
                self.paint_transport_pill(ui, min, [w, h]);
                self.paint_stats_card(ui, min, [w, h]);
                self.paint_diamond_watermark(ui, min, [w, h]);
                self.paint_play_mode_indicator(ui, min, [w, h]);
            }
        } else {
            ui.label("Viewport (no space)");
        }
    }
}

impl ViewportPanel {
    fn paint_tool_pill(&self, ui: &mut dyn UiBuilder, viewport_min: [f32; 2]) {
        let theme = &self.theme;
        let (current_gizmo, play_mode) = match self.state.lock() {
            Ok(s) => (s.gizmo_mode, s.play_mode),
            Err(_) => return,
        };
        let _ = play_mode;

        let pill_x = viewport_min[0] + 12.0;
        let pill_y = viewport_min[1] + 12.0;
        let pill_h = 32.0;
        let btn_w = 32.0;

        let tools = [
            (Icon::Hand, GizmoMode::Select, "h-tool-hand"),
            (Icon::Move, GizmoMode::Move, "h-tool-move"),
            (Icon::Rotate, GizmoMode::Rotate, "h-tool-rotate"),
            (Icon::Scale, GizmoMode::Scale, "h-tool-scale"),
        ];
        // Local/World labels are decorations today (the toggle isn't wired).
        // Keep the pill snug around just the gizmo tools so the labels can't
        // overflow.
        let total_w = btn_w * tools.len() as f32 + 8.0;

        // bg
        ui.paint_rect_filled(
            [pill_x, pill_y],
            [total_w, pill_h],
            crate::widgets::paint::with_alpha(theme.surface_elevated, 0.92),
            999.0,
        );
        ui.paint_rect_stroke(
            [pill_x, pill_y],
            [total_w, pill_h],
            crate::widgets::paint::with_alpha(theme.separator, 0.6),
            999.0,
            1.0,
        );

        let mut cx = pill_x + 4.0;
        for (icon, mode, salt) in &tools {
            let active = current_gizmo == *mode;
            let r = [cx, pill_y + 3.0, btn_w - 4.0, pill_h - 6.0];
            let int = ui.interact_rect(salt, r);
            if active {
                ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], theme.surface_active, 999.0);
            }
            let icon_color = if active || int.hovered {
                theme.text
            } else {
                theme.text_dim
            };
            crate::widgets::paint::paint_icon(
                ui,
                [r[0] + 6.0, r[1] + 6.0],
                *icon,
                14.0,
                icon_color,
            );
            if int.clicked {
                if let Ok(mut s) = self.state.lock() {
                    s.gizmo_mode = *mode;
                }
            }
            cx += btn_w;
        }

        let _ = cx; // Local/World toggle removed until it's actually wired.
    }

    /// The transport — play, pause, stop — floating at the bottom centre.
    ///
    /// Each button's *state* is derived from `PlayMode`, so the control always
    /// tells the truth about what the engine is doing: a disabled Stop while
    /// editing means there is genuinely nothing to stop. Buttons that cannot
    /// act are dimmed and swallow their clicks rather than silently no-op.
    fn paint_transport_pill(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        use khora_tool_ui::widgets::paint::{fill_stroke, icon_centered, tint};

        let theme = &self.theme;
        let mode = self
            .state
            .lock()
            .ok()
            .map(|s| s.play_mode)
            .unwrap_or(PlayMode::Editing);

        let playing = mode == PlayMode::Playing;
        let paused = mode == PlayMode::Paused;
        let running = playing || paused;

        let btn = 26.0;
        let gap = 4.0;
        let pad = 6.0;
        let pill_w = btn * 3.0 + gap * 2.0 + pad * 2.0;
        let pill_h = btn + pad * 2.0;
        let pill_x = viewport_min[0] + (viewport_size[0] - pill_w) * 0.5;
        let pill_y = viewport_min[1] + viewport_size[1] - pill_h - 12.0;

        if viewport_size[0] < pill_w + 24.0 || viewport_size[1] < 80.0 {
            return;
        }

        fill_stroke(
            ui,
            [pill_x, pill_y, pill_w, pill_h],
            tint(theme.surface_elevated, 0.92),
            theme.border,
            pill_h * 0.5,
        );

        let mut x = pill_x + pad;
        let cy = pill_y + pad;

        // ── Play / resume ──
        // While playing, this is the "running" indicator rather than an action.
        let play_rect = [x, cy, btn, btn];
        let play_hit = ui.interact_rect("vp-play", play_rect);
        if playing {
            ui.paint_circle_filled([x + btn * 0.5, cy + btn * 0.5], btn * 0.5, theme.success);
        }
        icon_centered(
            ui,
            play_rect,
            Icon::Play,
            13.0,
            if playing {
                theme.text_inverse
            } else if play_hit.hovered {
                theme.success
            } else {
                theme.text
            },
        );
        if play_hit.clicked && !playing {
            self.dispatch("play");
        }
        x += btn + gap;

        // ── Pause ── only meaningful while something is running.
        let pause_rect = [x, cy, btn, btn];
        let pause_hit = ui.interact_rect("vp-pause", pause_rect);
        if paused {
            ui.paint_circle_filled([x + btn * 0.5, cy + btn * 0.5], btn * 0.5, theme.warning);
        }
        icon_centered(
            ui,
            pause_rect,
            Icon::Pause,
            13.0,
            if paused {
                theme.text_inverse
            } else if !running {
                theme.text_disabled
            } else if pause_hit.hovered {
                theme.warning
            } else {
                theme.text
            },
        );
        if pause_hit.clicked && playing {
            self.dispatch("pause");
        }
        x += btn + gap;

        // ── Stop ──
        let stop_rect = [x, cy, btn, btn];
        let stop_hit = ui.interact_rect("vp-stop", stop_rect);
        icon_centered(
            ui,
            stop_rect,
            Icon::Stop,
            13.0,
            if !running {
                theme.text_disabled
            } else if stop_hit.hovered {
                theme.error
            } else {
                theme.text
            },
        );
        if stop_hit.clicked && running {
            self.dispatch("stop");
        }
    }

    /// Queues a menu action for the app to pick up next tick.
    fn dispatch(&self, action: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.pending_menu_action = Some(action.to_owned());
        }
    }

    fn paint_stats_card(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let theme = &self.theme;
        let snapshot = match self.state.lock() {
            Ok(s) => StatsSnap {
                fps: s.status.fps,
                frame_time_ms: s.status.frame_time_ms,
                entity_count: s.entity_count,
                memory_used_mb: s.status.memory_used_mb,
                draw_calls: s.status.draw_calls,
                triangles: s.status.triangles,
                vram_mb: s.status.vram_mb,
            },
            Err(_) => return,
        };

        let card_h = 50.0;
        // Card width adapts to viewport: never wider than viewport - 24px
        // margin, never narrower than 320px (FPS cell would otherwise become
        // unreadable). On very small viewports we bail out entirely.
        let avail_w = viewport_size[0] - 24.0;
        if avail_w < 280.0 || viewport_size[1] < 120.0 {
            return;
        }
        let card_w = avail_w.min(480.0);
        let card_x = viewport_min[0] + 12.0;
        let card_y = viewport_min[1] + viewport_size[1] - card_h - 12.0;

        ui.paint_rect_filled(
            [card_x, card_y],
            [card_w, card_h],
            crate::widgets::paint::with_alpha(theme.surface_elevated, 0.85),
            theme.radius_lg,
        );
        ui.paint_rect_stroke(
            [card_x, card_y],
            [card_w, card_h],
            crate::widgets::paint::with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        // Format triangle counts compactly.
        let tris_str = if snapshot.triangles >= 1_000_000 {
            format!("{:.1}M", snapshot.triangles as f64 / 1_000_000.0)
        } else if snapshot.triangles >= 1_000 {
            format!("{:.1}K", snapshot.triangles as f64 / 1_000.0)
        } else {
            format!("{}", snapshot.triangles)
        };

        let stats: [(&str, String, [f32; 4]); 6] = [
            (
                "FPS",
                format!("{:.0}", snapshot.fps),
                if snapshot.fps > 55.0 {
                    theme.success
                } else if snapshot.fps > 30.0 {
                    theme.warning
                } else {
                    theme.error
                },
            ),
            (
                "FRAME",
                format!("{:.2}ms", snapshot.frame_time_ms),
                theme.text,
            ),
            ("ENTITIES", format!("{}", snapshot.entity_count), theme.text),
            ("DRAWS", format!("{}", snapshot.draw_calls), theme.text),
            ("TRIS", tris_str, theme.text),
            (
                if snapshot.vram_mb > 0.0 {
                    "VRAM"
                } else {
                    "MEM"
                },
                if snapshot.vram_mb > 0.0 {
                    format!("{:.1}GB", snapshot.vram_mb / 1024.0)
                } else {
                    format!("{:.0}MB", snapshot.memory_used_mb)
                },
                if snapshot.vram_mb > 1500.0 || snapshot.memory_used_mb > 1500.0 {
                    theme.warning
                } else {
                    theme.text
                },
            ),
        ];
        let cell_w = card_w / stats.len() as f32;
        for (i, (k, v, color)) in stats.iter().enumerate() {
            let cx = card_x + i as f32 * cell_w + cell_w * 0.5;
            ui.paint_text_styled(
                [cx, card_y + 8.0],
                k,
                9.5,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
            ui.paint_text_styled(
                [cx, card_y + 22.0],
                v,
                12.0,
                *color,
                FontFamilyHint::Monospace,
                TextAlign::Center,
            );
        }
    }

    // Empty marker — closes paint_stats_card

    fn paint_play_mode_indicator(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let theme = &self.theme;
        let (mode_label, color) = match self.state.lock().ok().map(|s| s.play_mode) {
            Some(PlayMode::Playing) => ("PLAY", theme.success),
            Some(PlayMode::Paused) => ("PAUSED", theme.warning),
            _ => return, // Editing — no indicator.
        };

        // Colored border around the entire viewport so the user can't miss
        // that they're not in edit mode.
        let stroke_w = 2.0;
        ui.paint_rect_stroke(
            [viewport_min[0], viewport_min[1]],
            viewport_size,
            crate::widgets::paint::with_alpha(color, 0.85),
            0.0,
            stroke_w,
        );

        // Label pill in the top-center.
        let label_w = ui.measure_text(mode_label, 11.0, FontFamilyHint::Proportional)[0] + 24.0;
        let label_h = 22.0;
        let lx = viewport_min[0] + (viewport_size[0] - label_w) * 0.5;
        let ly = viewport_min[1] + 12.0;
        ui.paint_rect_filled(
            [lx, ly],
            [label_w, label_h],
            crate::widgets::paint::with_alpha(color, 0.85),
            999.0,
        );
        ui.paint_circle_filled([lx + 10.0, ly + label_h * 0.5], 3.0, theme.background);
        ui.paint_text_styled(
            [lx + label_w * 0.5 + 6.0, ly + 4.5],
            mode_label,
            11.0,
            theme.background,
            FontFamilyHint::Proportional,
            TextAlign::Center,
        );
    }

    /// Routes a dropped asset by its declared type. Prefabs/scenes spawn or
    /// load; a mesh materialises at the unprojected drop point; a texture or
    /// material is assigned to the current selection.
    fn dispatch_asset_drop(
        &self,
        ui: &dyn UiBuilder,
        entry: &AssetEntry,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let rel = entry.source_path.clone();
        match entry.asset_type.as_str() {
            "prefab" => {
                if let Ok(mut state) = self.state.lock() {
                    state.pending_prefab_spawn = Some((rel, None));
                    log::info!("Viewport: prefab '{}' dropped — spawning", entry.name);
                }
            }
            "scene" => {
                let abs = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|s| s.project_folder.clone())
                    .map(|pf| {
                        Path::new(&pf)
                            .join("assets")
                            .join(rel.replace('/', MAIN_SEPARATOR_STR))
                            .to_string_lossy()
                            .to_string()
                    });
                match abs {
                    Some(abs) => {
                        if let Ok(mut state) = self.state.lock() {
                            state.pending_scene_load = Some(abs);
                            log::info!("Viewport: scene '{}' dropped — loading", entry.name);
                        }
                    }
                    None => log::warn!(
                        "Viewport: cannot load scene '{}' — no project folder set",
                        rel
                    ),
                }
            }
            "mesh" => {
                let point = self.compute_drop_point(ui, viewport_min, viewport_size);
                if let Ok(mut state) = self.state.lock() {
                    state.pending_spawn_mesh_asset = Some((rel, point, None));
                    log::info!(
                        "Viewport: mesh '{}' dropped at [{:.2}, {:.2}, {:.2}]",
                        entry.name,
                        point[0],
                        point[1],
                        point[2]
                    );
                }
            }
            "texture" | "material" => {
                if let Ok(mut state) = self.state.lock() {
                    match state.selection.iter().copied().next() {
                        Some(target) => {
                            state.pending_assign_texture = Some((rel, target));
                            log::info!(
                                "Viewport: '{}' dropped — assigning to selected entity",
                                entry.name
                            );
                        }
                        None => log::warn!(
                            "Viewport: select an entity to assign '{}' to",
                            entry.name
                        ),
                    }
                }
            }
            other => log::info!("Viewport: dropped asset type '{other}' is not droppable here"),
        }
    }

    /// Unprojects the drop point onto the ground plane (`y = 0`). Falls back to
    /// a fixed distance along the ray when the ray is parallel to the ground or
    /// the pointer position is unavailable (uses the viewport centre then).
    fn compute_drop_point(
        &self,
        ui: &dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) -> [f32; 3] {
        let [w, h] = viewport_size;
        let (local_x, local_y) = match ui.pointer_position() {
            Some([px, py]) => (px - viewport_min[0], py - viewport_min[1]),
            None => (w * 0.5, h * 0.5),
        };
        let ray = match self.camera.lock() {
            Ok(cam) => cam.screen_to_ray(local_x, local_y, w, h),
            Err(_) => return [0.0, 0.0, 0.0],
        };
        let point = if ray.direction.y.abs() > 1e-4 {
            let t = -ray.origin.y / ray.direction.y;
            if t > 0.0 {
                ray.origin + ray.direction * t
            } else {
                ray.origin + ray.direction * 10.0
            }
        } else {
            ray.origin + ray.direction * 10.0
        };
        [point.x, point.y, point.z]
    }

    fn paint_diamond_watermark(
        &self,
        ui: &mut dyn UiBuilder,
        viewport_min: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let theme = &self.theme;
        let cx = viewport_min[0] + viewport_size[0] * 0.5;
        let cy = viewport_min[1] + viewport_size[1] * 0.5;
        let size = (viewport_size[0].min(viewport_size[1]) * 0.18).clamp(80.0, 220.0);
        crate::widgets::brand::paint_diamond_outline(
            ui,
            cx,
            cy,
            size,
            crate::widgets::paint::with_alpha(theme.primary, 0.06),
            1.5,
        );
    }
}
