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

//! Gradient colouring of a tessellated egui mesh.
//!
//! egui fills a mesh by interpolating vertex colours across each triangle, so
//! a triangle that spans a gradient stop would blend straight past the stop's
//! colour. [`split_at_stops`] first cuts every such triangle along the stop's
//! line; [`tint`] then gives each vertex the gradient's colour at its position
//! while keeping the coverage the tessellator's feathering put in its alpha.
//!
//! A hard edge — two stops at one offset — has two colours on its line, one
//! per side. The split gives each side its own vertices there, and the tint
//! colours them with that side's stop.
//!
//! A radial gradient's stops lie on circles, which no straight cut follows:
//! [`subdivide`] instead refines the mesh until every edge is short, so the
//! per-vertex colours [`tint`] gives sample the field finely.

use khora_core::ui::editor::{GradientStop, LinearGradient, RadialGradient};

/// How far past a stop line a vertex must lie, in offset units, before its
/// triangle counts as crossing the line.
const STOP_EPSILON: f32 = 1e-4;

/// Splits every triangle of `mesh` that straddles a stop of `gradient` —
/// the first and last stops included, since the colour is held past them —
/// so no triangle spans one. New vertices interpolate position, uv and colour
/// linearly along the cut edge; the covered area is unchanged. On a hard edge
/// the two sides keep separate vertices.
pub(crate) fn split_at_stops(mesh: &mut egui::epaint::Mesh, gradient: &LinearGradient<'_>) {
    let mut previous: Option<f32> = None;
    for stop in gradient.stops {
        if previous.is_some_and(|p| (stop.offset - p).abs() <= f32::EPSILON) {
            continue;
        }
        previous = Some(stop.offset);
        split_along(mesh, gradient, stop.offset);
        if is_hard_edge(gradient, stop.offset) {
            unshare_line(mesh, gradient, stop.offset);
        }
    }
}

/// Cuts every triangle of `mesh` crossing the line `offset_at == stop` into a
/// fan on each side of it. Both fans share the crossing vertices, so the cut
/// leaves no crack.
fn split_along(mesh: &mut egui::epaint::Mesh, gradient: &LinearGradient<'_>, stop: f32) {
    let indices = std::mem::take(&mut mesh.indices);
    let mut out = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let d: [f32; 3] = std::array::from_fn(|k| gradient.offset_at(pos(mesh, tri[k])) - stop);
        let crosses = d.iter().any(|&x| x > STOP_EPSILON) && d.iter().any(|&x| x < -STOP_EPSILON);
        if !crosses {
            out.extend_from_slice(tri);
            continue;
        }
        let mut below = Vec::with_capacity(4);
        let mut above = Vec::with_capacity(4);
        for k in 0..3 {
            let (i, j) = (tri[k], tri[(k + 1) % 3]);
            let (di, dj) = (d[k], d[(k + 1) % 3]);
            if di <= 0.0 {
                below.push(i);
            }
            if di >= 0.0 {
                above.push(i);
            }
            if (di < 0.0 && dj > 0.0) || (di > 0.0 && dj < 0.0) {
                let v = lerp_vertex(mesh, i, j, di / (di - dj));
                below.push(v);
                above.push(v);
            }
        }
        for fan in [below, above] {
            for w in 1..fan.len().saturating_sub(1) {
                out.extend_from_slice(&[fan[0], fan[w], fan[w + 1]]);
            }
        }
    }
    mesh.indices = out;
}

/// Whether two or more stops sit at `offset`.
fn is_hard_edge(gradient: &LinearGradient<'_>, offset: f32) -> bool {
    gradient
        .stops
        .iter()
        .filter(|s| (s.offset - offset).abs() <= f32::EPSILON)
        .count()
        > 1
}

/// Gives the triangles past the line `offset_at == stop` their own copy of
/// every vertex on that line, so each side can carry its own colour there.
fn unshare_line(mesh: &mut egui::epaint::Mesh, gradient: &LinearGradient<'_>, stop: f32) {
    let mut copies = std::collections::HashMap::new();
    for t in 0..mesh.indices.len() / 3 {
        let tri = [
            mesh.indices[3 * t],
            mesh.indices[3 * t + 1],
            mesh.indices[3 * t + 2],
        ];
        if side_of(mesh, gradient, tri) <= stop {
            continue;
        }
        for (k, &i) in tri.iter().enumerate() {
            if (gradient.offset_at(pos(mesh, i)) - stop).abs() <= STOP_EPSILON {
                let copy = *copies.entry(i).or_insert_with(|| {
                    mesh.vertices.push(mesh.vertices[i as usize]);
                    (mesh.vertices.len() - 1) as u32
                });
                mesh.indices[3 * t + k] = copy;
            }
        }
    }
}

/// The gradient offset of a triangle's centroid: which side of a line it is on.
fn side_of<G: GradientField + ?Sized>(
    mesh: &egui::epaint::Mesh,
    gradient: &G,
    tri: [u32; 3],
) -> f32 {
    let p = tri.map(|i| pos(mesh, i));
    gradient.offset_at([
        (p[0][0] + p[1][0] + p[2][0]) / 3.0,
        (p[0][1] + p[1][1] + p[2][1]) / 3.0,
    ])
}

fn pos(mesh: &egui::epaint::Mesh, i: u32) -> [f32; 2] {
    let p = mesh.vertices[i as usize].pos;
    [p.x, p.y]
}

/// Appends the vertex `t` of the way from `i` to `j` and returns its index.
fn lerp_vertex(mesh: &mut egui::epaint::Mesh, i: u32, j: u32, t: f32) -> u32 {
    let (a, b) = (mesh.vertices[i as usize], mesh.vertices[j as usize]);
    let byte = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    let (ca, cb) = (a.color.to_array(), b.color.to_array());
    let v = egui::epaint::Vertex {
        pos: a.pos + (b.pos - a.pos) * t,
        uv: a.uv + (b.uv - a.uv) * t,
        color: egui::Color32::from_rgba_premultiplied(
            byte(ca[0], cb[0]),
            byte(ca[1], cb[1]),
            byte(ca[2], cb[2]),
            byte(ca[3], cb[3]),
        ),
    };
    mesh.vertices.push(v);
    (mesh.vertices.len() - 1) as u32
}

/// The most triangles subdivision may make of one fill: past it, triangles
/// are left as they are, so no gradient fill can cost more than this.
const MAX_TRIANGLES: usize = 65_536;

/// The finest subdivision, in points: below half a point, finer triangles
/// change nothing on screen.
const FINEST: f32 = 0.5;

/// Splits every triangle of `mesh` whose longest edge exceeds `max_edge`, at
/// edge midpoints and recursively, until no edge is longer. New vertices
/// interpolate position, uv and colour linearly; the covered area is
/// unchanged. A `max_edge` that is not a positive number leaves the mesh
/// alone.
#[cfg(test)]
pub(crate) fn subdivide(mesh: &mut egui::epaint::Mesh, max_edge: f32) {
    if !(max_edge.is_finite() && max_edge > 0.0) {
        return;
    }
    subdivide_by(mesh, |_| Some(max_edge));
}

/// Subdivides `mesh` where `gradient` changes colour: a sixteenth of the
/// radius between the first and last stop circles, half a point across a
/// hard edge (two stops at one offset), nothing where the colour is constant
/// (inside the first stop circle, outside the last).
pub(crate) fn subdivide_radial(mesh: &mut egui::epaint::Mesh, gradient: &RadialGradient<'_>) {
    let r = gradient.radius;
    let (Some(first), Some(last)) = (gradient.stops.first(), gradient.stops.last()) else {
        return;
    };
    if !(r.is_finite() && r > 0.0) {
        return;
    }
    let coarse = (r / 16.0).max(FINEST);
    let inner = first.offset.max(0.0) * r;
    let outer = last.offset * r;
    let hard: Vec<f32> = gradient
        .stops
        .windows(2)
        .filter(|w| (w[1].offset - w[0].offset).abs() <= f32::EPSILON)
        .map(|w| w[0].offset * r)
        .collect();
    let c = gradient.center;
    subdivide_by(mesh, |tri| {
        let (near, far) = distance_range(c, tri);
        if near >= outer || far <= inner {
            return None;
        }
        if hard.iter().any(|&h| near < h && h < far) {
            Some(FINEST)
        } else {
            Some(coarse)
        }
    });
}

/// Splits every triangle whose longest edge exceeds `limit(triangle)` (`None`:
/// leave it), at edge midpoints, recursively. Neighbours share the midpoint of
/// a shared edge. A triangle with a non-finite corner is left as it is.
fn subdivide_by(mesh: &mut egui::epaint::Mesh, limit: impl Fn(&[[f32; 2]; 3]) -> Option<f32>) {
    let mut midpoints = std::collections::HashMap::new();
    let triangles: Vec<[u32; 3]> = mesh
        .indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    let mut out = Vec::with_capacity(mesh.indices.len());
    for triangle in triangles {
        // Depth-first, so an unsplit mesh keeps its triangle order.
        let mut pending = vec![triangle];
        while let Some(tri) = pending.pop() {
            let corners = tri.map(|i| pos(mesh, i));
            let finite = corners.iter().all(|p| p[0].is_finite() && p[1].is_finite());
            let budget_left = out.len() / 3 + pending.len() < MAX_TRIANGLES;
            let max_edge = if finite && budget_left {
                limit(&corners)
            } else {
                None
            };
            let len_sq = |k: usize| {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)
            };
            let lens = [len_sq(0), len_sq(1), len_sq(2)];
            let k = (0..3)
                .max_by(|&x, &y| lens[x].total_cmp(&lens[y]))
                .unwrap_or(0);
            match max_edge {
                Some(m) if m.is_finite() && m > 0.0 && lens[k] > m * m => {}
                _ => {
                    out.extend_from_slice(&tri);
                    continue;
                }
            }
            // Halve the longest edge, keeping the triangle's winding.
            let (i, j, o) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            let key = (i.min(j), i.max(j));
            let m = match midpoints.get(&key) {
                Some(&m) => m,
                None => {
                    let m = lerp_vertex(mesh, i, j, 0.5);
                    midpoints.insert(key, m);
                    m
                }
            };
            pending.push([m, j, o]);
            pending.push([i, m, o]);
        }
    }
    mesh.indices = out;
}

/// The nearest and farthest distances from `c` to the triangle `tri`.
fn distance_range(c: [f32; 2], tri: &[[f32; 2]; 3]) -> (f32, f32) {
    let dist = |p: [f32; 2]| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
    let far = tri.iter().map(|&p| dist(p)).fold(0.0, f32::max);
    let side =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let s = [
        side(tri[0], tri[1]),
        side(tri[1], tri[2]),
        side(tri[2], tri[0]),
    ];
    if s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0) {
        return (0.0, far);
    }
    let to_edge = |a: [f32; 2], b: [f32; 2]| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len_sq = (d[0] * d[0] + d[1] * d[1]).max(f32::MIN_POSITIVE);
        let t = (((c[0] - a[0]) * d[0] + (c[1] - a[1]) * d[1]) / len_sq).clamp(0.0, 1.0);
        dist([a[0] + d[0] * t, a[1] + d[1] * t])
    };
    let near = to_edge(tri[0], tri[1])
        .min(to_edge(tri[1], tri[2]))
        .min(to_edge(tri[2], tri[0]));
    (near, far)
}

/// A colour field over window space, sampled by [`tint`] at each vertex.
pub(crate) trait GradientField {
    /// The field's offset at `p`, on the scale its stops use.
    fn offset_at(&self, p: [f32; 2]) -> f32;
    /// The colour stops, sorted by offset.
    fn stops(&self) -> &[GradientStop];
    /// The colour at `p`.
    fn color_at(&self, p: [f32; 2]) -> [f32; 4];
}

impl GradientField for LinearGradient<'_> {
    fn offset_at(&self, p: [f32; 2]) -> f32 {
        LinearGradient::offset_at(self, p)
    }
    fn stops(&self) -> &[GradientStop] {
        self.stops
    }
    fn color_at(&self, p: [f32; 2]) -> [f32; 4] {
        LinearGradient::color_at(self, p)
    }
}

impl GradientField for RadialGradient<'_> {
    fn offset_at(&self, p: [f32; 2]) -> f32 {
        RadialGradient::offset_at(self, p)
    }
    fn stops(&self) -> &[GradientStop] {
        self.stops
    }
    fn color_at(&self, p: [f32; 2]) -> [f32; 4] {
        RadialGradient::color_at(self, p)
    }
}

/// Recolours every vertex of `mesh`: rgb from `gradient` at the vertex
/// position, alpha the gradient's alpha multiplied by the vertex's existing
/// alpha (the anti-aliasing coverage). A vertex on a hard edge takes the stop
/// on its triangle's side: the first stop at that offset before it, the last
/// one past it.
pub(crate) fn tint<G: GradientField + ?Sized>(mesh: &mut egui::epaint::Mesh, gradient: &G) {
    let mut colors: Vec<[f32; 4]> = mesh
        .vertices
        .iter()
        .map(|v| gradient.color_at([v.pos.x, v.pos.y]))
        .collect();
    for tri in mesh.indices.chunks_exact(3) {
        let side = side_of(mesh, gradient, [tri[0], tri[1], tri[2]]);
        for &i in tri {
            let offset = gradient.offset_at(pos(mesh, i));
            let mut on_edge = gradient
                .stops()
                .iter()
                .filter(|s| (s.offset - offset).abs() <= STOP_EPSILON);
            let (Some(first), Some(last)) = (on_edge.next(), on_edge.next_back()) else {
                continue;
            };
            colors[i as usize] = if side > first.offset {
                last.color
            } else {
                first.color
            };
        }
    }
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    for (v, c) in mesh.vertices.iter_mut().zip(colors) {
        let coverage = v.color.a() as f32 / 255.0;
        v.color = egui::Color32::from_rgba_unmultiplied(
            byte(c[0]),
            byte(c[1]),
            byte(c[2]),
            byte(c[3] * coverage),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{Mesh, Vertex};
    use egui::{pos2, Color32, Pos2};
    use khora_core::ui::editor::GradientStop;

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

    const FOUR: [GradientStop; 4] = [
        GradientStop {
            offset: 0.0,
            color: RED,
        },
        GradientStop {
            offset: 0.3,
            color: GREEN,
        },
        GradientStop {
            offset: 0.7,
            color: BLUE,
        },
        GradientStop {
            offset: 1.0,
            color: RED,
        },
    ];

    /// An axis along x: offset 0 at x = 0, offset 1 at x = 100.
    fn along_x(stops: &[GradientStop]) -> LinearGradient<'_> {
        LinearGradient {
            from: [0.0, 0.0],
            to: [100.0, 0.0],
            stops,
        }
    }

    fn offset_of(p: Pos2) -> f32 {
        p.x / 100.0
    }

    /// The colour every vertex of [`quad`] carries: red rises with x, so a
    /// vertex inserted on an edge can be checked against its position.
    fn red_ramp(x: f32) -> Color32 {
        Color32::from_rgba_premultiplied((255.0 * x / 100.0).round() as u8, 0, 0, 255)
    }

    fn vertex(x: f32, y: f32, color: Color32) -> Vertex {
        Vertex {
            pos: pos2(x, y),
            uv: pos2(x / 100.0, y / 10.0),
            color,
        }
    }

    /// A 100 × 10 quad of two triangles spanning offsets 0..1, uv and colour
    /// linear in position.
    fn quad() -> Mesh {
        let mut mesh = Mesh::default();
        for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0)] {
            mesh.vertices.push(vertex(x, y, red_ramp(x)));
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        mesh
    }

    /// The same quad, opaque white, wrapped in a one-unit ring of fully
    /// transparent vertices — the shape egui's feathering produces.
    fn feathered_quad() -> Mesh {
        let mut mesh = Mesh::default();
        let inner = [(0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0)];
        let outer = [(-1.0, -1.0), (101.0, -1.0), (101.0, 11.0), (-1.0, 11.0)];
        for (x, y) in inner {
            mesh.vertices.push(vertex(x, y, Color32::WHITE));
        }
        for (x, y) in outer {
            mesh.vertices.push(vertex(x, y, Color32::TRANSPARENT));
        }
        let mut indices = vec![0, 1, 2, 0, 2, 3];
        for i in 0..4u32 {
            let j = (i + 1) % 4;
            indices.extend_from_slice(&[i, j, 4 + j, i, 4 + j, 4 + i]);
        }
        mesh.indices = indices;
        mesh
    }

    fn triangle_area(mesh: &Mesh, tri: &[u32]) -> f32 {
        let a = mesh.vertices[tri[0] as usize].pos;
        let b = mesh.vertices[tri[1] as usize].pos;
        let c = mesh.vertices[tri[2] as usize].pos;
        ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() * 0.5
    }

    fn total_area(mesh: &Mesh) -> f32 {
        mesh.indices
            .chunks(3)
            .map(|tri| triangle_area(mesh, tri))
            .sum()
    }

    fn assert_well_formed(mesh: &Mesh) {
        assert_eq!(mesh.indices.len() % 3, 0, "indices form whole triangles");
        for &i in &mesh.indices {
            assert!(
                (i as usize) < mesh.vertices.len(),
                "index {i} out of {} vertices",
                mesh.vertices.len()
            );
        }
    }

    /// No triangle has a vertex clearly before an interior stop and another
    /// clearly after it.
    fn assert_no_triangle_straddles(mesh: &Mesh, stops: &[GradientStop]) {
        let first = stops[0].offset;
        let last = stops[stops.len() - 1].offset;
        for stop in stops.iter().filter(|s| s.offset > first && s.offset < last) {
            let s = stop.offset;
            for tri in mesh.indices.chunks(3) {
                let offsets: Vec<f32> = tri
                    .iter()
                    .map(|&i| offset_of(mesh.vertices[i as usize].pos))
                    .collect();
                let before = offsets.iter().any(|&o| o < s - 1e-4);
                let after = offsets.iter().any(|&o| o > s + 1e-4);
                assert!(
                    !(before && after),
                    "triangle {tri:?} at offsets {offsets:?} straddles the stop at {s}"
                );
            }
        }
    }

    fn assert_area_unchanged(before: f32, after: f32) {
        assert!(
            ((after - before) / before).abs() < 1e-3,
            "area changed: {before} → {after}"
        );
    }

    fn assert_channel(actual: u8, expected: u8, what: &str) {
        assert!(
            (actual as i32 - expected as i32).abs() <= 2,
            "{what}: got {actual}, expected {expected}"
        );
    }

    fn assert_color32(actual: Color32, expected: Color32, at: Pos2) {
        assert_channel(actual.r(), expected.r(), &format!("r at {at:?}"));
        assert_channel(actual.g(), expected.g(), &format!("g at {at:?}"));
        assert_channel(actual.b(), expected.b(), &format!("b at {at:?}"));
        assert_channel(actual.a(), expected.a(), &format!("a at {at:?}"));
    }

    fn unmultiplied(c: [f32; 4], coverage: u8) -> Color32 {
        let to_u8 = |v: f32| (v * 255.0).round() as u8;
        let alpha = c[3] * coverage as f32 / 255.0;
        Color32::from_rgba_unmultiplied(to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), to_u8(alpha))
    }

    #[test]
    fn split_leaves_no_triangle_across_the_middle_stop() {
        let mut mesh = quad();
        split_at_stops(&mut mesh, &along_x(&THREE));
        assert_well_formed(&mesh);
        assert_no_triangle_straddles(&mesh, &THREE);
    }

    #[test]
    fn split_leaves_no_triangle_across_any_interior_stop() {
        let mut mesh = quad();
        split_at_stops(&mut mesh, &along_x(&FOUR));
        assert_well_formed(&mesh);
        assert_no_triangle_straddles(&mesh, &FOUR);
    }

    #[test]
    fn split_actually_adds_vertices_on_the_stop_line() {
        let mut mesh = quad();
        split_at_stops(&mut mesh, &along_x(&THREE));
        assert!(
            mesh.vertices
                .iter()
                .any(|v| (offset_of(v.pos) - 0.5).abs() < 1e-4),
            "no vertex lies on the stop at 0.5"
        );
    }

    #[test]
    fn split_keeps_the_covered_area() {
        let mut mesh = quad();
        let before = total_area(&mesh);
        split_at_stops(&mut mesh, &along_x(&FOUR));
        assert_area_unchanged(before, total_area(&mesh));
    }

    #[test]
    fn split_of_a_feathered_mesh_keeps_area_and_crosses_no_stop() {
        let mut mesh = feathered_quad();
        let before = total_area(&mesh);
        split_at_stops(&mut mesh, &along_x(&THREE));
        assert_well_formed(&mesh);
        assert_no_triangle_straddles(&mesh, &THREE);
        assert_area_unchanged(before, total_area(&mesh));
    }

    #[test]
    fn split_interpolates_uv_and_colour_of_new_vertices() {
        let mut mesh = quad();
        let original = mesh.vertices.len();
        split_at_stops(&mut mesh, &along_x(&FOUR));
        assert!(mesh.vertices.len() > original, "the quad was not split");
        // uv and the red ramp are linear in position on the quad, so a vertex
        // interpolated along any edge still matches its own position.
        for v in &mesh.vertices {
            assert!(
                (v.uv.x - v.pos.x / 100.0).abs() < 1e-4 && (v.uv.y - v.pos.y / 10.0).abs() < 1e-4,
                "uv {:?} does not match position {:?}",
                v.uv,
                v.pos
            );
            assert_color32(v.color, red_ramp(v.pos.x), v.pos);
        }
    }

    #[test]
    fn split_without_interior_stops_leaves_the_mesh_alone() {
        let stops = [
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 1.0,
                color: BLUE,
            },
        ];
        let mut mesh = quad();
        let before = mesh.clone();
        split_at_stops(&mut mesh, &along_x(&stops));
        assert_eq!(mesh.indices, before.indices);
        assert_eq!(mesh.vertices, before.vertices);
    }

    #[test]
    fn tint_takes_rgb_from_the_gradient_and_keeps_coverage() {
        let mut mesh = Mesh::default();
        // Fully covered, half covered, and feathered-out vertices along the
        // axis, including one past each end.
        let samples = [
            (-1.0, 0u8),
            (0.0, 255u8),
            (25.0, 128u8),
            (50.0, 255u8),
            (75.0, 64u8),
            (100.0, 255u8),
            (101.0, 0u8),
        ];
        for (x, coverage) in samples {
            mesh.vertices.push(vertex(
                x,
                5.0,
                Color32::from_rgba_premultiplied(coverage, coverage, coverage, coverage),
            ));
        }
        mesh.indices = vec![0, 1, 2, 2, 3, 4, 4, 5, 6];

        tint(&mut mesh, &along_x(&THREE));

        let expected_rgb = [
            RED,
            RED,
            [0.5, 0.5, 0.0, 1.0],
            GREEN,
            [0.0, 0.5, 0.5, 1.0],
            BLUE,
            BLUE,
        ];
        assert_eq!(mesh.vertices.len(), samples.len(), "tint adds no vertex");
        for ((v, (_, coverage)), rgb) in mesh.vertices.iter().zip(samples).zip(expected_rgb) {
            assert_color32(v.color, unmultiplied(rgb, coverage), v.pos);
        }
    }

    #[test]
    fn tint_multiplies_a_translucent_stop_by_the_coverage() {
        let half = [
            GradientStop {
                offset: 0.0,
                color: [1.0, 1.0, 1.0, 0.5],
            },
            GradientStop {
                offset: 1.0,
                color: [1.0, 1.0, 1.0, 0.5],
            },
        ];
        let mut mesh = Mesh::default();
        mesh.vertices.push(vertex(10.0, 0.0, Color32::WHITE));
        mesh.vertices
            .push(vertex(50.0, 0.0, Color32::from_white_alpha(128)));
        mesh.vertices.push(vertex(90.0, 0.0, Color32::TRANSPARENT));
        mesh.indices = vec![0, 1, 2];

        tint(&mut mesh, &along_x(&half));

        assert_channel(mesh.vertices[0].color.a(), 128, "full coverage × 0.5");
        assert_channel(mesh.vertices[1].color.a(), 64, "half coverage × 0.5");
        assert_eq!(mesh.vertices[2].color.a(), 0, "no coverage stays invisible");
    }

    /// The colour the mesh shows at `p`: the vertex colours of a triangle
    /// covering `p`, interpolated barycentrically — what the rasteriser draws.
    fn shade_at(mesh: &Mesh, p: Pos2) -> Color32 {
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[tri[k] as usize]);
            let det = (b.pos.x - a.pos.x) * (c.pos.y - a.pos.y)
                - (c.pos.x - a.pos.x) * (b.pos.y - a.pos.y);
            if det.abs() < 1e-9 {
                continue;
            }
            let wb = ((p.x - a.pos.x) * (c.pos.y - a.pos.y)
                - (c.pos.x - a.pos.x) * (p.y - a.pos.y))
                / det;
            let wc = ((b.pos.x - a.pos.x) * (p.y - a.pos.y)
                - (p.x - a.pos.x) * (b.pos.y - a.pos.y))
                / det;
            let wa = 1.0 - wb - wc;
            if wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5 {
                let (ca, cb, cc) = (a.color.to_array(), b.color.to_array(), c.color.to_array());
                let ch = |k: usize| {
                    (ca[k] as f32 * wa + cb[k] as f32 * wb + cc[k] as f32 * wc).round() as u8
                };
                return Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), ch(3));
            }
        }
        panic!("no triangle covers {p:?}");
    }

    #[test]
    fn filled_quad_holds_the_end_colours_where_the_stops_stop_short_of_the_shape() {
        let stops = [
            GradientStop {
                offset: 0.2,
                color: RED,
            },
            GradientStop {
                offset: 0.8,
                color: GREEN,
            },
        ];
        let g = along_x(&stops);
        let mut mesh = quad();
        split_at_stops(&mut mesh, &g);
        tint(&mut mesh, &g);
        for x in [5.0, 10.0, 50.0, 90.0, 95.0] {
            let p = pos2(x, 5.0);
            assert_color32(
                shade_at(&mesh, p),
                unmultiplied(g.color_at([x, 5.0]), 255),
                p,
            );
        }
    }

    #[test]
    fn filled_quad_keeps_a_hard_edge_where_two_stops_share_an_offset() {
        let stops = [
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: BLUE,
            },
            GradientStop {
                offset: 1.0,
                color: BLUE,
            },
        ];
        let g = along_x(&stops);
        let mut mesh = quad();
        split_at_stops(&mut mesh, &g);
        tint(&mut mesh, &g);
        for x in [25.0, 45.0, 55.0, 75.0] {
            let p = pos2(x, 5.0);
            assert_color32(
                shade_at(&mesh, p),
                unmultiplied(g.color_at([x, 5.0]), 255),
                p,
            );
        }
    }

    #[test]
    fn tint_keeps_triangles_and_positions() {
        let mut mesh = feathered_quad();
        let before = mesh.clone();
        tint(&mut mesh, &along_x(&THREE));
        assert_eq!(mesh.indices, before.indices);
        let positions: Vec<Pos2> = mesh.vertices.iter().map(|v| v.pos).collect();
        let original: Vec<Pos2> = before.vertices.iter().map(|v| v.pos).collect();
        assert_eq!(positions, original);
        // The opaque inner corners take the gradient's ends; the ring stays
        // fully transparent.
        assert_color32(
            mesh.vertices[0].color,
            unmultiplied(RED, 255),
            mesh.vertices[0].pos,
        );
        assert_color32(
            mesh.vertices[1].color,
            unmultiplied(BLUE, 255),
            mesh.vertices[1].pos,
        );
        for v in &mesh.vertices[4..] {
            assert_eq!(
                v.color.a(),
                0,
                "feathered vertex at {:?} gained coverage",
                v.pos
            );
        }
    }

    fn edge_lengths(mesh: &Mesh, tri: &[u32]) -> [f32; 3] {
        let p = [0, 1, 2].map(|k| mesh.vertices[tri[k] as usize].pos);
        [0, 1, 2].map(|k| (p[(k + 1) % 3] - p[k]).length())
    }

    fn assert_no_edge_longer_than(mesh: &Mesh, max_edge: f32) {
        for tri in mesh.indices.chunks(3) {
            for len in edge_lengths(mesh, tri) {
                assert!(
                    len <= max_edge + 1e-3,
                    "triangle {tri:?} has an edge of {len}, longer than {max_edge}"
                );
            }
        }
    }

    /// A 128 × 8 quad of two triangles whose red channel equals x and whose
    /// uv is linear in position. Midpoint splits only ever land on even x, so
    /// an interpolated vertex's colour is exact.
    fn ramp_quad() -> Mesh {
        let mut mesh = Mesh::default();
        for (x, y) in [(0.0, 0.0), (128.0, 0.0), (128.0, 8.0), (0.0, 8.0)] {
            mesh.vertices.push(Vertex {
                pos: pos2(x, y),
                uv: pos2(x / 128.0, y / 8.0),
                color: Color32::from_rgba_premultiplied(x as u8, 0, 0, 255),
            });
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        mesh
    }

    #[test]
    fn subdivide_leaves_no_edge_longer_than_max_edge() {
        let mut mesh = quad();
        subdivide(&mut mesh, 7.0);
        assert_well_formed(&mesh);
        assert!(mesh.vertices.len() > 4, "the quad was not subdivided");
        assert_no_edge_longer_than(&mesh, 7.0);
    }

    #[test]
    fn subdivide_keeps_the_covered_area() {
        let mut mesh = quad();
        let before = total_area(&mesh);
        subdivide(&mut mesh, 3.0);
        assert_area_unchanged(before, total_area(&mesh));
    }

    #[test]
    fn subdivide_of_a_feathered_mesh_keeps_area_and_bounds_every_edge() {
        let mut mesh = feathered_quad();
        let before = total_area(&mesh);
        subdivide(&mut mesh, 4.0);
        assert_well_formed(&mesh);
        assert_no_edge_longer_than(&mesh, 4.0);
        assert_area_unchanged(before, total_area(&mesh));
    }

    #[test]
    fn subdivide_interpolates_uv_and_colour_of_new_vertices() {
        let mut mesh = ramp_quad();
        subdivide(&mut mesh, 8.5);
        assert!(mesh.vertices.len() > 4, "the quad was not subdivided");
        // uv and the red channel are linear in position on the quad, so a
        // vertex interpolated along any edge still matches its own position.
        for v in &mesh.vertices {
            assert!(
                (v.uv.x - v.pos.x / 128.0).abs() < 1e-4 && (v.uv.y - v.pos.y / 8.0).abs() < 1e-4,
                "uv {:?} does not match position {:?}",
                v.uv,
                v.pos
            );
            let expected = Color32::from_rgba_premultiplied(v.pos.x.round() as u8, 0, 0, 255);
            assert_color32(v.color, expected, v.pos);
        }
    }

    #[test]
    fn subdivide_with_a_max_edge_that_is_not_a_positive_number_leaves_the_mesh_alone() {
        for max_edge in [0.0, -3.0, f32::NEG_INFINITY, f32::NAN] {
            let mut mesh = quad();
            let before = mesh.clone();
            subdivide(&mut mesh, max_edge);
            assert_eq!(mesh.indices, before.indices, "max_edge = {max_edge}");
            assert_eq!(mesh.vertices, before.vertices, "max_edge = {max_edge}");
        }
    }

    #[test]
    fn subdivide_of_a_mesh_already_finer_than_max_edge_leaves_it_alone() {
        // The quad's longest edge is its diagonal, about 100.5 units.
        for max_edge in [150.0, f32::INFINITY] {
            let mut mesh = quad();
            let before = mesh.clone();
            subdivide(&mut mesh, max_edge);
            assert_eq!(mesh.indices, before.indices, "max_edge = {max_edge}");
            assert_eq!(mesh.vertices, before.vertices, "max_edge = {max_edge}");
        }
    }

    /// Runs `body` in a child test process (the test named `test` re-run with
    /// `KHORA_TEST_CHILD` set) and reports whether it finished within
    /// `limit` — a runaway loop is killed instead of hanging the run.
    fn finishes_within(test: &str, limit: std::time::Duration, body: impl FnOnce()) -> bool {
        if std::env::var_os("KHORA_TEST_CHILD").is_some() {
            body();
            return true;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([test, "--exact", "--test-threads=1"])
            .env("KHORA_TEST_CHILD", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while start.elapsed() < limit {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        false
    }

    #[test]
    fn subdivide_of_a_mesh_with_a_non_finite_vertex_terminates() {
        // A polygon point that is NaN or infinite: no edge through it ever
        // gets shorter, so halving it must not go on forever.
        let test = module_path!()
            .split_once("::")
            .map_or(String::new(), |(_, path)| path.to_owned())
            + "::subdivide_of_a_mesh_with_a_non_finite_vertex_terminates";
        let finished = finishes_within(&test, std::time::Duration::from_secs(2), || {
            for bad in [f32::NAN, f32::INFINITY] {
                let mut mesh = quad();
                mesh.vertices[2].pos = pos2(bad, 10.0);
                subdivide(&mut mesh, 7.0);
            }
        });
        assert!(
            finished,
            "subdivide did not return within 2 s on a mesh with a NaN or infinite vertex"
        );
    }

    /// A circle of radius 20 around `[50, 50]`.
    fn disc(stops: &[GradientStop]) -> RadialGradient<'_> {
        RadialGradient {
            center: [50.0, 50.0],
            radius: 20.0,
            stops,
        }
    }

    /// The point `distance` from [`disc`]'s centre in direction `angle`.
    fn around(distance: f32, angle: f32) -> Pos2 {
        pos2(50.0 + distance * angle.cos(), 50.0 + distance * angle.sin())
    }

    #[test]
    fn tint_colours_a_radial_fan_by_distance_and_keeps_coverage() {
        // A centre vertex, an opaque ring halfway out, a half-covered ring on
        // the circle and a feathered-out ring past it.
        let rings = [(10.0, 255u8), (20.0, 128u8), (30.0, 0u8)];
        let mut mesh = Mesh::default();
        mesh.vertices.push(vertex(50.0, 50.0, Color32::WHITE));
        for (distance, coverage) in rings {
            for k in 0..6 {
                let p = around(distance, k as f32 * std::f32::consts::FRAC_PI_3 + 0.2);
                mesh.vertices.push(vertex(
                    p.x,
                    p.y,
                    Color32::from_rgba_premultiplied(coverage, coverage, coverage, coverage),
                ));
            }
        }
        let ring = |r: u32, k: u32| 1 + 6 * r + k % 6;
        let mut indices = Vec::new();
        for k in 0..6 {
            indices.extend_from_slice(&[0, ring(0, k), ring(0, k + 1)]);
        }
        for r in 0..2 {
            for k in 0..6 {
                let (a, b) = (ring(r, k), ring(r, k + 1));
                let (c, d) = (ring(r + 1, k + 1), ring(r + 1, k));
                indices.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
        mesh.indices = indices;

        tint(&mut mesh, &disc(&THREE));

        assert_eq!(mesh.vertices.len(), 19, "tint adds no vertex");
        assert_color32(
            mesh.vertices[0].color,
            unmultiplied(RED, 255),
            mesh.vertices[0].pos,
        );
        for v in &mesh.vertices[1..7] {
            assert_color32(v.color, unmultiplied(GREEN, 255), v.pos);
        }
        for v in &mesh.vertices[7..13] {
            assert_color32(v.color, unmultiplied(BLUE, 128), v.pos);
        }
        for v in &mesh.vertices[13..] {
            assert_eq!(
                v.color.a(),
                0,
                "feathered vertex at {:?} gained coverage",
                v.pos
            );
        }
    }

    #[test]
    fn subdivided_and_tinted_square_keeps_a_radial_hard_edge() {
        // A solid red disc inside a solid blue one: two stops share the
        // offset 0.5, so the colour jumps on the circle of radius 160.
        let stops = [
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: RED,
            },
            GradientStop {
                offset: 0.5,
                color: BLUE,
            },
            GradientStop {
                offset: 1.0,
                color: BLUE,
            },
        ];
        let g = RadialGradient {
            center: [400.0, 400.0],
            radius: 320.0,
            stops: &stops,
        };
        let mut mesh = Mesh::default();
        for (x, y) in [(80.0, 80.0), (720.0, 80.0), (720.0, 720.0), (80.0, 720.0)] {
            mesh.vertices.push(vertex(x, y, Color32::WHITE));
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];

        subdivide_radial(&mut mesh, &g);
        tint(&mut mesh, &g);

        // Six units either side of the edge, in several directions.
        for distance in [154.0, 166.0] {
            for angle in [0.4_f32, 1.9, 3.3, 5.0] {
                let p = pos2(
                    400.0 + distance * angle.cos(),
                    400.0 + distance * angle.sin(),
                );
                let got = shade_at(&mesh, p);
                let want = unmultiplied(g.color_at([p.x, p.y]), 255);
                for (k, (a, b)) in got.to_array().iter().zip(want.to_array()).enumerate() {
                    assert!(
                        (*a as i32 - b as i32).abs() <= 3,
                        "channel {k} at {p:?}, {distance} from the centre: got {got:?}, expected {want:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn subdivided_and_tinted_square_follows_the_radial_field() {
        let stops = [
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 1.0,
                color: BLUE,
            },
        ];
        let g = disc(&stops);
        // The square circumscribing the circle: two triangles whose vertex
        // colours alone would blend straight across the centre.
        let mut mesh = Mesh::default();
        for (x, y) in [(30.0, 30.0), (70.0, 30.0), (70.0, 70.0), (30.0, 70.0)] {
            mesh.vertices.push(vertex(x, y, Color32::WHITE));
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];

        subdivide_radial(&mut mesh, &g);
        tint(&mut mesh, &g);

        // Samples clear of the cone's tip and of the circle, where linear
        // interpolation over 1.25-unit triangles stays within a few levels.
        for distance in [8.0, 14.0] {
            for angle in [0.4, 1.9, 3.3, 5.0] {
                let p = around(distance, angle);
                let got = shade_at(&mesh, p);
                let want = unmultiplied(g.color_at([p.x, p.y]), 255);
                for (k, (a, b)) in got.to_array().iter().zip(want.to_array()).enumerate() {
                    assert!(
                        (*a as i32 - b as i32).abs() <= 3,
                        "channel {k} at {p:?}: got {got:?}, expected {want:?}"
                    );
                }
            }
        }
    }
}
