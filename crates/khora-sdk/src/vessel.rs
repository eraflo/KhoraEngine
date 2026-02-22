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

//! Vessel abstraction for the Khora SDK.
//!
//! A Vessel is a high-level wrapper around an ECS entity that provides
//! a convenient API for common game development tasks. It allows you to
//! create and manipulate entities without dealing directly with the ECS.
//!
//! Every Vessel has both a Transform (local) and GlobalTransform (world)
//! which are kept in sync automatically.
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a cube at a specific position
//! let cube = world.spawn_at(Vec3::new(0.0, 0.5, -5.0))
//!     .as_cube(1.0)
//!     .build();
//!
//! // Create a camera
//! let camera = world.spawn_at(Vec3::new(0.0, 2.0, 10.0))
//!     .as_camera_perspective(45.0, 16.0/9.0, 0.1, 1000.0)
//!     .build();
//! ```

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Aabb, Vec2, Vec3};
use khora_core::renderer::api::{
    pipeline::{PrimitiveTopology, VertexAttributeDescriptor, VertexFormat},
    scene::Mesh,
};
use khora_data::ecs::{GlobalTransform, Transform};

use crate::GameWorld;

/// A high-level wrapper around an ECS entity.
///
/// Vessel provides a builder-pattern API for creating and configuring
/// game entities. It automatically handles the underlying ECS components
/// and synchronization between Transform and GlobalTransform.
///
/// Every Vessel is guaranteed to have:
/// - Transform (local position/rotation/scale)
/// - GlobalTransform (world-space transform for rendering)
pub struct Vessel<'a> {
    world: &'a mut GameWorld,
    entity: EntityId,
    transform: Transform,
}

impl<'a> Vessel<'a> {
    /// Creates a new Vessel at the origin.
    ///
    /// The entity is spawned immediately with Transform and GlobalTransform.
    pub fn new(world: &'a mut GameWorld) -> Self {
        let transform = Transform::identity();
        let global = GlobalTransform::new(transform.to_mat4());
        let entity = world.spawn((transform, global));

        Self {
            world,
            entity,
            transform,
        }
    }

    /// Creates a new Vessel at the specified position.
    pub fn at(world: &'a mut GameWorld, position: Vec3) -> Self {
        let transform = Transform::from_translation(position);
        let global = GlobalTransform::new(transform.to_mat4());
        let entity = world.spawn((transform, global));

        Self {
            world,
            entity,
            transform,
        }
    }

    /// Sets the transform (position, rotation, scale).
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Sets the position.
    pub fn at_position(mut self, position: Vec3) -> Self {
        self.transform.translation = position;
        self
    }

    /// Sets the rotation of the transform.
    pub fn with_rotation(mut self, rotation: khora_core::math::Quaternion) -> Self {
        self.transform.rotation = rotation;
        self
    }

    /// Sets the scale of the transform.
    pub fn with_scale(mut self, scale: Vec3) -> Self {
        self.transform.scale = scale;
        self
    }

    /// Adds a generic component to the entity immediately.
    ///
    /// Since the entity is already spawned when the `Vessel` is created,
    /// we can add the component right away. This avoids needing a field
    /// on `Vessel` for every possible component type.
    pub fn with_component<C: khora_data::ecs::Component>(self, component: C) -> Self {
        self.world.add_component(self.entity, component);
        self
    }

    /// Returns the entity ID.
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// Builds the Vessel, updating the final transforms.
    ///
    /// This finalizes the Vessel creation and returns the entity ID.
    pub fn build(self) -> EntityId {
        // Update transform (entity was spawned with one, so we need to update it)
        if let Some(existing_transform) = self.world.get_component_mut::<Transform>(self.entity) {
            *existing_transform = self.transform;
        }

        // Sync GlobalTransform
        let global = GlobalTransform::new(self.transform.to_mat4());
        if let Some(existing_global) = self.world.get_component_mut::<GlobalTransform>(self.entity)
        {
            *existing_global = global;
        }

        self.entity
    }
}

/// Creates a Vessel with a plane mesh at the origin.
pub fn spawn_plane<'a>(world: &'a mut GameWorld, size: f32, y: f32) -> Vessel<'a> {
    let mesh = create_plane(size, y);
    let handle = world.add_mesh(mesh);
    Vessel::new(world).with_component(handle)
}

/// Creates a Vessel with a cube mesh at a specific position.
pub fn spawn_cube_at<'a>(world: &'a mut GameWorld, position: Vec3, size: f32) -> Vessel<'a> {
    let mesh = create_cube(size);
    let handle = world.add_mesh(mesh);
    Vessel::at(world, position).with_component(handle)
}

/// Creates a Vessel with a sphere mesh at the origin.
pub fn spawn_sphere<'a>(
    world: &'a mut GameWorld,
    radius: f32,
    segments: u32,
    rings: u32,
) -> Vessel<'a> {
    let mesh = create_sphere(radius, segments, rings);
    let handle = world.add_mesh(mesh);
    Vessel::new(world).with_component(handle)
}

// =============================================================================
// Primitive Mesh Generation (internal)
// =============================================================================

/// Creates a plane mesh on the XZ plane.
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

    // Layout: Position (0), Normal (1), UV (2)
    let vertex_layout = vec![
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
    ];

    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(Vec3::new(-half, y, -half), Vec3::new(half, y, half)),
        vertex_layout,
    }
}

/// Creates a cube mesh centered at origin.
fn create_cube(size: f32) -> Mesh {
    let half = size / 2.0;

    // 24 vertices (4 per face, 6 faces)
    let positions = vec![
        // Front face (+Z)
        Vec3::new(-half, -half, half),
        Vec3::new(half, -half, half),
        Vec3::new(half, half, half),
        Vec3::new(-half, half, half),
        // Back face (-Z)
        Vec3::new(half, -half, -half),
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, half, -half),
        Vec3::new(half, half, -half),
        // Right face (+X)
        Vec3::new(half, -half, half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, half, -half),
        Vec3::new(half, half, half),
        // Left face (-X)
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, -half, half),
        Vec3::new(-half, half, half),
        Vec3::new(-half, half, -half),
        // Top face (+Y)
        Vec3::new(-half, half, half),
        Vec3::new(half, half, half),
        Vec3::new(half, half, -half),
        Vec3::new(-half, half, -half),
        // Bottom face (-Y)
        Vec3::new(-half, -half, -half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, -half, half),
        Vec3::new(-half, -half, half),
    ];

    let normals = vec![
        // Front
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        // Back
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        // Right
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        // Left
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        // Top
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        // Bottom
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
    ];

    let tex_coords = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ]
    .into_iter()
    .map(|uv| Vec2::new(uv[0], uv[1]))
    .collect();

    // Indices for all 6 faces (2 triangles per face)
    let indices = vec![
        // Front
        0u32, 1, 2, 0, 2, 3, // Back
        4, 5, 6, 4, 6, 7, // Right
        8, 9, 10, 8, 10, 11, // Left
        12, 13, 14, 12, 14, 15, // Top
        16, 17, 18, 16, 18, 19, // Bottom
        20, 21, 22, 20, 22, 23,
    ];

    // Layout: Position (0), Normal (1), UV (2)
    let vertex_layout = vec![
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
        vertex_layout,
    }
}

/// Creates a sphere mesh.
fn create_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tex_coords = Vec::new();

    // Generate vertices
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

    // Generate indices
    let mut indices = Vec::new();
    for ring in 0..rings {
        for segment in 0..segments {
            let current = ring * (segments + 1) + segment;
            let next = current + segments + 1;

            // Two triangles per quad
            indices.push(current);
            indices.push(next);
            indices.push(current + 1);

            indices.push(current + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    // Layout: Position (0), Normal (1), UV (2)
    let vertex_layout = vec![
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
            Vec3::new(-radius, -radius, -radius),
            Vec3::new(radius, radius, radius),
        ),
        vertex_layout,
    }
}
