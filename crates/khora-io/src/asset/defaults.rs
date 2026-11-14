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
use super::decoders::mesh::{
    FileSystemResolver, GltfContainer, GltfResourceResolver, MeshDispatcher, PackedResolver,
};
use super::file::FileLoader;
use super::id_registry::AssetIdRegistry;
use super::index_builder::IndexBuilder;
use super::AssetService;
use crate::trust::FileGuard;

impl AssetService {
    /// Registers every decoder declared with `inventory::submit!` (a `DecoderRegistration`).
    ///
    /// Call once after construction. Audio and mesh decoders are registered
    /// explicitly; `decoders/audio` and `decoders/mesh` say why.
    pub fn register_inventory_decoders(&mut self) {
        for reg in inventory::iter::<super::decoder_registry::DecoderRegistration> {
            (reg.register)(self);
        }
    }

    /// Opens a service over the loose files of `<project_root>/assets/`.
    ///
    /// Every default decoder is registered ([`Self::register_default_decoders`]).
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

    /// Registers the decoders every Khora application uses, over loose files in `assets_root`.
    ///
    /// Those of `inventory`, audio through Symphonia, meshes through the
    /// [`MeshDispatcher`], and glTF as a [`GltfContainer`]. A glTF's external
    /// files resolve beside it, never outside `assets_root`, through a guard at the
    /// service's trust reporting to its refusals: call [`Self::set_refusals`] first.
    pub fn register_default_decoders(&mut self, assets_root: &Path) {
        self.register_inventory_decoders();
        self.register_decoder::<SoundData>("audio", SymphoniaDecoder);
        let refusals = self.refusals();
        // At the trust the service reads at (`set_trust`, set before).
        let trust = self.reader().trust;
        let guard = || FileGuard::new(assets_root, trust, Arc::clone(&refusals));
        let gltf_resolver = Arc::new(FileSystemResolver::guarded(guard()));
        let container = GltfContainer::guarded(guard());
        self.register_decoder::<Mesh>("mesh", MeshDispatcher::new(gltf_resolver));
        self.register_container("mesh", container);
    }

    /// Registers the decoders every Khora application uses, for a service reading a pack.
    ///
    /// As [`Self::register_default_decoders`], except that a glTF's buffers and
    /// images resolve inside the pack by their asset key.
    pub fn register_packed_decoders(&mut self) {
        self.register_inventory_decoders();
        self.register_decoder::<SoundData>("audio", SymphoniaDecoder);
        let refusals = self.refusals();
        let reader = self.reader();
        let resolver: Arc<dyn GltfResourceResolver> = Arc::new(PackedResolver::new(
            &reader.index,
            Arc::clone(&reader.io),
            reader.manifest.clone(),
            refusals,
            reader.trust,
        ));
        self.register_decoder::<Mesh>("mesh", MeshDispatcher::new(Arc::clone(&resolver)));
        self.register_container("mesh", GltfContainer::resolving_with(resolver));
    }
}
