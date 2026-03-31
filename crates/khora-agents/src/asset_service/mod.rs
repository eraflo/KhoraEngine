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

//! Asset management service — on-demand loading, not an Agent.
//!
//! This service replaces the former `AssetAgent`. It provides a simple
//! `load()` API backed by a VFS + IO layer + decoder registry.
//! No GORNA negotiation, no per-frame budget — assets are loaded on-demand.

pub mod decoder;

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_core::vfs::VirtualFileSystem;
use khora_data::assets::Assets;
use khora_lanes::asset_lane::PackLoadingLane;
use khora_telemetry::MetricsRegistry;

use decoder::DecoderRegistry;

/// The asset management service.
///
/// Provides on-demand asset loading through a VFS → IO → Decode → Store pipeline.
/// Registered in `ServiceRegistry` and accessed by game code via `AppContext`.
pub struct AssetService {
    vfs: VirtualFileSystem,
    io: PackLoadingLane,
    decoders: DecoderRegistry,
    storages: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    load_count: usize,
}

impl AssetService {
    /// Creates a new `AssetService`.
    pub fn new(
        index_bytes: &[u8],
        data_file: File,
        metrics_registry: Arc<MetricsRegistry>,
    ) -> Result<Self> {
        let vfs = VirtualFileSystem::new(index_bytes)
            .context("Failed to initialize VirtualFileSystem from index bytes")?;

        let io = PackLoadingLane::new(data_file);

        Ok(Self {
            vfs,
            io,
            decoders: DecoderRegistry::new(metrics_registry),
            storages: HashMap::new(),
            load_count: 0,
        })
    }

    /// Registers a decoder for a specific asset type.
    pub fn register_decoder<A: Asset>(
        &mut self,
        type_name: &str,
        decoder: impl khora_lanes::asset_lane::AssetDecoder<A> + Send + Sync + 'static,
    ) {
        self.decoders.register::<A>(type_name, decoder);
    }

    /// Loads, decodes, and returns a typed handle to an asset.
    pub fn load<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<AssetHandle<A>> {
        let type_id = TypeId::of::<A>();

        // Get or create the typed storage.
        let storage = self
            .storages
            .entry(type_id)
            .or_insert_with(|| Box::new(Assets::<A>::new()));

        let assets = storage
            .downcast_mut::<Assets<A>>()
            .ok_or_else(|| anyhow!("Mismatched asset storage type"))?;

        // Return cached handle if already loaded.
        if let Some(handle) = assets.get(uuid) {
            return Ok(handle.clone());
        }

        // VFS lookup → IO → Decode → Store
        let metadata = self
            .vfs
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;

        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;

        let bytes = self.io.load_asset_bytes(source)?;
        let asset: A = self
            .decoders
            .decode::<A>(&metadata.asset_type_name, &bytes)?;

        let handle = AssetHandle::new(asset);
        assets.insert(*uuid, handle.clone());

        self.load_count += 1;
        Ok(handle)
    }

    /// Returns the total number of assets loaded so far.
    pub fn load_count(&self) -> usize {
        self.load_count
    }

    /// Returns the number of cached asset type storages.
    pub fn cached_type_count(&self) -> usize {
        self.storages.len()
    }
}
