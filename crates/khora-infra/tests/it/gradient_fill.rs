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

//! A polygon filled with a linear or radial gradient through the egui
//! builder: one anti-aliased mesh whose colour, wherever the shape is fully
//! covered, is the gradient's colour at that point.

use std::collections::HashMap;

use egui::epaint::Mesh;
use egui::{pos2, Color32, Pos2};
use khora_core::ui::editor::{GradientStop, LinearGradient, RadialGradient, UiBuilder};
use khora_infra::EguiUiBuilder;

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

const THREE: [GradientStop; 3] = [
    GradientStop {
        offset: 0.0,
        color: RED,
    },
    GradientStop {
        offset: 0.5,
        color: GREEN,
    },
    GradientStop {
        offset: 1.0,
        color: BLUE,
    },
];

/// The square `[10, 10] → [110, 110]`, filled red → green → blue along x,
/// painted in one egui frame: the meshes it produced.
fn painted_square() -> (Vec<Mesh>, LinearGradient<'static>) {
    let gradient = LinearGradient {
        from: [10.0, 0.0],
        to: [110.0, 0.0],
        stops: &THREE,
    };
    let points = [[10.0, 10.0], [110.0, 10.0], [110.0, 110.0], [10.0, 110.0]];
    let ctx = egui::Context::default();
    let textures = HashMap::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(200.0, 200.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.paint_path_filled_gradient(&points, gradient);
    });
    let meshes = output
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Mesh(mesh) => Some(mesh.as_ref().clone()),
            _ => None,
        })
        .collect();
    (meshes, gradient)
}

/// The colour `mesh` shows at `p`: the vertex colours of a triangle covering
/// `p`, interpolated barycentrically — what the rasteriser draws.
fn shade_at(mesh: &Mesh, p: Pos2) -> Option<Color32> {
    for tri in mesh.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[tri[k] as usize]);
        let det =
            (b.pos.x - a.pos.x) * (c.pos.y - a.pos.y) - (c.pos.x - a.pos.x) * (b.pos.y - a.pos.y);
        if det.abs() < 1e-9 {
            continue;
        }
        let wb =
            ((p.x - a.pos.x) * (c.pos.y - a.pos.y) - (c.pos.x - a.pos.x) * (p.y - a.pos.y)) / det;
        let wc =
            ((b.pos.x - a.pos.x) * (p.y - a.pos.y) - (p.x - a.pos.x) * (b.pos.y - a.pos.y)) / det;
        let wa = 1.0 - wb - wc;
        if wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5 {
            let (ca, cb, cc) = (a.color.to_array(), b.color.to_array(), c.color.to_array());
            let ch = |k: usize| {
                (ca[k] as f32 * wa + cb[k] as f32 * wb + cc[k] as f32 * wc).round() as u8
            };
            return Some(Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), ch(3)));
        }
    }
    None
}

#[test]
fn a_gradient_polygon_is_painted_in_the_gradients_colours_inside_and_feathered_at_its_edge() {
    let (meshes, gradient) = painted_square();
    assert_eq!(meshes.len(), 1, "the polygon is one mesh");
    let mesh = &meshes[0];

    for x in [15.0, 35.0, 60.0, 85.0, 105.0] {
        let p = pos2(x, 60.0);
        let c = gradient.color_at([x, 60.0]);
        let to_u8 = |v: f32| (v * 255.0).round() as i32;
        let want = [to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), 255];
        let got = shade_at(mesh, p).unwrap_or_else(|| panic!("nothing painted at {p:?}"));
        for (k, (g, w)) in got.to_array().iter().zip(want).enumerate() {
            assert!(
                (*g as i32 - w).abs() <= 2,
                "channel {k} at {p:?}: got {got:?}, expected {want:?}"
            );
        }
    }

    // Anti-aliasing: the outermost vertices are fully transparent.
    assert!(
        mesh.vertices
            .iter()
            .any(|v| v.color.a() == 0 && (v.pos.x < 10.0 || v.pos.y < 10.0)),
        "no feathering ring around the polygon"
    );
}

/// The square `[10, 10] → [110, 110]`, filled red → green → blue outward
/// from its centre to the inscribed circle, painted in one egui frame: the
/// meshes it produced.
fn painted_radial_square() -> (Vec<Mesh>, RadialGradient<'static>) {
    let gradient = RadialGradient {
        center: [60.0, 60.0],
        radius: 50.0,
        stops: &THREE,
    };
    let points = [[10.0, 10.0], [110.0, 10.0], [110.0, 110.0], [10.0, 110.0]];
    let ctx = egui::Context::default();
    let textures = HashMap::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(200.0, 200.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.paint_path_filled_radial(&points, gradient);
    });
    let meshes = output
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Mesh(mesh) => Some(mesh.as_ref().clone()),
            _ => None,
        })
        .collect();
    (meshes, gradient)
}

#[test]
fn a_radial_gradient_polygon_is_painted_in_the_gradients_colours_by_distance_from_its_centre() {
    let (meshes, gradient) = painted_radial_square();
    assert_eq!(meshes.len(), 1, "the polygon is one mesh");
    let mesh = &meshes[0];

    // Samples in several directions, clear of the centre, of the green ring
    // at half the radius and of the circle, where the colour is smooth.
    for distance in [15.0, 19.0, 31.0, 44.0] {
        for angle in [0.3_f32, 1.6, 2.9, 4.4, 5.7] {
            let p = pos2(60.0 + distance * angle.cos(), 60.0 + distance * angle.sin());
            let c = gradient.color_at([p.x, p.y]);
            let to_u8 = |v: f32| (v * 255.0).round() as i32;
            let want = [to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), 255];
            let got = shade_at(mesh, p).unwrap_or_else(|| panic!("nothing painted at {p:?}"));
            for (k, (g, w)) in got.to_array().iter().zip(want).enumerate() {
                assert!(
                    (*g as i32 - w).abs() <= 3,
                    "channel {k} at {p:?}: got {got:?}, expected {want:?}"
                );
            }
        }
    }

    // Anti-aliasing: the outermost vertices are fully transparent.
    assert!(
        mesh.vertices
            .iter()
            .any(|v| v.color.a() == 0 && (v.pos.x < 10.0 || v.pos.y < 10.0)),
        "no feathering ring around the polygon"
    );
}
