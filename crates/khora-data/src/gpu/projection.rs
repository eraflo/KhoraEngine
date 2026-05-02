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

//! CPU→GPU mesh projection — the data-layer replacement for `MeshPreparationSystem`.
//!
//! [`ProjectionRegistry`] is created once in `engine.rs` bootstrap, registered
//! into `ServiceRegistry`, and called via `sync_all()` in `tick_with_services()`
//! **before** the scheduler dispatches agents.
//!
//! After `sync_all()` returns for a given frame, every entity that has a
//! `HandleComponent<Mesh>` also has a `HandleComponent<GpuMesh>`, and the
//! shared `GpuCache` is fully up to date.  This call is idempotent: entities
//! already holding a `HandleComponent<GpuMesh>` are skipped via the
//! `Without<HandleComponent<GpuMesh>>` query filter.

use crate::{
    ecs::{HandleComponent, Without, World},
    gpu::GpuCache,
};
use khora_core::{
    asset::AssetHandle,
    ecs::entity::EntityId,
    renderer::{
        api::{
            resource::{BufferDescriptor, BufferUsage},
            scene::{GpuMesh, Mesh},
            util::IndexFormat,
        },
        GraphicsDevice,
    },
};
use std::collections::HashMap;

/// Engine-wide CPU→GPU mesh upload service.
///
/// Registered into `ServiceRegistry` during bootstrap.
/// `sync_all()` is called once per frame in `EngineCore::tick_with_services()`
/// before the scheduler runs agents.
#[derive(Clone)]
pub struct ProjectionRegistry {
    cache: GpuCache,
}

impl ProjectionRegistry {
    /// Creates a new `ProjectionRegistry` backed by the given shared cache.
    pub fn new(cache: GpuCache) -> Self {
        Self { cache }
    }

    /// Returns a reference to the underlying `GpuCache`.
    pub fn gpu_cache(&self) -> &GpuCache {
        &self.cache
    }

    /// Uploads any newly loaded CPU meshes to the GPU and tags their ECS entities.
    ///
    /// For each entity that has `HandleComponent<Mesh>` but not yet
    /// `HandleComponent<GpuMesh>`:
    /// 1. Checks whether the UUID is already in `GpuCache` (shared across agents).
    /// 2. If not, uploads vertex + index buffers via `device`.
    /// 3. Inserts the result into `GpuCache`.
    /// 4. Adds `HandleComponent<GpuMesh>` to the entity so subsequent frames skip it.
    ///
    /// This method is idempotent and safe to call every frame.
    pub fn sync_all(&self, world: &mut World, device: &dyn GraphicsDevice) {
        // Phase 1: collect pending uploads (read-only ECS borrow).
        let mut pending: HashMap<EntityId, HandleComponent<GpuMesh>> = HashMap::new();

        {
            let query = world.query::<(
                EntityId,
                &HandleComponent<Mesh>,
                Without<HandleComponent<GpuMesh>>,
            )>();

            for (entity_id, mesh_handle_comp, _) in query {
                let uuid = mesh_handle_comp.uuid;

                // Cache miss: upload to GPU for the first time.
                if !self.cache.0.read().unwrap().contains(&uuid) {
                    let gpu_mesh = Self::upload_mesh(mesh_handle_comp, device);
                    self.cache
                        .0
                        .write()
                        .unwrap()
                        .insert(uuid, AssetHandle::new(gpu_mesh));
                }

                // Schedule the ECS component addition.
                if let Some(handle) = self.cache.0.read().unwrap().get(&uuid) {
                    pending.insert(entity_id, HandleComponent { handle: handle.clone(), uuid });
                }
            }
        }

        // Phase 2: mutate the ECS world (no longer borrowed by the query above).
        for (entity_id, component) in pending {
            let _ = world.add_component(entity_id, component);
        }
    }

    /// Uploads a single CPU [`Mesh`] to the GPU and returns the resulting [`GpuMesh`].
    fn upload_mesh(mesh: &Mesh, device: &dyn GraphicsDevice) -> GpuMesh {
        // Upload vertex buffer.
        let vertex_data = mesh.create_vertex_buffer();
        let vb_desc = BufferDescriptor {
            label: Some("Mesh Vertex Buffer".into()),
            size: vertex_data.len() as u64,
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        };
        let vertex_buffer = device
            .create_buffer_with_data(&vb_desc, &vertex_data)
            .expect("Failed to create vertex buffer");

        // Upload index buffer (or create an empty placeholder).
        let (index_buffer, index_count) = if let Some(indices) = &mesh.indices {
            let index_data = bytemuck::cast_slice(indices);
            let ib_desc = BufferDescriptor {
                label: Some("Mesh Index Buffer".into()),
                size: index_data.len() as u64,
                usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            };
            let buffer = device
                .create_buffer_with_data(&ib_desc, index_data)
                .expect("Failed to create index buffer");
            (buffer, indices.len() as u32)
        } else {
            let dummy_desc = BufferDescriptor {
                label: Some("Empty Index Buffer".into()),
                size: 0,
                usage: BufferUsage::INDEX,
                mapped_at_creation: false,
            };
            let buffer = device
                .create_buffer(&dummy_desc)
                .expect("Failed to create empty index buffer");
            (buffer, 0)
        };

        GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: mesh.primitive_type,
        }
    }
}
