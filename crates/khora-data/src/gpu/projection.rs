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

//! [`ProjectionRegistry`] — the store of what the GPU holds for the world's
//! meshes and materials, and the mesh upload.
//!
//! Created once at boot and kept as a runtime resource. `sync_all` is the
//! [`MeshOnGpu`] residency's work: every entity with a `HandleComponent<Mesh>`
//! gets a `HandleComponent<GpuMesh>`, idempotently (entities already holding
//! one are skipped by the `Without<HandleComponent<GpuMesh>>` filter). The
//! material upload is the `gpu_material_sync` residency's
//! (`residency/material_on_gpu.rs`).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::resource::SamplerId;
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::Residency;

use crate::assets::AssetStore;
use crate::ecs::{HandleComponent, Without, World};
use crate::residency::MeshOnGpu;

/// What the GPU holds for the world's meshes and materials, shared by the
/// mesh and material residencies, which run at `PreExtract` before any agent.
///
/// A runtime resource, made once at boot.
#[derive(Clone)]
pub struct ProjectionRegistry {
    /// Unified store holding the `Assets<GpuMesh>` / `Assets<GpuMaterial>` /
    /// `Assets<CpuTexture>` sub-stores.
    pub(crate) store: AssetStore,
    /// The single filtering sampler every material's maps share, created
    /// lazily on the first material upload.
    pub(crate) sampler: Arc<OnceLock<SamplerId>>,
}

impl ProjectionRegistry {
    /// Creates a new `ProjectionRegistry` backed by the shared [`AssetStore`].
    pub fn new(store: AssetStore) -> Self {
        Self {
            store,
            sampler: Arc::new(OnceLock::new()),
        }
    }

    /// Uploads any newly loaded CPU meshes to the GPU and tags their ECS entities.
    ///
    /// For each entity that has `HandleComponent<Mesh>` but not yet
    /// `HandleComponent<GpuMesh>`:
    /// 1. Checks whether the UUID is already in the `AssetStore` (shared across agents).
    /// 2. If not, uploads vertex + index buffers via `device`.
    /// 3. Inserts the result into the `AssetStore`.
    /// 4. Adds `HandleComponent<GpuMesh>` to the entity so subsequent frames skip it.
    ///
    /// This method is idempotent and safe to call every frame.
    pub fn sync_all(&self, world: &mut World, device: &dyn GraphicsDevice) {
        let cache = self.store.store::<GpuMesh>();
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

                // Cache miss: upload to GPU for the first time. A mesh the
                // device refuses stays untagged and is tried again next frame.
                if !cache
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&uuid)
                {
                    match MeshOnGpu::make(mesh_handle_comp, &mut &*device) {
                        Ok(gpu_mesh) => {
                            cache
                                .write()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(uuid, gpu_mesh);
                        }
                        Err(refusal) => {
                            log::error!("{refusal}");
                            continue;
                        }
                    }
                }

                // Schedule the ECS component addition.
                if let Some(handle) = cache.read().unwrap_or_else(|e| e.into_inner()).get(&uuid) {
                    pending.insert(
                        entity_id,
                        HandleComponent {
                            handle: handle.clone(),
                            uuid,
                        },
                    );
                }
            }
        }

        // Phase 2: mutate the ECS world (no longer borrowed by the query above).
        for (entity_id, component) in pending {
            // The ECS query skips stale orphan rows, so an entity reaches phase 2
            // only when it genuinely lacks the handle — any failure is unexpected.
            if let Err(e) = world.add_component(entity_id, component) {
                log::warn!("projection: attaching GPU handle to {entity_id:?} failed: {e:?}");
            }
        }
    }
}
