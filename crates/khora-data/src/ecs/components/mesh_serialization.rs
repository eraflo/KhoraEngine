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

//! Serializable reference to a mesh asset.
//!
//! Procedural meshes (cube, sphere, plane) are serialized by their generation
//! parameters so they can be reconstructed on load. Imported meshes (glTF, OBJ)
//! are serialized by their `AssetUUID` for VFS lookup.

use bincode::{Decode, Encode};
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::{Aabb, Vec2, Vec3};
use khora_core::renderer::api::{
    pipeline::{PrimitiveTopology, VertexAttributeDescriptor, VertexFormat},
    scene::Mesh,
};

use crate::ecs::HandleComponent;

/// Identifies a known procedural mesh primitive.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, serde::Serialize, serde::Deserialize,
)]
pub enum ProceduralMeshKind {
    Cube,
    Sphere,
    Plane,
}

/// Serializable reference to a mesh, either as procedural parameters or an asset UUID.
#[derive(Debug, Clone, Encode, Decode, serde::Serialize, serde::Deserialize)]
pub enum SerializableMeshRef {
    /// Procedural mesh — regenerate from parameters on load.
    Procedural {
        kind: ProceduralMeshKind,
        params: [f32; 4],
    },
    /// Imported asset mesh — lookup by UUID in the VFS/asset registry.
    Asset(AssetUUID),
}

impl SerializableMeshRef {
    /// Serializes a `HandleComponent<Mesh>` into a compact reference.
    ///
    /// Detects known procedural mesh patterns by vertex/index counts and topology.
    /// Falls back to the asset UUID for custom/imported meshes.
    pub fn from_handle(comp: &HandleComponent<Mesh>) -> Self {
        let mesh: &Mesh = &comp.handle;

        // Detect procedural meshes by structural fingerprints.
        if let Some(kind) = detect_procedural_mesh(mesh) {
            let params = extract_mesh_params(&kind, mesh);
            return Self::Procedural { kind, params };
        }

        Self::Asset(comp.uuid)
    }

    /// Reconstructs a `HandleComponent<Mesh>` from this serializable reference.
    pub fn into_handle(self) -> HandleComponent<Mesh> {
        match self {
            Self::Procedural { kind, params } => {
                let mesh = reconstruct_procedural_mesh(&kind, &params);
                let uuid = AssetUUID::new();
                HandleComponent {
                    handle: AssetHandle::new(mesh),
                    uuid,
                }
            }
            Self::Asset(uuid) => {
                // For asset meshes, create a placeholder handle.
                // The actual mesh data should be loaded from the VFS/asset pipeline
                // by the asset system using this UUID.
                let mesh = Mesh {
                    positions: Vec::new(),
                    normals: None,
                    tex_coords: None,
                    tangents: None,
                    colors: None,
                    indices: None,
                    primitive_type: PrimitiveTopology::TriangleList,
                    bounding_box: Aabb::from_min_max(Vec3::ZERO, Vec3::ZERO),
                    vertex_layout: default_vertex_layout(),
                };
                let handle = AssetHandle::new(mesh);
                HandleComponent { handle, uuid }
            }
        }
    }
}

/// Detects if a mesh matches a known procedural primitive pattern.
fn detect_procedural_mesh(mesh: &Mesh) -> Option<ProceduralMeshKind> {
    let vert_count = mesh.positions.len();
    let index_count = mesh.indices.as_ref().map_or(0, |i| i.len());
    let is_tri_list = mesh.primitive_type == PrimitiveTopology::TriangleList;

    if !is_tri_list {
        return None;
    }

    // Cube: 24 vertices (4 per face × 6 faces), 36 indices (6 triangles × 6 faces)
    if vert_count == 24 && index_count == 36 {
        return Some(ProceduralMeshKind::Cube);
    }

    // Plane: 4 vertices, 6 indices (2 triangles)
    if vert_count == 4 && index_count == 6 {
        return Some(ProceduralMeshKind::Plane);
    }

    // Sphere: variable — detected by (rings+1)*(segments+1) vertex pattern
    // Common defaults: 16×16 → 289 vertices, 32×16 → 561 vertices
    // Check if vertex count is a product of (r+1)*(s+1) with reasonable r,s
    if index_count > 0 && vert_count > 4 {
        for rings in 4..=64 {
            for segments in 4..=64 {
                let expected_verts = (rings + 1) * (segments + 1);
                let expected_indices = rings * segments * 6;
                if vert_count == expected_verts as usize && index_count == expected_indices as usize
                {
                    return Some(ProceduralMeshKind::Sphere);
                }
            }
        }
    }

    None
}

/// Extracts the generation parameters from a detected procedural mesh.
fn extract_mesh_params(kind: &ProceduralMeshKind, mesh: &Mesh) -> [f32; 4] {
    match kind {
        ProceduralMeshKind::Cube => {
            // Size is derivable from the bounding box.
            let extent = mesh.bounding_box.half_extents();
            let size = extent.x * 2.0;
            [size, 0.0, 0.0, 0.0]
        }
        ProceduralMeshKind::Plane => {
            // Size is derivable from the bounding box; Y from vertex positions.
            let extent = mesh.bounding_box.half_extents();
            let size = extent.x * 2.0;
            let y = mesh.positions.first().map_or(0.0, |p| p.y);
            [size, y, 0.0, 0.0]
        }
        ProceduralMeshKind::Sphere => {
            // Radius from bounding box; segments/rings from vertex count.
            let radius = mesh.bounding_box.half_extents().x;
            let vert_count = mesh.positions.len();
            // Solve (rings+1)*(segments+1) = vert_count for common patterns.
            // Try common segment values.
            let mut segments = 16u32;
            let mut rings = 16u32;
            for s in 4..=128 {
                if vert_count % (s as usize + 1) == 0 {
                    let r = vert_count / (s as usize + 1) - 1;
                    if r >= 4 && r <= 128 {
                        segments = s;
                        rings = r as u32;
                        break;
                    }
                }
            }
            [radius, segments as f32, rings as f32, 0.0]
        }
    }
}

/// Reconstructs a procedural `Mesh` from its kind and parameters.
fn reconstruct_procedural_mesh(kind: &ProceduralMeshKind, params: &[f32; 4]) -> Mesh {
    match kind {
        ProceduralMeshKind::Cube => create_cube(params[0]),
        ProceduralMeshKind::Plane => create_plane(params[0], params[1]),
        ProceduralMeshKind::Sphere => create_sphere(params[0], params[1] as u32, params[2] as u32),
    }
}

fn default_vertex_layout() -> Vec<VertexAttributeDescriptor> {
    vec![
        VertexAttributeDescriptor {
            shader_location: 0,
            format: VertexFormat::Float32x3,
            offset: 0,
        },
        VertexAttributeDescriptor {
            shader_location: 1,
            format: VertexFormat::Float32x3,
            offset: 12,
        },
        VertexAttributeDescriptor {
            shader_location: 2,
            format: VertexFormat::Float32x2,
            offset: 24,
        },
    ]
}

// ─── Procedural mesh generation (mirrors khora-sdk/vessel.rs) ───

fn create_plane(size: f32, y: f32) -> Mesh {
    let half = size / 2.0;
    let positions = vec![
        Vec3::new(-half, y, -half),
        Vec3::new(half, y, -half),
        Vec3::new(half, y, half),
        Vec3::new(-half, y, half),
    ];
    let normals = vec![
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let tex_coords = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    let indices = vec![0u32, 1, 2, 0, 2, 3];
    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(Vec3::new(-half, y, -half), Vec3::new(half, y, half)),
        vertex_layout: default_vertex_layout(),
    }
}

fn create_cube(size: f32) -> Mesh {
    let half = size / 2.0;
    let positions = vec![
        // Front (+Z)
        Vec3::new(-half, -half, half),
        Vec3::new(half, -half, half),
        Vec3::new(half, half, half),
        Vec3::new(-half, half, half),
        // Back (-Z)
        Vec3::new(half, -half, -half),
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, half, -half),
        Vec3::new(half, half, -half),
        // Right (+X)
        Vec3::new(half, -half, half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, half, -half),
        Vec3::new(half, half, half),
        // Left (-X)
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, -half, half),
        Vec3::new(-half, half, half),
        Vec3::new(-half, half, -half),
        // Top (+Y)
        Vec3::new(-half, half, half),
        Vec3::new(half, half, half),
        Vec3::new(half, half, -half),
        Vec3::new(-half, half, -half),
        // Bottom (-Y)
        Vec3::new(-half, -half, -half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, -half, half),
        Vec3::new(-half, -half, half),
    ];
    let normals = vec![
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
    ];
    let tex_coords: Vec<Vec2> = (0..6)
        .flat_map(|_| {
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ]
        })
        .collect();
    let indices = vec![
        0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11, 12, 13, 14, 12, 14, 15, 16, 17,
        18, 16, 18, 19, 20, 21, 22, 20, 22, 23,
    ];
    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(
            Vec3::new(-half, -half, -half),
            Vec3::new(half, half, half),
        ),
        vertex_layout: default_vertex_layout(),
    }
}

fn create_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tex_coords = Vec::new();

    for ring in 0..=rings {
        let phi = std::f32::consts::PI * (ring as f32 / rings as f32);
        let y = radius * phi.cos();
        let ring_radius = radius * phi.sin();
        for segment in 0..=segments {
            let theta = 2.0 * std::f32::consts::PI * (segment as f32 / segments as f32);
            let x = ring_radius * theta.cos();
            let z = ring_radius * theta.sin();
            positions.push(Vec3::new(x, y, z));
            normals.push(Vec3::new(x / radius, y / radius, z / radius));
            tex_coords.push(Vec2::new(
                segment as f32 / segments as f32,
                ring as f32 / rings as f32,
            ));
        }
    }

    let mut indices = Vec::new();
    for ring in 0..rings {
        for segment in 0..segments {
            let current = ring * (segments + 1) + segment;
            let next = current + segments + 1;
            indices.push(current);
            indices.push(next);
            indices.push(current + 1);
            indices.push(current + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(
            Vec3::new(-radius, -radius, -radius),
            Vec3::new(radius, radius, radius),
        ),
        vertex_layout: default_vertex_layout(),
    }
}

// ─── ComponentRegistration for HandleComponent<Mesh> ───

use crate::ecs::World;
use crate::scene::registry::ComponentRegistration;
use bincode::config;
use khora_core::ecs::entity::EntityId;
use std::any::TypeId;

fn serialize_mesh_handle(world: &World, entity: EntityId) -> Option<Vec<u8>> {
    world.get::<HandleComponent<Mesh>>(entity).map(|comp| {
        let mesh_ref = SerializableMeshRef::from_handle(comp);
        bincode::encode_to_vec(&mesh_ref, config::standard()).unwrap_or_default()
    })
}

fn deserialize_mesh_handle(world: &mut World, entity: EntityId, data: &[u8]) -> Result<(), String> {
    let (mesh_ref, _): (SerializableMeshRef, _) =
        bincode::decode_from_slice(data, config::standard()).map_err(|e| e.to_string())?;
    let component = mesh_ref.into_handle();
    world.add_component(entity, component).ok();
    Ok(())
}

inventory::submit! {
    ComponentRegistration {
        type_id: TypeId::of::<HandleComponent<Mesh>>(),
        type_name: "HandleComponent<Mesh>",
        serialize_recipe: serialize_mesh_handle,
        deserialize_recipe: deserialize_mesh_handle,
        create_default: |_world, _entity| {
            // Mesh handles can't be created with defaults — they need a specific asset.
            // This registration is for serialization only, not for "Add Component".
            Err("Mesh handles cannot be created with defaults".to_string())
        },
        to_json: |world, entity| {
            world.get::<HandleComponent<Mesh>>(entity).and_then(|comp| {
                let mesh_ref = SerializableMeshRef::from_handle(comp);
                serde_json::to_value(mesh_ref).ok()
            })
        },
        from_json: |_world, _entity, _value| {
            // Editing a mesh handle from the inspector is not yet wired —
            // it would require resolving the asset back from VFS, which the
            // editor's serde-JSON path doesn't do today.
            Err("Mesh handle editing from inspector not implemented".to_string())
        },
        remove: |world, entity| {
            // Surgical single-component remove — preserves every other
            // component on the entity (any domain).
            match world.remove_component::<HandleComponent<Mesh>>(entity) {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{:?}", e)),
            }
        },
    }
}
