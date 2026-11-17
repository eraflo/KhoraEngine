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

//! [`ProjectionRegistry`], the store of what the GPU holds for the world's
//! meshes and materials, and the mesh upload the [`MeshOnGpu`] residency runs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::resource::SamplerId;
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::Residency;
use khora_core::sync::RwLockExt;

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

    /// Uploads newly loaded meshes and gives each entity its
    /// `HandleComponent<GpuMesh>`.
    ///
    /// Idempotent: a tagged entity is skipped, a mesh shared by several
    /// entities is uploaded once, and a mesh the device refuses is tried again
    /// on the next call.
    pub fn sync_all(&self, world: &mut World, device: &dyn GraphicsDevice) {
        let cache = self.store.store::<GpuMesh>();
        // Collected first: the query borrows the world the tags are added to.
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
                if !cache.read_recovered().contains(&uuid) {
                    match MeshOnGpu::make(mesh_handle_comp, &mut &*device) {
                        Ok(gpu_mesh) => {
                            cache.write_recovered().insert(uuid, gpu_mesh);
                        }
                        Err(refusal) => {
                            log::error!("{refusal}");
                            continue;
                        }
                    }
                }

                if let Some(handle) = cache.read_recovered().get(&uuid) {
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

        for (entity_id, component) in pending {
            // The ECS query skips stale orphan rows, so an entity reaches this loop
            // only when it genuinely lacks the handle — any failure is unexpected.
            if let Err(e) = world.add_component(entity_id, component) {
                log::warn!("projection: attaching GPU handle to {entity_id:?} failed: {e:?}");
            }
        }
    }
}
