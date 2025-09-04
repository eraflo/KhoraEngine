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

//! Defines a lane for loading OBJ mesh assets.

use super::AssetLoaderLane;
use ahash::AHashMap;
use anyhow::{Context, Result};
use khora_core::{
    math::{geometry::Aabb, Vec2, Vec3},
    renderer::api::{Mesh, PrimitiveTopology, VertexAttributeDescriptor, VertexFormat},
};
use std::error::Error;

/// Lane for loading OBJ mesh assets
#[derive(Clone)]
pub struct ObjLoaderLane;

impl AssetLoaderLane<Mesh> for ObjLoaderLane {
    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        let obj_text = std::str::from_utf8(bytes).context("OBJ file is not valid UTF-8")?;

        let obj = tobj::load_obj_buf(
            &mut std::io::Cursor::new(obj_text),
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
            |_| Ok((Vec::new(), AHashMap::new())),
        )
        .context("Failed to parse OBJ file")?;

        let (models, _materials) = obj;

        if models.is_empty() {
            return Err("No models found in OBJ file".into());
        }

        // For now, just use the first model
        let model = &models[0];
        let mesh = &model.mesh;

        // Extract positions
        let positions = mesh
            .positions
            .chunks(3)
            .map(|v| Vec3::new(v[0], v[1], v[2]))
            .collect();

        // Extract normals if available
        let normals = if !mesh.normals.is_empty() {
            Some(
                mesh.normals
                    .chunks(3)
                    .map(|n| Vec3::new(n[0], n[1], n[2]))
                    .collect(),
            )
        } else {
            None
        };

        // Extract texture coordinates if available
        let tex_coords = if !mesh.texcoords.is_empty() {
            Some(
                mesh.texcoords
                    .chunks(2)
                    .map(|t| Vec2::new(t[0], t[1]))
                    .collect(),
            )
        } else {
            None
        };

        // Calculate bounding box
        let bounding_box = Aabb::from_points(
            &mesh
                .positions
                .chunks(3)
                .map(|v| Vec3::new(v[0], v[1], v[2]))
                .collect::<Vec<_>>(),
        )
        .unwrap_or(Aabb::INVALID);

        // Create vertex layout
        let mut vertex_layout = vec![VertexAttributeDescriptor {
            shader_location: 0,
            format: VertexFormat::Float32x3,
            offset: 0,
        }];

        let mut next_location = 1;
        if normals.is_some() {
            vertex_layout.push(VertexAttributeDescriptor {
                shader_location: next_location,
                format: VertexFormat::Float32x3,
                offset: std::mem::size_of::<Vec3>() as u64,
            });
            next_location += 1;
        }

        if tex_coords.is_some() {
            vertex_layout.push(VertexAttributeDescriptor {
                shader_location: next_location,
                format: VertexFormat::Float32x2,
                offset: (std::mem::size_of::<Vec3>() * (1 + normals.as_ref().map_or(0, |_| 1)))
                    as u64,
            });
        }

        Ok(Mesh {
            positions,
            normals,
            tex_coords,
            tangents: None,
            colors: None,
            indices: Some(mesh.indices.clone()),
            primitive_type: PrimitiveTopology::TriangleList,
            bounding_box,
            vertex_layout,
        })
    }
}
