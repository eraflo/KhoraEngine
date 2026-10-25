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

//! GLTF mesh decoder with support for both embedded and external resources.
//!
//! A glTF file decoded as a [`Mesh`] is its default scene as one mesh: every
//! node's primitives, placed by the node hierarchy, merged into one vertex
//! and index list.

use super::gltf_checks::{check_primitive, MAX_DECODED_VERTICES};
use super::gltf_resources::load_buffers;
use super::GltfResourceResolver;
use anyhow::Result;
use gltf::{
    mesh::{Mode, Semantic},
    Buffer,
};
use khora_core::math::geometry::Aabb;
use khora_core::math::{Mat3, Mat4, Vec2, Vec3, Vec4};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use std::{error::Error, path::Path, sync::Arc};

use crate::asset::AssetDecoder;

/// Decoder for GLTF meshes, configured with a resource resolver.
#[derive(Clone)]
pub struct GltfDecoder {
    resolver: Arc<dyn GltfResourceResolver>,
}

impl GltfDecoder {
    /// Creates a new GLTF decoder with the given resource resolver.
    pub fn new(resolver: Arc<dyn GltfResourceResolver>) -> Self {
        Self { resolver }
    }
}

impl AssetDecoder<Mesh> for GltfDecoder {
    /// Decodes a glTF whose external files are named from the assets root.
    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        self.load_at(bytes, Path::new(""))
    }

    /// Decodes the glTF at `source_path`: its external files are read
    /// relative to its own folder, as glTF specifies.
    fn load_at(
        &self,
        bytes: &[u8],
        source_path: &Path,
    ) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        let gltf = parse_gltf(bytes).map_err(|e| format!("Failed to parse GLTF file: {}", e))?;

        let base_dir = source_path.parent().unwrap_or(Path::new(""));
        let buffer_data = load_buffers(&gltf, &*self.resolver, base_dir)
            .map_err(|e| format!("Failed to load GLTF buffer data: {}", e))?;

        let placed = placed_meshes(&gltf.document)?;
        let mut primitives = Vec::new();
        let mut vertices = 0usize;
        for (mesh, world) in &placed {
            for primitive in mesh.primitives() {
                // An unwelded primitive (flat normals) has one vertex per index.
                let count =
                    |accessor: Option<gltf::Accessor<'_>>| accessor.map_or(0, |a| a.count());
                vertices = vertices.saturating_add(
                    count(primitive.get(&Semantic::Positions)).max(count(primitive.indices())),
                );
                if vertices > MAX_DECODED_VERTICES {
                    return Err(format!(
                        "glTF scene flattens to more than {MAX_DECODED_VERTICES} vertices"
                    )
                    .into());
                }
                primitives.push(read_primitive(&primitive, world, &buffer_data)?);
            }
        }

        let triangles = primitives
            .iter()
            .filter(|p| p.topology == PrimitiveTopology::TriangleList)
            .count();
        let kept: Vec<Primitive> = if triangles > 0 {
            let skipped = primitives.len() - triangles;
            if skipped > 0 {
                log::warn!(
                    "glTF: {skipped} line/point primitive(s) beside triangles are not \
                     part of the mesh"
                );
            }
            primitives
                .into_iter()
                .filter(|p| p.topology == PrimitiveTopology::TriangleList)
                .collect()
        } else {
            primitives.into_iter().take(1).collect()
        };

        merge(kept)
    }
}

/// Parses and validates a glTF file, binary or JSON, refusing a malformed
/// one with an error.
///
/// `gltf-json`'s validator reads a primitive's `POSITION` accessor by index
/// without a bounds check, so a file naming an accessor that does not exist
/// would panic inside it. Those indices are checked here first, and the file
/// is validated only once they hold.
pub(super) fn parse_gltf(bytes: &[u8]) -> Result<gltf::Gltf, Box<dyn Error + Send + Sync>> {
    // The GLB reader subtracts the 12-byte header from the declared length
    // unchecked.
    if let Some(header) = bytes.strip_prefix(b"glTF") {
        let declared = header
            .get(4..8)
            .map(|length| u32::from_le_bytes([length[0], length[1], length[2], length[3]]));
        if declared.is_none_or(|length| length < 12) {
            return Err("GLB header declares a length shorter than the header itself".into());
        }
    }
    let unchecked = gltf::Gltf::from_slice_without_validation(bytes)?;
    let blob = unchecked.blob;
    let root = unchecked.document.into_json();
    let positions = gltf::json::validation::Checked::Valid(gltf::json::mesh::Semantic::Positions);
    for (m, mesh) in root.meshes.iter().enumerate() {
        for (p, primitive) in mesh.primitives.iter().enumerate() {
            if let Some(accessor) = primitive.attributes.get(&positions) {
                if accessor.value() >= root.accessors.len() {
                    return Err(format!(
                        "glTF mesh {m} primitive {p}: POSITION names accessor {}, but the file \
                         has {}",
                        accessor.value(),
                        root.accessors.len()
                    )
                    .into());
                }
            }
        }
    }
    let document = gltf::Document::from_json(root)?;
    Ok(gltf::Gltf { document, blob })
}

/// Every mesh the file shows, with the world transform of the node showing
/// it: the default scene (else the first one), walked depth first; a file
/// without any scene shows each of its meshes once, untransformed.
///
/// glTF requires the node hierarchy to be disjoint trees; a node reached
/// twice (a cycle, or a node with two parents) fails the decode. The walk is
/// iterative, so a deep hierarchy cannot overflow the stack.
fn placed_meshes(
    document: &gltf::Document,
) -> Result<Vec<(gltf::Mesh<'_>, Mat4)>, Box<dyn Error + Send + Sync>> {
    let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    else {
        return Ok(document
            .meshes()
            .map(|mesh| (mesh, Mat4::IDENTITY))
            .collect());
    };

    let mut placed = Vec::new();
    let mut reached = vec![false; document.nodes().len()];
    // Children are pushed last-first, so they pop in file order.
    let mut pending: Vec<(gltf::Node<'_>, Mat4)> =
        scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    pending.reverse();
    while let Some((node, parent)) = pending.pop() {
        let seen = reached
            .get_mut(node.index())
            .ok_or("glTF node index out of range")?;
        if std::mem::replace(seen, true) {
            return Err(format!(
                "glTF node {} is reached twice: the node hierarchy is not a tree",
                node.index()
            )
            .into());
        }
        let world = parent * local_matrix(&node);
        if let Some(mesh) = node.mesh() {
            placed.push((mesh, world));
        }
        let first_child = pending.len();
        pending.extend(node.children().map(|child| (child, world)));
        pending[first_child..].reverse();
    }
    Ok(placed)
}

/// The node's local transform (TRS or matrix), column-major.
fn local_matrix(node: &gltf::Node<'_>) -> Mat4 {
    let [c0, c1, c2, c3] = node.transform().matrix();
    Mat4::from_cols(
        Vec4::new(c0[0], c0[1], c0[2], c0[3]),
        Vec4::new(c1[0], c1[1], c1[2], c1[3]),
        Vec4::new(c2[0], c2[1], c2[2], c2[3]),
        Vec4::new(c3[0], c3[1], c3[2], c3[3]),
    )
}

/// The matrix that carries normals through `world`: the cofactor matrix of
/// its 3×3 part, i.e. det · M⁻ᵀ, signed so a mirror keeps normals pointing
/// out. Unlike the inverse it needs no tolerance on the determinant, so a
/// centimetre-scaled node still rotates its normals.
fn normal_matrix(world: &Mat4) -> Mat3 {
    let [a, b, c] = [
        world.cols[0].truncate(),
        world.cols[1].truncate(),
        world.cols[2].truncate(),
    ];
    let sign = if a.dot(b.cross(c)) < 0.0 { -1.0 } else { 1.0 };
    Mat3::from_cols(b.cross(c) * sign, c.cross(a) * sign, a.cross(b) * sign)
}

/// `v` scaled to unit length, or `fallback` when it has none. No tolerance:
/// a normal through a tiny scale is short but still a direction.
fn unit_or(v: Vec3, fallback: Vec3) -> Vec3 {
    let length = v.length();
    if length > 0.0 && length.is_finite() {
        v * (1.0 / length)
    } else {
        fallback
    }
}

/// One glTF primitive read into the engine's frame: transformed, indexed,
/// as a list topology, with normals.
pub(super) struct Primitive {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    tex_coords: Option<Vec<Vec2>>,
    colors: Option<Vec<Vec4>>,
    indices: Vec<u32>,
    pub(super) topology: PrimitiveTopology,
}

pub(super) fn read_primitive(
    primitive: &gltf::Primitive<'_>,
    world: &Mat4,
    buffer_data: &[Vec<u8>],
) -> Result<Primitive, Box<dyn Error + Send + Sync>> {
    check_primitive(primitive, buffer_data)?;
    let reader =
        primitive.reader(|buffer: Buffer<'_>| buffer_data.get(buffer.index()).map(Vec::as_slice));
    // Every accessor was checked to lie inside its buffer, so an attribute
    // the primitive names but the reader cannot read is a file the checks
    // missed: refused, never read as absent.
    let unread = |name: &str| format!("glTF {name} accessor could not be read");

    let positions: Vec<Vec3> = reader
        .read_positions()
        .ok_or("Vertex positions attribute not found")?
        .map(|[x, y, z]| world.transform_point(Vec3::new(x, y, z)))
        .collect();
    let count = positions.len() as u32;
    let normal_matrix = normal_matrix(world);
    let normals: Option<Vec<Vec3>> = reader.read_normals().map(|iter| {
        iter.map(|[x, y, z]| unit_or(normal_matrix * Vec3::new(x, y, z), Vec3::Y))
            .collect()
    });
    if normals.is_none() && primitive.get(&Semantic::Normals).is_some() {
        return Err(unread("NORMAL").into());
    }
    let tex_coords: Option<Vec<Vec2>> = reader
        .read_tex_coords(0)
        .map(|iter| iter.into_f32().map(|[u, v]| Vec2::new(u, v)).collect());
    if tex_coords.is_none() && primitive.get(&Semantic::TexCoords(0)).is_some() {
        return Err(unread("TEXCOORD_0").into());
    }
    let colors: Option<Vec<Vec4>> = reader.read_colors(0).map(|iter| {
        iter.into_rgba_f32()
            .map(|[r, g, b, a]| Vec4::new(r, g, b, a))
            .collect()
    });
    if colors.is_none() && primitive.get(&Semantic::Colors(0)).is_some() {
        return Err(unread("COLOR_0").into());
    }
    // glTF requires every attribute of a primitive to have one value per
    // vertex; a file that breaks this would shift values onto other vertices.
    let counts = [
        ("NORMAL", normals.as_ref().map(Vec::len)),
        ("TEXCOORD_0", tex_coords.as_ref().map(Vec::len)),
        ("COLOR_0", colors.as_ref().map(Vec::len)),
    ];
    for (name, len) in counts {
        if let Some(len) = len.filter(|&len| len != positions.len()) {
            return Err(format!(
                "glTF {name} has {len} values for {} vertices",
                positions.len()
            )
            .into());
        }
    }
    let indices: Vec<u32> = match (reader.read_indices(), primitive.indices()) {
        (Some(indices), _) => indices.into_u32().collect(),
        (None, Some(_)) => return Err(unread("indices").into()),
        (None, None) => (0..count).collect(),
    };
    if let Some(&bad) = indices.iter().find(|&&i| i >= count) {
        return Err(format!("glTF index {bad} out of range for {count} vertices").into());
    }

    let (mut indices, topology) = match primitive.mode() {
        Mode::Triangles => (indices, PrimitiveTopology::TriangleList),
        Mode::TriangleStrip => (strip_to_list(&indices), PrimitiveTopology::TriangleList),
        Mode::TriangleFan => (fan_to_list(&indices), PrimitiveTopology::TriangleList),
        Mode::Lines => (indices, PrimitiveTopology::LineList),
        Mode::LineStrip => (indices, PrimitiveTopology::LineStrip),
        // Line loops are drawn as a strip closed back onto its first vertex.
        Mode::LineLoop => {
            let mut closed = indices;
            if let Some(&first) = closed.first() {
                closed.push(first);
            }
            (closed, PrimitiveTopology::LineStrip)
        }
        Mode::Points => (indices, PrimitiveTopology::PointList),
    };
    if topology == PrimitiveTopology::TriangleList {
        indices.truncate(indices.len() - indices.len() % 3);
        // A mirroring transform turns counter-clockwise faces clockwise.
        if world.determinant() < 0.0 {
            for triangle in indices.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
        }
    }

    let mut mesh = Mesh {
        positions,
        normals,
        tex_coords,
        tangents: None,
        colors,
        indices: Some(indices),
        primitive_type: topology,
        bounding_box: Aabb::INVALID,
    };
    // glTF: a primitive without normals is shaded flat.
    mesh.generate_flat_normals();
    let vertex_count = mesh.positions.len();
    Ok(Primitive {
        normals: mesh.normals.unwrap_or_else(|| vec![Vec3::Y; vertex_count]),
        positions: mesh.positions,
        tex_coords: mesh.tex_coords,
        colors: mesh.colors,
        indices: mesh.indices.unwrap_or_default(),
        topology,
    })
}

/// Triangle list of a strip: every triangle after the first alternates its
/// winding, so odd ones swap their first two corners to stay counter-clockwise.
fn strip_to_list(strip: &[u32]) -> Vec<u32> {
    strip
        .windows(3)
        .enumerate()
        .flat_map(|(i, w)| {
            if i % 2 == 0 {
                [w[0], w[1], w[2]]
            } else {
                [w[1], w[0], w[2]]
            }
        })
        .collect()
}

/// Triangle list of a fan: every triangle shares the first corner.
fn fan_to_list(fan: &[u32]) -> Vec<u32> {
    match fan.split_first() {
        Some((&hub, rim)) => rim.windows(2).flat_map(|w| [hub, w[0], w[1]]).collect(),
        None => Vec::new(),
    }
}

/// Concatenates primitives of one topology into one mesh. An attribute some
/// primitives carry and others don't is filled with its default for the
/// others: UV `(0, 0)`, colour opaque white.
pub(super) fn merge(primitives: Vec<Primitive>) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
    let topology = primitives
        .first()
        .map(|p| p.topology)
        .ok_or("No meshes found in GLTF file")?;
    let any_uv = primitives.iter().any(|p| p.tex_coords.is_some());
    let any_color = primitives.iter().any(|p| p.colors.is_some());

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tex_coords = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for primitive in primitives {
        let base = positions.len() as u32;
        let count = primitive.positions.len();
        indices.extend(primitive.indices.iter().map(|&i| base + i));
        positions.extend(primitive.positions);
        normals.extend(primitive.normals);
        if any_uv {
            tex_coords.extend(aligned(primitive.tex_coords, count, Vec2::ZERO));
        }
        if any_color {
            colors.extend(aligned(primitive.colors, count, Vec4::ONE));
        }
    }
    if positions.is_empty() {
        return Err("GLTF file has no vertex positions".into());
    }

    let bounding_box = Aabb::from_points(&positions).unwrap_or(Aabb::INVALID);
    Ok(Mesh {
        positions,
        normals: Some(normals),
        tex_coords: any_uv.then_some(tex_coords),
        tangents: None,
        colors: any_color.then_some(colors),
        indices: Some(indices),
        primitive_type: topology,
        bounding_box,
    })
}

/// Exactly `count` values: the attribute's own, then `default`.
fn aligned<T: Copy>(values: Option<Vec<T>>, count: usize, default: T) -> Vec<T> {
    let mut values = values.unwrap_or_default();
    values.resize(count, default);
    values
}
