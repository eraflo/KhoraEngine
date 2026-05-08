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
//! This service provides a `load()` API backed by a VFS + IO layer + decoder registry.
//! No GORNA negotiation, no per-frame budget — assets are loaded on-demand.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_data::assets::Assets;
use khora_telemetry::MetricsRegistry;

use super::io::AssetIo;
use super::manifest::PackManifest;
use super::registry::DecoderRegistry;
use crate::vfs::VirtualFileSystem;

/// Trait-object adapter so the service's storage map can dispatch
/// per-uuid removal without knowing the concrete `A` at the call site.
///
/// One impl exists for every `Assets<A: Asset>` (see the blanket below).
trait AnyAssets: Any + Send + Sync {
    /// Removes the cached handle for `uuid` if present. Returns `true` if
    /// something was removed.
    fn remove_uuid(&mut self, uuid: &AssetUUID) -> bool;

    /// Required for downcast back to `Assets<A>` from the service's
    /// `HashMap<TypeId, Box<dyn AnyAssets>>` map.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<A: Asset + Send + Sync + 'static> AnyAssets for Assets<A> {
    fn remove_uuid(&mut self, uuid: &AssetUUID) -> bool {
        self.remove(uuid).is_some()
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The asset management service.
///
/// Provides on-demand asset loading through a VFS → IO → Decode → Store pipeline.
/// Registered in `ServiceRegistry` and accessed by game code via `AppContext`.
pub struct AssetService {
    vfs: VirtualFileSystem,
    io: Box<dyn AssetIo>,
    decoders: DecoderRegistry,
    storages: HashMap<TypeId, Box<dyn AnyAssets>>,
    load_count: usize,
    /// When `Some`, every byte slice produced by `io.load_bytes` is
    /// re-hashed against the manifest before reaching the decoder.
    /// Constructed by the runtime only when `RuntimeConfig::verify_integrity`
    /// is set and `manifest.bin` is present next to `data.pack`.
    manifest: Option<PackManifest>,
}

impl AssetService {
    /// Creates a new `AssetService`.
    ///
    /// When `manifest` is `Some`, both `load` and `load_raw` re-hash the
    /// bytes returned by the underlying `AssetIo` against the recorded
    /// BLAKE3 digest and refuse to proceed on mismatch.
    pub fn new(
        index_bytes: &[u8],
        io: Box<dyn AssetIo>,
        metrics_registry: Arc<MetricsRegistry>,
        manifest: Option<PackManifest>,
    ) -> Result<Self> {
        let vfs = VirtualFileSystem::new(index_bytes)
            .context("Failed to initialize VirtualFileSystem from index bytes")?;

        Ok(Self {
            vfs,
            io,
            decoders: DecoderRegistry::new(metrics_registry),
            storages: HashMap::new(),
            load_count: 0,
            manifest,
        })
    }

    /// Returns a reference to the underlying VFS for metadata enumeration
    /// (asset browser) and direct UUID lookup.
    pub fn vfs(&self) -> &VirtualFileSystem {
        &self.vfs
    }

    /// Registers a decoder for a specific asset type.
    pub fn register_decoder<A: Asset>(
        &mut self,
        type_name: &str,
        decoder: impl super::decoder::AssetDecoder<A> + Send + Sync + 'static,
    ) {
        self.decoders.register::<A>(type_name, decoder);
    }

    /// Walks `inventory::iter::<DecoderRegistration>` and runs each entry's
    /// `register` fn. Use this once after construction to pull in every
    /// decoder declared via `inventory::submit!` — currently `texture` and
    /// `font`. Audio and mesh decoders are intentionally explicit (see
    /// `decoders/audio` and `decoders/mesh` for the rationale).
    pub fn register_inventory_decoders(&mut self) {
        for reg in inventory::iter::<super::registry::DecoderRegistration> {
            (reg.register)(self);
        }
    }

    /// Loads, decodes, and returns a typed handle to an asset.
    pub fn load<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<AssetHandle<A>> {
        let type_id = TypeId::of::<A>();

        // Get or create the typed storage. Inserts a fresh `Assets<A>` the
        // first time we see this `A`.
        let storage = self
            .storages
            .entry(type_id)
            .or_insert_with(|| Box::new(Assets::<A>::new()));

        let assets = storage
            .as_any_mut()
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

        let bytes = self.io.load_bytes(source)?;
        if let Some(m) = &self.manifest {
            m.verify(uuid, &bytes)
                .context("Asset integrity check failed")?;
        }
        let asset: A = self
            .decoders
            .decode::<A>(&metadata.asset_type_name, &bytes)?;

        let handle = AssetHandle::new(asset);
        assets.insert(*uuid, handle.clone());

        self.load_count += 1;
        Ok(handle)
    }

    /// Loads an asset's raw bytes via the VFS → IO path, skipping the
    /// decoder. Used by scene loading where the bytes are then decoded by a
    /// `SerializationService` strategy rather than an `AssetDecoder<A>`.
    pub fn load_raw(&mut self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        let metadata = self
            .vfs
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;
        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;
        let bytes = self.io.load_bytes(source)?;
        if let Some(m) = &self.manifest {
            m.verify(uuid, &bytes)
                .context("Asset integrity check failed")?;
        }
        Ok(bytes)
    }

    /// Drops the cached handle for `uuid` across every typed storage.
    /// Subsequent `load::<A>()` calls re-run the IO + decoder pipeline.
    /// Returns `true` if any storage held this UUID.
    ///
    /// Outstanding clones of the previously-cached `AssetHandle<A>` keep the
    /// old asset alive until they themselves drop — by design (in-flight
    /// readers don't see a half-loaded replacement).
    pub fn invalidate(&mut self, uuid: &AssetUUID) -> bool {
        let mut any = false;
        for storage in self.storages.values_mut() {
            if storage.remove_uuid(uuid) {
                any = true;
            }
        }
        any
    }

    /// Atomically replaces the inner `VirtualFileSystem` with a freshly
    /// decoded one. Cached handles are kept; callers should `invalidate`
    /// any UUID whose underlying bytes changed (the editor's hot-reload
    /// pump only invokes `reindex` when files were *added or removed* —
    /// pure in-place modifications go through `invalidate`).
    pub fn reindex(&mut self, index_bytes: &[u8]) -> Result<()> {
        let new_vfs = VirtualFileSystem::new(index_bytes)
            .context("Failed to decode replacement VFS index bytes")?;
        self.vfs = new_vfs;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::IndexBuilder;
    use khora_core::asset::AssetSource;
    use std::fs;
    use tempfile::tempdir;

    /// Mock IO that returns bytes from an in-memory map keyed by rel-path.
    /// Lets the service tests cover invalidate/reindex without touching real
    /// decoders (which need real format-specific bytes).
    struct MockIo {
        files: HashMap<std::path::PathBuf, Vec<u8>>,
    }
    impl AssetIo for MockIo {
        fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
            match source {
                AssetSource::Path(rel) => self
                    .files
                    .get(rel)
                    .cloned()
                    .ok_or_else(|| anyhow!("not in mock: {:?}", rel)),
                AssetSource::Packed { .. } => Err(anyhow!("mock doesn't support Packed")),
            }
        }
    }

    #[test]
    fn vfs_accessor_returns_underlying_vfs() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("textures")).unwrap();
        fs::write(dir.path().join("textures").join("a.png"), b"PNG").unwrap();
        let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        let metrics = Arc::new(MetricsRegistry::new());
        let svc = AssetService::new(
            &bytes,
            Box::new(MockIo {
                files: HashMap::new(),
            }),
            metrics,
            None,
        )
        .unwrap();
        assert_eq!(svc.vfs().asset_count(), 1);
    }

    #[test]
    fn load_raw_returns_bytes_without_decode() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("scenes")).unwrap();
        fs::write(dir.path().join("scenes").join("a.kscene"), b"SCN").unwrap();
        let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();

        let mut files = HashMap::new();
        files.insert(std::path::PathBuf::from("scenes/a.kscene"), b"SCN".to_vec());
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc =
            AssetService::new(&bytes, Box::new(MockIo { files }), metrics, None).unwrap();

        let uuid = AssetUUID::new_v5("scenes/a.kscene");
        let raw = svc.load_raw(&uuid).unwrap();
        assert_eq!(raw, b"SCN");
    }

    #[test]
    fn reindex_swaps_vfs() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("textures")).unwrap();
        fs::write(dir.path().join("textures").join("a.png"), b"PNG").unwrap();
        let bytes_a = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &bytes_a,
            Box::new(MockIo {
                files: HashMap::new(),
            }),
            metrics,
            None,
        )
        .unwrap();
        assert_eq!(svc.vfs().asset_count(), 1);

        // Add another asset, rebuild, reindex.
        fs::write(dir.path().join("textures").join("b.png"), b"PNG2").unwrap();
        let bytes_b = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        svc.reindex(&bytes_b).unwrap();
        assert_eq!(svc.vfs().asset_count(), 2);

        let new_uuid = AssetUUID::new_v5("textures/b.png");
        assert!(svc.vfs().get_metadata(&new_uuid).is_some());
    }

    #[test]
    fn invalidate_returns_false_when_nothing_cached() {
        let dir = tempdir().unwrap();
        let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &bytes,
            Box::new(MockIo {
                files: HashMap::new(),
            }),
            metrics,
            None,
        )
        .unwrap();
        assert!(!svc.invalidate(&AssetUUID::new_v5("missing")));
    }

    #[test]
    fn load_raw_verifies_against_manifest_and_rejects_corruption() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("scenes")).unwrap();
        let payload = b"SCENE-PAYLOAD";
        fs::write(dir.path().join("scenes").join("a.kscene"), payload).unwrap();
        let index_bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();

        let uuid = AssetUUID::new_v5("scenes/a.kscene");

        // Build a manifest matching the genuine payload.
        let mut manifest = PackManifest::new();
        manifest.insert(uuid, payload);

        // Mock returns the genuine payload — verification should pass.
        let mut files = HashMap::new();
        files.insert(
            std::path::PathBuf::from("scenes/a.kscene"),
            payload.to_vec(),
        );
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &index_bytes,
            Box::new(MockIo { files }),
            metrics,
            Some(manifest.clone()),
        )
        .unwrap();
        let raw = svc.load_raw(&uuid).unwrap();
        assert_eq!(raw, payload);

        // Now corrupt the bytes the mock returns. Same uuid, same manifest,
        // different bytes => verification must fail.
        let mut bad_files = HashMap::new();
        bad_files.insert(
            std::path::PathBuf::from("scenes/a.kscene"),
            b"CORRUPTED-PAY".to_vec(),
        );
        let metrics2 = Arc::new(MetricsRegistry::new());
        let mut svc_bad = AssetService::new(
            &index_bytes,
            Box::new(MockIo { files: bad_files }),
            metrics2,
            Some(manifest),
        )
        .unwrap();
        let err = svc_bad.load_raw(&uuid).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("integrity") || msg.contains("BLAKE3") || msg.contains("size mismatch"),
            "expected integrity error, got: {}",
            msg
        );
    }
}
