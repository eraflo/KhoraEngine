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

//! `MeshOnGpu`: a CPU mesh as vertex and index buffers, kept under its uuid in
//! the `AssetStore`'s `Assets<GpuMesh>` the render lanes read. Its `DataSystem`
//! runs [`ProjectionRegistry::sync_meshes`].

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage, IndexFormat};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::{DeviceClass, Residency, ResidencyError};
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::Runtime;

use crate::assets::ArrivalsRead;
use crate::ecs::{AssetRef, Changed, SystemContext, Tick, World};
use crate::gpu::AssetEviction;
use crate::ProjectionRegistry;

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

impl ProjectionRegistry {
    /// Makes every mesh that arrived in the asset store resident under its uuid.
    ///
    /// A mesh that landed or was replaced is made (again) on the GPU, and one
    /// gone from the store leaves it; a mesh a `AssetRef<Mesh>` changed since `since`
    /// names, held in the store but not on the GPU, is made again. When the
    /// arrivals were lost (a resync), the residency restores the truth: it
    /// makes resident every mesh a `AssetRef<Mesh>` names that the store holds,
    /// brings every resident one in line with the store, forgets what left
    /// the store, and makes nothing else. A replaced or gone GPU copy is
    /// handed to `eviction` to free after this frame's render (dropped unfreed
    /// without one). A copy stored again unchanged is not made twice, and one
    /// the device refuses is tried again on the next call. Attaches nothing to
    /// any entity.
    pub fn sync_meshes(
        &self,
        world: &World,
        since: Tick,
        device: &dyn GraphicsDevice,
        eviction: Option<&Mutex<AssetEviction>>,
    ) {
        let cpu = self.store.store::<Mesh>();
        let gpu = self.store.store::<GpuMesh>();
        let mut made = self.meshes.lock_recovered();

        // Every uuid whose CPU copy may differ from its GPU copy.
        let mut arrived = made.take_refused();
        let read = cpu.read_recovered().arrivals(MeshOnGpu::NAME);
        match read {
            ArrivalsRead::Since(arrivals) => {
                arrived.extend(arrivals.iter().map(|arrival| arrival.uuid));
            }
            ArrivalsRead::Resync => {
                let cpu = cpu.read_recovered();
                arrived.extend(
                    world
                        .query::<&AssetRef<Mesh>>()
                        .map(AssetRef::<Mesh>::uuid)
                        .filter(|uuid| cpu.contains(uuid)),
                );
                arrived.extend(gpu.read_recovered().keys());
                let gone: Vec<AssetUUID> = made
                    .every_known()
                    .filter(|uuid| !cpu.contains(uuid))
                    .collect();
                arrived.extend(gone);
            }
        }
        // A reference named again after its GPU copy was evicted.
        let named: HashSet<AssetUUID> = world
            .query::<(&AssetRef<Mesh>, Changed<AssetRef<Mesh>>)>()
            .since(since)
            .map(|(mesh, _)| mesh.uuid())
            .filter(|uuid| !arrived.contains(uuid))
            .collect();
        for uuid in named {
            if cpu.read_recovered().contains(&uuid) && !gpu.read_recovered().contains(&uuid) {
                arrived.insert(uuid);
            }
        }

        for uuid in arrived {
            let source = cpu.read_recovered().get(&uuid).cloned();
            let Some(source) = source else {
                retire_mesh(&self.store, eviction, &uuid);
                made.drop_uuid(&uuid);
                continue;
            };
            if made.is_made_from(&uuid, &source) && gpu.read_recovered().contains(&uuid) {
                continue;
            }
            match MeshOnGpu::make(&source, &mut &*device) {
                Ok(resident) => {
                    retire_mesh(&self.store, eviction, &uuid);
                    gpu.write_recovered().insert(uuid, resident);
                    made.made(uuid, source);
                }
                Err(refusal) => {
                    log::error!("{refusal}");
                    made.refuse(uuid);
                }
            }
        }
    }
}

/// Takes the GPU mesh of `uuid` out of the store, handed to `eviction` to free.
fn retire_mesh(
    store: &crate::AssetStore,
    eviction: Option<&Mutex<AssetEviction>>,
    uuid: &AssetUUID,
) {
    match eviction {
        Some(eviction) => eviction.lock_recovered().retire_mesh(store, uuid),
        None => {
            store.store::<GpuMesh>().write_recovered().remove(uuid);
        }
    }
}

fn gpu_mesh_sync_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    ctx: SystemContext,
) {
    let Some(proj) = runtime.resources.get::<ProjectionRegistry>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let eviction = runtime
        .resources
        .get::<Arc<Mutex<AssetEviction>>>()
        .map(|eviction| &**eviction);
    proj.sync_meshes(world, ctx.since, device.as_ref(), eviction);
}

crate::register_residency!(
    name = MeshOnGpu::NAME,
    run = gpu_mesh_sync_system,
    observes = [AssetRef<Mesh>],
);
