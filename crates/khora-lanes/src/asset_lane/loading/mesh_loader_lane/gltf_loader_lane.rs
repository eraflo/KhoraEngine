// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! GLTF mesh format loader lane with support for both embedded and external resources.

use super::{AssetLoaderLane, GltfResourceResolver};
use anyhow::Result;
use base64::Engine;
use gltf::{mesh::Reader, Buffer};
use khora_core::{
    math::{geometry::Aabb, Vec2, Vec3, Vec4},
    renderer::api::{
        pipeline::enums::{PrimitiveTopology, VertexFormat},
        pipeline::VertexAttributeDescriptor,
        scene::Mesh,
    },
};
use std::{error::Error, sync::Arc};

/// Lane for loading GLTF meshes, configured with a resource resolver.
#[derive(Clone)]
pub struct GltfLoaderLane {
    resolver: Arc<dyn GltfResourceResolver>,
}

impl GltfLoaderLane {
    /// Creates a new GLTF loader lane with the given resource resolver.
    pub fn new(resolver: Arc<dyn GltfResourceResolver>) -> Self {
        Self { resolver }
    }
}

impl AssetLoaderLane<Mesh> for GltfLoaderLane {
    /// Loads a mesh from GLTF byte data.
    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        let gltf = gltf::Gltf::from_slice(bytes)
            .map_err(|e| format!("Failed to parse GLTF file: {}", e))?;

        let buffer_data = self
            .load_buffer_data(&gltf, &*self.resolver)
            .map_err(|e| format!("Failed to load GLTF buffer data: {}", e))?;

        let mesh = gltf
            .document
            .meshes()
            .next()
            .ok_or("No meshes found in GLTF file")?;
        let primitive = mesh
            .primitives()
            .next()
            .ok_or("No primitives found in mesh")?;

        // This closure has a unique, unnamable type `F`
        let get_buffer_data = |buffer: Buffer<'_>| Some(buffer_data[buffer.index()].as_slice());
        let reader = primitive.reader(get_buffer_data);

        // The compiler will infer the generic types for `F` when we call the helpers
        let positions = self.extract_positions(&reader)?;
        let normals = self.extract_normals(&reader);
        let tex_coords = self.extract_tex_coords(&reader);
        let tangents = self.extract_tangents(&reader);
        let colors = self.extract_colors(&reader);
        let indices = self.extract_indices(&reader);

        let bounding_box = {
            let bb = primitive.bounding_box();
            let min = Vec3::new(bb.min[0], bb.min[1], bb.min[2]);
            let max = Vec3::new(bb.max[0], bb.max[1], bb.max[2]);
            Aabb::from_min_max(min, max)
        };

        let vertex_layout = self.build_vertex_layout(
            normals.is_some(),
            tex_coords.is_some(),
            tangents.is_some(),
            colors.is_some(),
        );

        Ok(Mesh {
            positions,
            normals,
            tex_coords,
            tangents,
            colors,
            indices,
            primitive_type: self.map_primitive_type(primitive.mode()),
            bounding_box,
            vertex_layout,
        })
    }
}

impl GltfLoaderLane {
    fn load_buffer_data(
        &self,
        gltf: &gltf::Gltf,
        resolver: &dyn GltfResourceResolver,
    ) -> Result<Vec<Vec<u8>>, Box<dyn Error + Send + Sync>> {
        let mut buffer_data = Vec::new();
        for buffer in gltf.buffers() {
            match buffer.source() {
                gltf::buffer::Source::Bin => {
                    if let Some(blob) = gltf.blob.as_deref() {
                        buffer_data.push(blob.to_vec());
                    } else {
                        return Err("GLB file references binary chunk but it is missing".into());
                    }
                }
                gltf::buffer::Source::Uri(uri) => {
                    if uri.starts_with("data:") {
                        buffer_data.push(self.decode_data_uri(uri)?);
                    } else {
                        buffer_data.push(resolver.resolve_buffer(uri)?);
                    }
                }
            }
        }
        Ok(buffer_data)
    }

    fn decode_data_uri(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let prefix = "data:application/octet-stream;base64,";
        if let Some(base64_data) = uri.strip_prefix(prefix) {
            base64::engine::general_purpose::STANDARD
                .decode(base64_data)
                .map_err(Into::into)
        } else if let Some(base64_data) = uri.strip_prefix("data:application/gltf-buffer;base64,") {
            base64::engine::general_purpose::STANDARD
                .decode(base64_data)
                .map_err(Into::into)
        } else {
            Err(format!("Unsupported data URI format: {}", uri).into())
        }
    }

    // The helper functions are now generic over the closure type `F`.
    fn extract_positions<'a, 's, F>(
        &self,
        reader: &Reader<'a, 's, F>,
    ) -> Result<Vec<Vec3>, Box<dyn Error + Send + Sync>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader
            .read_positions()
            .map(|iter| iter.map(|[x, y, z]| Vec3::new(x, y, z)).collect())
            .ok_or_else(|| "Vertex positions attribute not found".into())
    }

    fn extract_normals<'a, 's, F>(&self, reader: &Reader<'a, 's, F>) -> Option<Vec<Vec3>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader
            .read_normals()
            .map(|iter| iter.map(|[x, y, z]| Vec3::new(x, y, z)).collect())
    }

    fn extract_tex_coords<'a, 's, F>(&self, reader: &Reader<'a, 's, F>) -> Option<Vec<Vec2>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader
            .read_tex_coords(0)
            .map(|iter| iter.into_f32().map(|[x, y]| Vec2::new(x, y)).collect())
    }

    fn extract_tangents<'a, 's, F>(&self, reader: &Reader<'a, 's, F>) -> Option<Vec<Vec4>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader
            .read_tangents()
            .map(|iter| iter.map(|[x, y, z, w]| Vec4::new(x, y, z, w)).collect())
    }

    fn extract_colors<'a, 's, F>(&self, reader: &Reader<'a, 's, F>) -> Option<Vec<Vec4>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader.read_colors(0).map(|iter| {
            iter.into_rgba_f32()
                .map(|[r, g, b, a]| Vec4::new(r, g, b, a))
                .collect()
        })
    }

    fn extract_indices<'a, 's, F>(&self, reader: &Reader<'a, 's, F>) -> Option<Vec<u32>>
    where
        F: Clone + Fn(Buffer<'a>) -> Option<&'s [u8]>,
    {
        reader.read_indices().map(|iter| iter.into_u32().collect())
    }

    fn build_vertex_layout(
        &self,
        has_normals: bool,
        has_tex_coords: bool,
        has_tangents: bool,
        has_colors: bool,
    ) -> Vec<VertexAttributeDescriptor> {
        let mut layout = Vec::new();
        let mut shader_location = 0;
        let mut offset = 0;

        layout.push(VertexAttributeDescriptor {
            shader_location,
            format: VertexFormat::Float32x3,
            offset: offset as u64,
        });
        shader_location += 1;
        offset += std::mem::size_of::<Vec3>();

        if has_normals {
            layout.push(VertexAttributeDescriptor {
                shader_location,
                format: VertexFormat::Float32x3,
                offset: offset as u64,
            });
            shader_location += 1;
            offset += std::mem::size_of::<Vec3>();
        }
        if has_tex_coords {
            layout.push(VertexAttributeDescriptor {
                shader_location,
                format: VertexFormat::Float32x2,
                offset: offset as u64,
            });
            shader_location += 1;
            offset += std::mem::size_of::<Vec2>();
        }
        if has_tangents {
            layout.push(VertexAttributeDescriptor {
                shader_location,
                format: VertexFormat::Float32x4,
                offset: offset as u64,
            });
            shader_location += 1;
            offset += std::mem::size_of::<Vec4>();
        }
        if has_colors {
            layout.push(VertexAttributeDescriptor {
                shader_location,
                format: VertexFormat::Float32x4,
                offset: offset as u64,
            });
        }
        layout
    }

    fn map_primitive_type(&self, mode: gltf::mesh::Mode) -> PrimitiveTopology {
        match mode {
            gltf::mesh::Mode::Triangles => PrimitiveTopology::TriangleList,
            gltf::mesh::Mode::TriangleStrip => PrimitiveTopology::TriangleStrip,
            gltf::mesh::Mode::Lines => PrimitiveTopology::LineList,
            gltf::mesh::Mode::LineStrip => PrimitiveTopology::LineStrip,
            gltf::mesh::Mode::Points => PrimitiveTopology::PointList,
            _ => PrimitiveTopology::TriangleList,
        }
    }
}

impl khora_core::lane::Lane for GltfLoaderLane {
    fn strategy_name(&self) -> &'static str {
        "GltfLoader"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Asset
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
