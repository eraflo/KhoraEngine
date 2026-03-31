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

use std::sync::{Arc, Mutex};

use khora_core::ui::editor::*;
use khora_sdk::prelude::math::Vec3;
use khora_sdk::prelude::*;
use khora_sdk::PRIMARY_VIEWPORT;

pub struct ViewportPanel {
    handle: ViewportTextureHandle,
    state: Arc<Mutex<EditorState>>,
    camera: Arc<Mutex<EditorCamera>>,
}

impl ViewportPanel {
    pub fn new(
        handle: ViewportTextureHandle,
        state: Arc<Mutex<EditorState>>,
        camera: Arc<Mutex<EditorCamera>>,
    ) -> Self {
        Self {
            handle,
            state,
            camera,
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
        let min_dim = viewport_size[0].min(viewport_size[1]);
        let scale = (min_dim / 700.0).clamp(0.65, 1.45);
        let length = 26.0 * scale;
        let margin = 12.0 * scale;
        let center = [
            viewport_min[0] + margin + length,
            viewport_min[1] + viewport_size[1] - margin - length,
        ];
        let plate_half = 32.0 * scale;

        // Backplate.
        ui.paint_rect_filled(
            [center[0] - plate_half, center[1] - plate_half],
            [plate_half * 2.0, plate_half * 2.0],
            [0.03, 0.04, 0.06, 0.55],
            6.0 * scale,
        );

        let (right, up) = if let Ok(cam) = self.camera.lock() {
            (cam.right(), cam.up())
        } else {
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
        };

        let line_w = (1.8 * scale).clamp(1.2, 2.8);
        let label_offset = 4.0 * scale;
        let show_labels = min_dim >= 240.0;

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
            ui.paint_line(center, end, color, line_w);
            if show_labels {
                ui.paint_text([end[0] + label_offset, end[1] - 8.0 * scale], color, label);
            }
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
        "viewport"
    }
    fn title(&self) -> &str {
        "Viewport"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let w = ui.available_width();
        let h = ui.available_height();
        if w > 1.0 && h > 1.0 {
            if let Some(min) = ui.viewport_image(self.handle, [w, h]) {
                let hovered = ui.is_last_item_hovered();
                let mut show_camera_preview = false;
                if let Ok(mut state) = self.state.lock() {
                    state.viewport_hovered = hovered;
                    show_camera_preview = Self::has_camera_node(&state.scene_roots);
                }

                if w >= 170.0 && h >= 140.0 {
                    self.paint_axis_gizmo(ui, min, [w, h]);
                }

                if show_camera_preview {
                    self.paint_camera_preview(ui, min, [w, h]);
                }
            }
        } else {
            ui.label("Viewport (no space)");
        }
    }
}
