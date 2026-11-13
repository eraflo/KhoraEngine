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

//! What the egui UI builder paints directly, outside any widget: rectangles,
//! filled paths (plain or with a gradient), text, on the panel's own layer
//! or on the overlay layer above every panel.

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::{LinearGradient, RadialGradient};

use super::convert::{color_to_egui, corner_of, font_id_for, rect_at};
use super::ui_builder::EguiUiBuilder;

impl EguiUiBuilder<'_> {
    /// `points` tessellated as a white, anti-aliased fill: the coverage a
    /// gradient fill then tints vertex by vertex.
    pub(super) fn white_fill(&self, points: &[[f32; 2]]) -> egui::epaint::Mesh {
        use egui::epaint::{Mesh, PathShape, PathStroke, Tessellator};
        let ctx = self.ui.ctx();
        let options = ctx.tessellation_options(|o| *o);
        // The font texture size only normalises glyph uvs; a filled path has none.
        let mut tessellator = Tessellator::new(ctx.pixels_per_point(), options, [1, 1], vec![]);
        let path = PathShape {
            points: points.iter().map(|p| egui::pos2(p[0], p[1])).collect(),
            closed: true,
            fill: egui::Color32::WHITE,
            stroke: PathStroke::NONE,
        };
        let mut mesh = Mesh::default();
        tessellator.tessellate_path(&path, &mut mesh);
        mesh
    }

    /// A painter on the top foreground layer, clipped only to the whole screen.
    /// Used for overlay affordances (drag ghosts) that must stay visible when
    /// the cursor leaves the current panel's clip rect.
    pub(super) fn overlay_painter(&self) -> egui::Painter {
        self.ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("khora_overlay"),
        ))
    }

    /// Fills the polygon `points` with one colour; fewer than three points
    /// paint nothing.
    pub(super) fn fill_path(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        if points.len() < 3 {
            return;
        }
        use egui::epaint::{PathShape, PathStroke};
        let pts: Vec<egui::Pos2> = points.iter().map(|p| egui::pos2(p[0], p[1])).collect();
        self.ui.painter().add(egui::Shape::Path(PathShape {
            points: pts,
            closed: true,
            fill: color_to_egui(color),
            stroke: PathStroke::NONE,
        }));
    }

    /// Fills the polygon `points` with a linear gradient; fewer than three
    /// points paint nothing.
    pub(super) fn fill_path_linear(&mut self, points: &[[f32; 2]], gradient: LinearGradient<'_>) {
        if points.len() < 3 {
            return;
        }
        let mut mesh = self.white_fill(points);
        super::gradient_fill::split_at_stops(&mut mesh, &gradient);
        super::gradient_fill::tint(&mut mesh, &gradient);
        self.ui.painter().add(egui::Shape::mesh(mesh));
    }

    /// Fills the polygon `points` with a radial gradient; fewer than three
    /// points paint nothing.
    pub(super) fn fill_path_radial(&mut self, points: &[[f32; 2]], gradient: RadialGradient<'_>) {
        if points.len() < 3 {
            return;
        }
        // Vertex colours interpolate linearly, so the radial field is sampled
        // on small triangles wherever its colour changes.
        let mut mesh = self.white_fill(points);
        super::gradient_fill::subdivide_radial(&mut mesh, &gradient);
        super::gradient_fill::tint(&mut mesh, &gradient);
        self.ui.painter().add(egui::Shape::mesh(mesh));
    }

    /// The size `text` takes laid out on one line, in points.
    pub(super) fn text_size(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        let font_id = font_id_for(family, size);
        // Use the painter's helper to lay out text — handles fonts atlas
        // mutability internally in egui 0.33.
        let galley =
            self.ui
                .painter()
                .layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE);
        let r = galley.rect;
        [r.width(), r.height()]
    }
}

/// Fills a rounded rectangle with `painter`.
pub(super) fn fill_rect(
    painter: &egui::Painter,
    min: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    rounding: f32,
) {
    painter.rect_filled(
        rect_at(min, size),
        corner_of(rounding),
        color_to_egui(color),
    );
}

/// Strokes a rounded rectangle's inside edge with `painter`.
pub(super) fn stroke_rect(
    painter: &egui::Painter,
    min: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    rounding: f32,
    thickness: f32,
) {
    painter.rect_stroke(
        rect_at(min, size),
        corner_of(rounding),
        egui::Stroke::new(thickness, color_to_egui(color)),
        egui::epaint::StrokeKind::Inside,
    );
}
