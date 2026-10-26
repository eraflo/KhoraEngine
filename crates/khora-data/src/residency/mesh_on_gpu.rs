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

//! `MeshOnGpu` — a CPU mesh as vertex and index buffers.
//!
//! Its DataSystem is `gpu_mesh_sync`
//! ([`ProjectionRegistry::sync_all`](crate::ProjectionRegistry::sync_all)): the
//! residents live in the `AssetStore`'s `Assets<GpuMesh>`, which the render
//! lanes read, and the asset reload retires a changed mesh's entry so it is
//! made again.

use khora_core::asset::AssetHandle;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage, IndexFormat};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::{DeviceClass, Residency, ResidencyError};

/// A mesh, as GPU buffers.
pub struct MeshOnGpu;

impl Residency for MeshOnGpu {
    const NAME: &'static str = "gpu_mesh_sync";
    const DEVICE: DeviceClass = DeviceClass::Gpu;
    type Source = Mesh;
    type Resident = AssetHandle<GpuMesh>;
    type Target<'a> = &'a dyn GraphicsDevice;

    fn make(
        mesh: &Mesh,
        device: &mut &dyn GraphicsDevice,
    ) -> Result<AssetHandle<GpuMesh>, ResidencyError> {
        let refused = |what: &str, error: &dyn std::fmt::Debug| ResidencyError {
            residency: Self::NAME,
            reason: format!("the device refused the {what}: {error:?}"),
        };
        let vertex_data = mesh.create_vertex_buffer();
        let vertex_buffer = device
            .create_buffer_with_data(
                &BufferDescriptor {
                    label: Some("Mesh Vertex Buffer".into()),
                    size: vertex_data.len() as u64,
                    usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
                    mapped_at_creation: false,
                },
                &vertex_data,
            )
            .map_err(|e| refused("vertex buffer", &e))?;

        // An unindexed mesh still binds an index buffer: an empty one.
        let indexed = match &mesh.indices {
            Some(indices) => {
                let index_data: &[u8] = bytemuck::cast_slice(indices);
                device
                    .create_buffer_with_data(
                        &BufferDescriptor {
                            label: Some("Mesh Index Buffer".into()),
                            size: index_data.len() as u64,
                            usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
                            mapped_at_creation: false,
                        },
                        index_data,
                    )
                    .map(|buffer| (buffer, indices.len() as u32))
            }
            None => device
                .create_buffer(&BufferDescriptor {
                    label: Some("Empty Index Buffer".into()),
                    size: 0,
                    usage: BufferUsage::INDEX,
                    mapped_at_creation: false,
                })
                .map(|buffer| (buffer, 0)),
        };
        let (index_buffer, index_count) = match indexed {
            Ok(indexed) => indexed,
            Err(e) => {
                // Nothing holds the vertex buffer yet: give it back.
                if let Err(release) = device.destroy_buffer(vertex_buffer) {
                    log::warn!(
                        "{}: releasing a vertex buffer failed: {release:?}",
                        Self::NAME
                    );
                }
                return Err(refused("index buffer", &e));
            }
        };

        Ok(AssetHandle::new(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: mesh.primitive_type,
        }))
    }

    fn release(mesh: AssetHandle<GpuMesh>, device: &mut &dyn GraphicsDevice) {
        for buffer in [mesh.vertex_buffer, mesh.index_buffer] {
            if let Err(e) = device.destroy_buffer(buffer) {
                log::warn!("{}: releasing a mesh buffer failed: {e:?}", Self::NAME);
            }
        }
    }
}
