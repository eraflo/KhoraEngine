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

//! Defines data structures for mesh representation.

use crate::{
    asset::Asset,
    math::{Aabb, Vec2, Vec3, Vec4},
    renderer::api::{
        pipeline::{PrimitiveTopology, VertexAttributeDescriptor},
        resource::BufferId,
        util::IndexFormat,
    },
};

/// Represents a complete mesh with vertex data and indices.
#[derive(Debug)]
pub struct Mesh {
    /// Vertex positions
    pub positions: Vec<Vec3>,
    /// Vertex normals
    pub normals: Option<Vec<Vec3>>,
    /// Vertex texture coordinates
    pub tex_coords: Option<Vec<Vec2>>,
    /// Vertex tangents (for normal mapping)
    pub tangents: Option<Vec<Vec4>>,
    /// Vertex colors
    pub colors: Option<Vec<Vec4>>,
    /// Index data for primitive assembly
    pub indices: Option<Vec<u32>>,
    /// Type of primitives to render
    pub primitive_type: PrimitiveTopology,
    /// Axis-aligned bounding box
    pub bounding_box: Aabb,
    /// Vertex format layout
    pub vertex_layout: Vec<VertexAttributeDescriptor>,
}

// Implement the core Asset trait for Mesh
impl Asset for Mesh {}

impl Mesh {
    /// Calculates the stride of a single vertex in bytes based on the vertex layout.
    pub fn vertex_size(&self) -> usize {
        self.vertex_layout
            .iter()
            .map(|attr| attr.format.size())
            .sum()
    }

    /// Creates an interleaved vertex buffer from the mesh data, ready for GPU upload.
    ///
    /// This function converts the struct-of-arrays (SoA) data layout of the `Mesh` struct
    /// into an array-of-structs (AoS) layout in a raw byte buffer. The exact memory
    /// layout is dictated by the `vertex_layout` field.
    pub fn create_vertex_buffer(&self) -> Vec<u8> {
        let vertex_count = self.positions.len();
        if vertex_count == 0 {
            return Vec::new();
        }

        let vertex_size = self.vertex_size();
        let mut buffer = vec![0u8; vertex_count * vertex_size];

        // --- Process each attribute type one by one ---

        // Positions (Attribute 0 - Mandatory)
        if let Some(pos_attr) = self.vertex_layout.iter().find(|a| a.shader_location == 0) {
            for (i, position) in self.positions.iter().enumerate().take(vertex_count) {
                let vertex_start = i * vertex_size;
                let attribute_offset = vertex_start + pos_attr.offset as usize;
                let data_bytes = bytemuck::bytes_of(position);
                buffer[attribute_offset..attribute_offset + data_bytes.len()]
                    .copy_from_slice(data_bytes);
            }
        }

        // Normals (Attribute 1 - Optional)
        if let (Some(normals), Some(normal_attr)) = (
            &self.normals,
            self.vertex_layout.iter().find(|a| a.shader_location == 1),
        ) {
            for (i, normal) in normals.iter().enumerate().take(vertex_count) {
                let vertex_start = i * vertex_size;
                let attribute_offset = vertex_start + normal_attr.offset as usize;
                let data_bytes = bytemuck::bytes_of(normal);
                buffer[attribute_offset..attribute_offset + data_bytes.len()]
                    .copy_from_slice(data_bytes);
            }
        }

        // Texture Coordinates (Attribute 2 - Optional)
        if let (Some(tex_coords), Some(uv_attr)) = (
            &self.tex_coords,
            self.vertex_layout.iter().find(|a| a.shader_location == 2),
        ) {
            for (i, tex_coord) in tex_coords.iter().enumerate().take(vertex_count) {
                let vertex_start = i * vertex_size;
                let attribute_offset = vertex_start + uv_attr.offset as usize;
                let data_bytes = bytemuck::bytes_of(tex_coord);
                buffer[attribute_offset..attribute_offset + data_bytes.len()]
                    .copy_from_slice(data_bytes);
            }
        }

        // Tangents (Attribute 3 - Optional)
        if let (Some(tangents), Some(tangent_attr)) = (
            &self.tangents,
            self.vertex_layout.iter().find(|a| a.shader_location == 3),
        ) {
            for (i, tangent) in tangents.iter().enumerate().take(vertex_count) {
                let vertex_start = i * vertex_size;
                let attribute_offset = vertex_start + tangent_attr.offset as usize;
                let data_bytes = bytemuck::bytes_of(tangent);
                buffer[attribute_offset..attribute_offset + data_bytes.len()]
                    .copy_from_slice(data_bytes);
            }
        }

        // Colors (Attribute 4 - Optional)
        if let (Some(colors), Some(color_attr)) = (
            &self.colors,
            self.vertex_layout.iter().find(|a| a.shader_location == 4),
        ) {
            for (i, color) in colors.iter().enumerate().take(vertex_count) {
                let vertex_start = i * vertex_size;
                let attribute_offset = vertex_start + color_attr.offset as usize;
                let data_bytes = bytemuck::bytes_of(color);
                buffer[attribute_offset..attribute_offset + data_bytes.len()]
                    .copy_from_slice(data_bytes);
            }
        }

        buffer
    }
}

/// A GPU-ready representation of a mesh, containing buffer IDs and draw parameters.
pub struct GpuMesh {
    /// The vertex buffer ID containing interleaved vertex data.
    pub vertex_buffer: BufferId,
    /// The index buffer ID, if the mesh uses indexed drawing.
    pub index_buffer: BufferId,
    /// The number of vertices or indices to draw.
    pub index_count: u32,
    /// The format of indices in the index buffer.
    pub index_format: IndexFormat,
    /// The topology of primitives to render (e.g., TriangleList, TriangleStrip).
    pub primitive_topology: PrimitiveTopology,
}

impl Asset for GpuMesh {}
