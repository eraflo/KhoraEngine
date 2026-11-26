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

//! [`ProjectionRegistry`], the store of what the GPU holds for the meshes and
//! materials in the asset store, and what the mesh and material residencies
//! remember between frames.

use std::sync::{Arc, Mutex, OnceLock};

use khora_core::asset::Material;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::SamplerId;

use crate::assets::AssetStore;
use crate::gpu::{MadeCopies, MaterialWaits};

/// What the GPU holds for the asset store's meshes and materials, shared by their residencies.
///
/// They run at `PreExtract`, before any agent. Each GPU copy is kept under its
/// asset's uuid, never on an entity. A runtime resource, made once at boot.
///
/// Lock order, outermost first; every holder takes them only downward:
/// 1. `meshes`, or `materials` then `material_waits`: held by
///    [`sync_meshes`](Self::sync_meshes) / [`sync_materials`](Self::sync_materials)
///    for the whole run, and by nothing else.
/// 2. The `CpuTexture` store's read lock: held by `sync_materials` while it
///    builds the materials that arrived, across the device calls of a build.
///    Its other holders (the reload, the resolver, the intake, the UI image
///    residency, the IBL bake) take no lock below it.
/// 3. The eviction service's mutex: taken by the residencies to retire a
///    copy, and by the `asset_eviction` system for its tick.
/// 4. The `GpuMesh` / `GpuMaterial` store locks: written under the eviction's
///    mutex by `retire_mesh` / `retire_material` and its tick, and by the
///    residencies to insert a copy; read by the render lanes, which hold none
///    of the locks above.
///
/// Every store's arrivals journal is a leaf below its own store lock. No
/// `GpuMesh` / `GpuMaterial` lock is held across a device call.
#[derive(Clone)]
pub struct ProjectionRegistry {
    /// Unified store holding the `Assets<GpuMesh>` / `Assets<GpuMaterial>` /
    /// `Assets<CpuTexture>` sub-stores.
    pub(crate) store: AssetStore,
    /// The single filtering sampler every material's maps share, created
    /// lazily on the first material upload.
    pub(crate) sampler: Arc<OnceLock<SamplerId>>,
    /// The mesh each GPU mesh was made from, and the meshes refused.
    pub(crate) meshes: Arc<Mutex<MadeCopies<Mesh>>>,
    /// The material each GPU material was made from, and the materials refused.
    pub(crate) materials: Arc<Mutex<MadeCopies<Box<dyn Material>>>>,
    /// The textures each material names, and the materials waiting for theirs.
    pub(crate) material_waits: Arc<Mutex<MaterialWaits>>,
}

impl ProjectionRegistry {
    /// Creates a new `ProjectionRegistry` backed by the shared [`AssetStore`].
    pub fn new(store: AssetStore) -> Self {
        Self {
            store,
            sampler: Arc::new(OnceLock::new()),
            meshes: Arc::new(Mutex::new(MadeCopies::default())),
            materials: Arc::new(Mutex::new(MadeCopies::default())),
            material_waits: Arc::new(Mutex::new(MaterialWaits::default())),
        }
    }
}
