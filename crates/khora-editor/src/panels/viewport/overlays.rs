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

//! Overlays drawn over the viewport: the camera preview and the axis gizmo.

use super::ViewportPanel;
use khora_sdk::editor_ui::*;
use khora_sdk::prelude::math::Vec3;

impl ViewportPanel {
    pub(super) fn paint_camera_preview(
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

    pub(super) fn paint_axis_gizmo(
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
