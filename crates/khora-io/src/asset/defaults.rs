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

//! What every Khora application's asset service starts with: the default
//! decoders, and a project's loose files opened with them.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_data::assets::SoundData;
use khora_telemetry::MetricsRegistry;

use super::decoders::audio::SymphoniaDecoder;
use super::decoders::mesh::{FileSystemResolver, GltfContainer, MeshDispatcher};
use super::file::FileLoader;
use super::id_registry::AssetIdRegistry;
use super::index_builder::IndexBuilder;
use super::AssetService;

impl AssetService {
    /// Walks `inventory::iter::<DecoderRegistration>` and runs each entry's
    /// `register` fn. Use this once after construction to pull in every
    /// decoder declared via `inventory::submit!` — currently `texture` and
    /// `font`. Audio and mesh decoders are intentionally explicit (see
    /// `decoders/audio` and `decoders/mesh` for the rationale).
    pub fn register_inventory_decoders(&mut self) {
        for reg in inventory::iter::<super::decoder_registry::DecoderRegistration> {
            (reg.register)(self);
        }
    }

    /// Opens a service over a project's loose `<project_root>/assets/`
    /// directory, with every default decoder registered (see
    /// [`Self::register_default_decoders`]).
    ///
    /// Asset ids come from `registry`: a path it freezes keeps its recorded
    /// UUID, every other path gets its `new_v5` default.
    pub fn open_loose_files(
        project_root: &Path,
        registry: &AssetIdRegistry,
        metrics_registry: Arc<MetricsRegistry>,
    ) -> Result<Self> {
        let assets_root = project_root.join("assets");
        let index_bytes = IndexBuilder::new(&assets_root)
            .with_registry(registry)
            .build_index_bytes()
            .with_context(|| format!("Failed to index {}", assets_root.display()))?;
        let io = Box::new(FileLoader::new(&assets_root));
        let mut svc = Self::new(&index_bytes, io, metrics_registry, None)?;
        svc.register_default_decoders(&assets_root);
        svc.serve_project(project_root);
        Ok(svc)
    }

    /// Registers the decoders every Khora application uses: the ones
    /// declared through `inventory` (see [`Self::register_inventory_decoders`]),
    /// audio through Symphonia, and meshes through the [`MeshDispatcher`].
    ///
    /// A glTF file's external URIs (`.bin` buffers, images) are resolved
    /// relative to the glTF file, as glTF specifies, and never outside
    /// `assets_root`. A glTF file is also a container: its scenes,
    /// primitives, materials and textures are sub-assets of their own (see
    /// [`GltfContainer`](super::decoders::mesh::GltfContainer)).
    pub fn register_default_decoders(&mut self, assets_root: &Path) {
        self.register_inventory_decoders();
        self.register_decoder::<SoundData>("audio", SymphoniaDecoder);
        let gltf_resolver = Arc::new(FileSystemResolver::new(assets_root));
        self.register_decoder::<Mesh>("mesh", MeshDispatcher::new(gltf_resolver));
        self.register_container("mesh", GltfContainer::new(assets_root));
    }
}
