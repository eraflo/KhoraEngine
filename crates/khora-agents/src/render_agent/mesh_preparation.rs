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

//! Defines the system responsible for preparing GPU resources for newly loaded meshes.

use khora_core::{
    asset::AssetHandle,
    ecs::entity::EntityId,
    renderer::{
        api::{BufferDescriptor, BufferUsage, GpuMesh, IndexFormat},
        GraphicsDevice, Mesh,
    },
};
use khora_data::{
    assets::Assets,
    ecs::{HandleComponent, Without, World},
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Manages the GPU upload and caching of mesh assets.
///
/// This system observes the `World` for entities that have a `HandleComponent<Mesh>`
/// (a CPU asset) but do not yet have a `HandleComponent<GpuMesh>` (a GPU asset).
/// For each such entity, it ensures the corresponding mesh data is uploaded to the
/// GPU, caches the resulting `GpuMesh` asset, and then adds the
/// `HandleComponent<GpuMesh>` to the entity.
pub struct MeshPreparationSystem {
    /// A thread-safe, shared handle to the `GpuMesh` asset cache.
    gpu_meshes: Arc<RwLock<Assets<GpuMesh>>>,
}

impl MeshPreparationSystem {
    /// Creates a new `MeshPreparationSystem`.
    ///
    /// # Arguments
    ///
    /// * `gpu_meshes_cache`: A shared pointer to the `Assets` storage where
    ///   `GpuMesh` assets will be cached.
    pub fn new(gpu_meshes_cache: Arc<RwLock<Assets<GpuMesh>>>) -> Self {
        Self {
            gpu_meshes: gpu_meshes_cache,
        }
    }

    /// Runs the preparation logic for one frame.
    ///
    /// This should be called once per frame, typically during the "Control Plane"
    /// phase, before the render extraction process begins. The process is split
    /// into two phases to comply with Rust's borrow checker: a read-only query
    /// phase and a subsequent mutation phase.
    ///
    /// # Arguments
    ///
    /// * `world`: A mutable reference to the main ECS `World`.
    /// * `cpu_meshes`: An immutable reference to the storage for loaded CPU `Mesh` assets.
    /// * `graphics_device`: A trait object for the active graphics device, used for buffer creation.
    pub fn run(
        &self,
        world: &mut World,
        cpu_meshes: &Assets<Mesh>,
        graphics_device: &dyn GraphicsDevice,
    ) {
        // A temporary map to store the components that need to be added to entities.
        // We collect these first and add them later to avoid borrowing `world` mutably
        // while iterating over its query results.
        let mut pending_additions: HashMap<EntityId, HandleComponent<GpuMesh>> = HashMap::new();

        // --- Phase 1: Query and Prepare (Read-only on World) ---

        // Find all entities that have a CPU mesh handle but lack a GPU mesh handle.
        let query = world.query::<(
            EntityId,
            &HandleComponent<Mesh>,
            Without<HandleComponent<GpuMesh>>,
        )>();

        for (entity_id, mesh_handle_comp, _) in query {
            let mesh_uuid = mesh_handle_comp.uuid;

            // Check if the GpuMesh has already been created and cached by this system
            // in a previous iteration or for another entity.
            if !self.gpu_meshes.read().unwrap().contains(&mesh_uuid) {
                // Cache Miss: This is the first time we've seen this mesh asset.
                // We need to upload its data to the GPU.
                if let Some(cpu_mesh) = cpu_meshes.get(&mesh_uuid) {
                    let gpu_mesh = self.upload_mesh(cpu_mesh, graphics_device);
                    // Lock the cache for writing and insert the new GpuMesh.
                    self.gpu_meshes
                        .write()
                        .unwrap()
                        .insert(mesh_uuid, AssetHandle::new(gpu_mesh));
                }
            }

            // We schedule the addition of the corresponding handle component to the entity.
            if let Some(gpu_mesh_handle) = self.gpu_meshes.read().unwrap().get(&mesh_uuid) {
                pending_additions.insert(
                    entity_id,
                    HandleComponent {
                        handle: gpu_mesh_handle.clone(),
                        uuid: mesh_uuid,
                    },
                );
            }
        }

        // --- Phase 2: Mutate World ---

        // Now, iterate over the collected additions and apply them to the world.
        // This is safe because we are no longer borrowing the world for the query.
        for (entity_id, component) in pending_additions {
            let _ = world.add_component(entity_id, component);
        }
    }

    /// A private helper function that takes a CPU `Mesh` and uploads its data
    /// to the GPU, returning a `GpuMesh` containing the new buffer handles.
    fn upload_mesh(&self, mesh: &Mesh, device: &dyn GraphicsDevice) -> GpuMesh {
        // 1. Create and upload the vertex buffer.
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

        // 2. Create and upload the index buffer, if the mesh has indices.
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
            // If there are no indices, create an empty buffer as a placeholder.
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

        // 3. Assemble and return the `GpuMesh` asset.
        GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
            index_format: IndexFormat::Uint32, // Assuming U32 indices as per our mesh loader.
        }
    }
}
