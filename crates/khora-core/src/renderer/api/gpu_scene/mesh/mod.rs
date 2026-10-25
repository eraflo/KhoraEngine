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

use crate::asset::Asset;
use crate::math::{Aabb, Vec2, Vec3, Vec4};
use crate::renderer::api::pipeline::{
    PrimitiveTopology, VertexAttributeDescriptor, VertexBufferLayoutDescriptor, VertexFormat,
    VertexStepMode,
};
use crate::renderer::api::resource::{BufferId, IndexFormat};
use std::borrow::Cow;

/// Represents a complete mesh with vertex data and indices.
///
/// The attributes are what the source provided; the GPU vertex every mesh is
/// uploaded as is fixed by the engine ([`Mesh::VERTEX_ATTRIBUTES`]), so an
/// absent attribute is filled with its default when the buffer is built.
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
    /// Vertex colors, linear RGBA
    pub colors: Option<Vec<Vec4>>,
    /// Index data for primitive assembly
    pub indices: Option<Vec<u32>>,
    /// Type of primitives to render
    pub primitive_type: PrimitiveTopology,
    /// Axis-aligned bounding box
    pub bounding_box: Aabb,
}

// Implement the core Asset trait for Mesh
impl Asset for Mesh {}

impl Mesh {
    /// Byte size of one vertex in every mesh vertex buffer.
    pub const VERTEX_STRIDE: u64 = 48;

    /// The engine's mesh vertex: what [`Mesh::create_vertex_buffer`] writes and
    /// every mesh pipeline declares. Location 0 position, 1 normal, 2 UV,
    /// 3 linear RGBA colour.
    pub const VERTEX_ATTRIBUTES: &'static [VertexAttributeDescriptor] = &[
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
        VertexAttributeDescriptor {
            shader_location: 3,
            format: VertexFormat::Float32x4,
            offset: 32,
        },
    ];

    /// The vertex buffer layout every mesh pipeline binds.
    pub fn vertex_buffer_layout() -> VertexBufferLayoutDescriptor<'static> {
        VertexBufferLayoutDescriptor {
            array_stride: Self::VERTEX_STRIDE,
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Borrowed(Self::VERTEX_ATTRIBUTES),
        }
    }

    /// Creates the interleaved vertex buffer of this mesh, ready for GPU upload.
    ///
    /// Always [`Mesh::VERTEX_STRIDE`] bytes per position, laid out as
    /// [`Mesh::VERTEX_ATTRIBUTES`]. An absent attribute, or the part of one
    /// shorter than `positions`, takes its default: normal `(0, 1, 0)`,
    /// UV `(0, 0)`, colour opaque white.
    pub fn create_vertex_buffer(&self) -> Vec<u8> {
        let stride = Self::VERTEX_STRIDE as usize;
        let mut buffer = Vec::with_capacity(self.positions.len() * stride);
        for (i, position) in self.positions.iter().enumerate() {
            let normal = attribute_at(&self.normals, i, Vec3::Y);
            let uv = attribute_at(&self.tex_coords, i, Vec2::ZERO);
            let color = attribute_at(&self.colors, i, Vec4::ONE);
            buffer.extend_from_slice(bytemuck::bytes_of(position));
            buffer.extend_from_slice(bytemuck::bytes_of(&normal));
            buffer.extend_from_slice(bytemuck::bytes_of(&uv));
            buffer.extend_from_slice(bytemuck::bytes_of(&color));
        }
        buffer
    }

    /// Gives a triangle-list mesh without normals flat normals.
    ///
    /// Each triangle gets its own three vertices carrying the face normal
    /// (counter-clockwise front); every other attribute is duplicated along,
    /// and the indices become `0..3n`. A degenerate triangle gets `(0, 1, 0)`.
    /// Does nothing when the mesh already has normals or is not a triangle list.
    pub fn generate_flat_normals(&mut self) {
        if self.normals.is_some() || self.primitive_type != PrimitiveTopology::TriangleList {
            return;
        }
        let corners: Vec<u32> = match &self.indices {
            Some(indices) => indices.clone(),
            None => (0..self.positions.len() as u32).collect(),
        };
        // A trailing partial triangle is not drawn, nor is one with a corner
        // past the positions; both are dropped whole.
        let count = self.positions.len();
        let corners: Vec<usize> = corners
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|triangle| triangle.iter().all(|&c| (c as usize) < count))
            .flatten()
            .map(|&c| c as usize)
            .collect();

        let mut normals = Vec::with_capacity(corners.len());
        for triangle in corners.as_chunks::<3>().0 {
            let [a, b, c] = [
                self.positions[triangle[0]],
                self.positions[triangle[1]],
                self.positions[triangle[2]],
            ];
            // No tolerance on the length: a millimetre triangle has a tiny
            // cross product that is still its face direction.
            let cross = (b - a).cross(c - a);
            let length = cross.length();
            let face = if length > 0.0 && length.is_finite() {
                cross * (1.0 / length)
            } else {
                Vec3::Y
            };
            normals.extend([face; 3]);
        }

        self.positions = corners.iter().map(|&c| self.positions[c]).collect();
        self.tex_coords = unweld(&self.tex_coords, &corners);
        self.tangents = unweld(&self.tangents, &corners);
        self.colors = unweld(&self.colors, &corners);
        self.normals = Some(normals);
        self.indices = Some((0..self.positions.len() as u32).collect());
    }
}

/// The value of an optional per-vertex attribute at `index`, or `default`
/// when the attribute is absent or shorter than the positions.
fn attribute_at<T: Copy>(attribute: &Option<Vec<T>>, index: usize, default: T) -> T {
    attribute
        .as_ref()
        .and_then(|values| values.get(index).copied())
        .unwrap_or(default)
}

/// One value per corner, read from a per-vertex attribute.
fn unweld<T: Copy>(attribute: &Option<Vec<T>>, corners: &[usize]) -> Option<Vec<T>> {
    attribute.as_ref().map(|values| {
        corners
            .iter()
            .map_while(|&c| values.get(c).copied())
            .collect()
    })
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

#[cfg(test)]
mod tests;
