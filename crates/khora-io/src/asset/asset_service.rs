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
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{Asset, AssetHandle, AssetSource, AssetUUID};
use khora_core::event::{Channel, WhenFull};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_data::assets::{Assets, SoundData};
use khora_telemetry::MetricsRegistry;

use super::change::{AssetChanged, CHANGE_BACKLOG};
use super::container::AssetContainer;
use super::decoder_registry::DecoderRegistry;
use super::decoders::audio::SymphoniaDecoder;
use super::decoders::mesh::{FileSystemResolver, GltfContainer, MeshDispatcher};
use super::file::FileLoader;
use super::id_registry::AssetIdRegistry;
use super::index_builder::IndexBuilder;
use super::io::AssetIo;
use super::manifest::PackManifest;
use crate::asset::AssetIndex;

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
    index: AssetIndex,
    io: Box<dyn AssetIo>,
    decoders: DecoderRegistry,
    storages: HashMap<TypeId, Box<dyn AnyAssets>>,
    load_count: usize,
    /// When `Some`, every byte slice produced by `io.load_bytes` is
    /// re-hashed against the manifest before reaching the decoder.
    /// Constructed by the runtime only when `RuntimeConfig::verify_integrity`
    /// is set and `manifest.bin` is present next to `data.pack`.
    manifest: Option<PackManifest>,
    /// The containers that open files holding sub-assets, by the file's
    /// index type.
    containers: HashMap<String, Arc<dyn AssetContainer>>,
    /// Each opened container, by the container file's uuid: opened once,
    /// shared by every sub-asset extracted from it.
    opened: HashMap<AssetUUID, Arc<dyn Any + Send + Sync>>,
    /// Every sub-asset ever served from each container, by the container
    /// file's uuid. Never emptied: invalidating the container drops and
    /// announces each of them, even once a new index no longer lists them
    /// and even after a reload of the container failed.
    served_subs: HashMap<AssetUUID, HashSet<AssetUUID>>,
    /// Every asset [`load`](Self::load) ever decoded: what the engine may
    /// hold a copy of.
    served: HashSet<AssetUUID>,
    /// The assets whose bytes changed, for the engine to replace its copies.
    changes: Channel<AssetChanged>,
    /// Set when `changes` dropped an announcement it had no room for.
    changes_lost: AtomicBool,
    /// What the services this one replaced served and it has not served
    /// since: the engine may still hold their copies, which are no last good
    /// copy of this service's assets.
    inherited: Mutex<HashSet<AssetUUID>>,
    /// The files, by asset key, each opened container reads besides its
    /// own: a change to one of them is a change to the container. Kept
    /// across invalidations, like `served_subs`.
    external_files: HashMap<AssetUUID, Vec<String>>,
    /// The assets whose last [`load`](Self::load) failed. Forgotten as soon
    /// as any asset changes or the index is replaced: whatever made a load
    /// fail (a broken file, a missing part) can only be fixed by such a
    /// change, so it is the moment to try again.
    failed: HashSet<AssetUUID>,
}

/// Sends `AssetChanged(uuid)` on `changes`, noting in `lost` if the full
/// stream dropped one to make room.
fn announce(changes: &Channel<AssetChanged>, lost: &AtomicBool, uuid: AssetUUID) {
    let dropped = changes.dropped();
    changes.send(AssetChanged(uuid));
    if changes.dropped() != dropped {
        lost.store(true, Ordering::Relaxed);
    }
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
        let index = AssetIndex::new(index_bytes)
            .context("Failed to initialize AssetIndex from index bytes")?;

        Ok(Self {
            index,
            io,
            decoders: DecoderRegistry::new(metrics_registry),
            storages: HashMap::new(),
            load_count: 0,
            manifest,
            containers: HashMap::new(),
            opened: HashMap::new(),
            served_subs: HashMap::new(),
            served: HashSet::new(),
            changes: Channel::bounded(CHANGE_BACKLOG, WhenFull::DropOldest),
            changes_lost: AtomicBool::new(false),
            inherited: Mutex::new(HashSet::new()),
            external_files: HashMap::new(),
            failed: HashSet::new(),
        })
    }

    /// Returns a reference to the underlying VFS for metadata enumeration
    /// (asset browser) and direct UUID lookup.
    pub fn index(&self) -> &AssetIndex {
        &self.index
    }

    /// Registers a decoder for a specific asset type.
    pub fn register_decoder<A: Asset>(
        &mut self,
        type_name: &str,
        decoder: impl super::decoder::AssetDecoder<A> + Send + Sync + 'static,
    ) {
        self.decoders.register::<A>(type_name, decoder);
    }

    /// Whether the file `uuid` names can hold sub-assets (a glTF can): when
    /// its bytes change, its parts may have changed too, and the index must
    /// list them again.
    pub fn may_hold_sub_assets(&self, uuid: &AssetUUID) -> bool {
        self.index.get_metadata(uuid).is_some_and(|metadata| {
            super::container::type_may_hold_sub_assets(&metadata.asset_type_name)
        })
    }

    /// Registers the [`AssetContainer`](super::container::AssetContainer) that
    /// opens files of index type `type_name` and extracts their sub-assets.
    pub fn register_container(
        &mut self,
        type_name: &str,
        container: impl super::container::AssetContainer,
    ) {
        self.containers
            .insert(type_name.to_string(), Arc::new(container));
    }

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
        let (asset, container) = match self.decode_uncached::<A>(uuid) {
            Ok(decoded) => decoded,
            Err(e) => {
                self.failed.insert(*uuid);
                return Err(e);
            }
        };
        self.failed.remove(uuid);
        if let Some(container) = container {
            self.served_subs.entry(container).or_default().insert(*uuid);
        }
        self.served.insert(*uuid);
        self.inherited_mut().remove(uuid);
        let handle = AssetHandle::new(asset);
        self.storages
            .get_mut(&type_id)
            .and_then(|storage| storage.as_any_mut().downcast_mut::<Assets<A>>())
            .ok_or_else(|| anyhow!("Mismatched asset storage type"))?
            .insert(*uuid, handle.clone());

        self.load_count += 1;
        Ok(handle)
    }

    /// Decodes the asset `uuid` names: from its own bytes, or, for a
    /// sub-asset, extracted from its opened container, which it returns too.
    fn decode_uncached<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<(A, Option<AssetUUID>)> {
        let metadata = self
            .index
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;
        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;

        if let AssetSource::Sub { container, label } = source {
            let (container, label) = (*container, label.clone());
            let type_name = metadata.asset_type_name.clone();
            return self
                .extract_sub::<A>(container, &label, &type_name)
                .map(|asset| (asset, Some(container)));
        }

        let bytes = self.io.load_bytes(source)?;
        if let Some(m) = &self.manifest {
            m.verify(uuid, &bytes)
                .context("Asset integrity check failed")?;
        }
        let asset = self.decoders.decode_at::<A>(
            &metadata.asset_type_name,
            &bytes,
            &metadata.source_path,
        )?;
        // A container file decoded whole (a glTF as one mesh) reads the same
        // files beside it as its parts do: a change to one of them is a
        // change to it.
        if let Some(opener) = self.containers.get(&metadata.asset_type_name) {
            if let Ok(opened) = opener.open(&bytes, &metadata.source_path) {
                self.external_files
                    .insert(*uuid, opener.external_files(opened.as_ref()));
            }
        }
        Ok((asset, None))
    }

    /// The container file `container` and what opens it, opened once and
    /// cached until it is invalidated. Each opening records the other files
    /// it reads.
    fn open_container(
        &mut self,
        container: AssetUUID,
    ) -> Result<(Arc<dyn AssetContainer>, Arc<dyn Any + Send + Sync>)> {
        let container_meta = self
            .index
            .get_metadata(&container)
            .ok_or_else(|| anyhow!("its container {container} is not in the VFS"))?;
        let opener = self
            .containers
            .get(&container_meta.asset_type_name)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "no container registered for '{}' files",
                    container_meta.asset_type_name
                )
            })?;
        if let Some(opened) = self.opened.get(&container) {
            return Ok((opener, opened.clone()));
        }

        let source = container_meta
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("container {container} has no 'default' variant"))?;
        let bytes = self.io.load_bytes(source)?;
        if let Some(m) = &self.manifest {
            m.verify(&container, &bytes)
                .context("Asset integrity check failed")?;
        }
        let opened = opener
            .open(&bytes, &container_meta.source_path)
            .with_context(|| format!("opening container {container}"))?;
        self.external_files
            .insert(container, opener.external_files(opened.as_ref()));
        self.opened.insert(container, opened.clone());
        Ok((opener, opened))
    }

    /// Extracts the sub-asset `label` (index type `type_name`) from the
    /// container file `container`, opening the container on first use.
    fn extract_sub<A: Asset>(
        &mut self,
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<A> {
        let (opener, opened) = self
            .open_container(container)
            .with_context(|| format!("sub-asset '{label}'"))?;
        let extracted = opener
            .extract(opened.as_ref(), container, label, type_name)
            .with_context(|| format!("extracting '{label}' from {container}"))?;
        extracted.downcast::<A>().map(|asset| *asset).map_err(|_| {
            anyhow!(
                "sub-asset '{label}' of {container} is a '{type_name}', not a {}",
                std::any::type_name::<A>()
            )
        })
    }

    /// Loads an asset's raw bytes via the VFS → IO path, skipping the
    /// decoder. Used by scene loading where the bytes are then decoded by a
    /// `SerializationService` strategy rather than an `AssetDecoder<A>`.
    pub fn load_raw(&mut self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        let metadata = self
            .index
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;
        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;
        if let AssetSource::Sub { container, label } = source {
            return Err(anyhow!(
                "sub-asset '{label}' of {container} has no bytes of its own"
            ));
        }
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
    ///
    /// Invalidating a container file also drops its opened container and
    /// every sub-asset extracted from it.
    ///
    /// `uuid` and every sub-asset it served are announced on
    /// [`changes`](Self::changes) whether or not anything was cached: after
    /// a reload that failed, the service holds nothing while the engine
    /// still holds the last good copy, which the next change must reach.
    ///
    /// A container that reads `uuid`'s file besides its own (a glTF's `.bin`
    /// or image beside it) is invalidated with it.
    pub fn invalidate(&mut self, uuid: &AssetUUID) -> bool {
        self.failed.clear();
        let mut any = false;
        let mut pending = vec![*uuid];
        let mut seen = HashSet::new();
        while let Some(file) = pending.pop() {
            if !seen.insert(file) {
                continue;
            }
            any |= self.invalidate_file(&file);
            // A file the index does not list yet (created since the last
            // reindex) is known only by its uuid, the one its path gives.
            let key = self
                .index
                .get_metadata(&file)
                .map(|metadata| khora_core::asset::asset_key(&metadata.source_path));
            let names_file = |external: &String| {
                key.as_ref() == Some(external) || AssetUUID::new_v5(external) == file
            };
            pending.extend(
                self.external_files
                    .iter()
                    .filter(|(_, files)| files.iter().any(names_file))
                    .map(|(container, _)| *container),
            );
        }
        any
    }

    /// Drops and announces `uuid` and every sub-asset it served.
    fn invalidate_file(&mut self, uuid: &AssetUUID) -> bool {
        let mut any = self.opened.remove(uuid).is_some();
        let subs = self.served_subs.get(uuid);
        for target in std::iter::once(uuid).chain(subs.into_iter().flatten()) {
            for storage in self.storages.values_mut() {
                if storage.remove_uuid(target) {
                    any = true;
                }
            }
            announce(&self.changes, &self.changes_lost, *target);
        }
        any
    }

    /// Whether `uuid` is no longer part of the project: it is not indexed
    /// and, for a sub-asset, its container is gone too, or opens and no
    /// longer holds it.
    ///
    /// A sub-asset whose container is indexed but does not open (a file
    /// caught mid-write, indexed without its parts) is not gone: its file
    /// is read again when it changes.
    pub fn is_gone(&mut self, uuid: &AssetUUID) -> bool {
        if self.index.get_metadata(uuid).is_some() {
            return false;
        }
        let container = self
            .served_subs
            .iter()
            .find(|(_, subs)| subs.contains(uuid))
            .map(|(container, _)| *container);
        let Some(container) = container else {
            return true;
        };
        if self.index.get_metadata(&container).is_none() {
            return true;
        }
        self.open_container(container).is_ok()
    }

    /// Serves `handle` for `uuid` until `uuid` is invalidated again: the
    /// last good copy of an asset whose current bytes did not decode, so a
    /// reference resolved meanwhile shows what the engine already shows.
    ///
    /// Refused (returns `false`) for a copy a replaced service served, which
    /// is no copy of this service's asset.
    pub fn keep<A: Asset>(&mut self, uuid: &AssetUUID, handle: AssetHandle<A>) -> bool {
        if self.inherited_mut().contains(uuid) {
            return false;
        }
        let storage = self
            .storages
            .entry(TypeId::of::<A>())
            .or_insert_with(|| Box::new(Assets::<A>::new()));
        if let Some(assets) = storage.as_any_mut().downcast_mut::<Assets<A>>() {
            assets.insert(*uuid, handle);
            self.served.insert(*uuid);
            return true;
        }
        false
    }

    /// The inherited set, poisoning ignored: it holds plain uuids.
    fn inherited_mut(&self) -> std::sync::MutexGuard<'_, HashSet<AssetUUID>> {
        self.inherited
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes every asset announced since the last call, for the engine to
    /// replace its copies. If the stream overflowed since, the announcements
    /// it lost cannot be told apart, so every asset this service ever served
    /// is returned.
    ///
    /// The stream's single consumer is the `asset_reload` DataSystem.
    pub fn take_changes(&self) -> HashSet<AssetUUID> {
        let mut changed: HashSet<AssetUUID> = self
            .changes
            .drain()
            .into_iter()
            .map(|AssetChanged(uuid)| uuid)
            .collect();
        if self.changes_lost.swap(false, Ordering::Relaxed) {
            log::warn!(
                "AssetService: more than {CHANGE_BACKLOG} assets changed at once; every asset \
                 served is reloaded"
            );
            changed.extend(self.served.iter().copied());
            changed.extend(self.inherited_mut().iter().copied());
        }
        changed
    }

    /// Atomically replaces the inner `AssetIndex` with a freshly
    /// decoded one. Cached handles are kept; callers should `invalidate`
    /// any UUID whose underlying bytes changed (the editor's hot-reload
    /// pump only invokes `reindex` when files were *added or removed* —
    /// pure in-place modifications go through `invalidate`).
    pub fn reindex(&mut self, index_bytes: &[u8]) -> Result<()> {
        let new_index =
            AssetIndex::new(index_bytes).context("Failed to decode replacement VFS index bytes")?;
        self.index = new_index;
        self.failed.clear();
        Ok(())
    }

    /// The stream of assets whose bytes changed: every [`invalidate`](Self::invalidate)
    /// sends the invalidated uuid and each part it served.
    ///
    /// Drained by its single consumer, the `asset_reload` DataSystem.
    pub fn changes(&self) -> &Channel<AssetChanged> {
        &self.changes
    }

    /// Announces, on this service's [`changes`](Self::changes), every asset
    /// `outgoing` ever served: this service replaces it, so the copies the
    /// engine took from it may be stale.
    ///
    /// What `outgoing` served (and what it inherited in turn) is inherited:
    /// a stream overflow still names it, and it is never kept as a last good
    /// copy of this service's asset.
    pub fn announce_replacing(&self, outgoing: &AssetService) {
        let mut replaced: HashSet<AssetUUID> = outgoing.inherited_mut().clone();
        replaced.extend(outgoing.served.iter().copied());
        for uuid in &replaced {
            announce(&self.changes, &self.changes_lost, *uuid);
        }
        let mut inherited = self.inherited_mut();
        inherited.extend(
            replaced
                .into_iter()
                .filter(|uuid| !self.served.contains(uuid)),
        );
    }

    /// Whether the last [`load`](Self::load) of `uuid` failed and nothing
    /// changed since (no asset invalidated, no new index): loading it again
    /// would fail the same way. Callers that resolve references every tick
    /// skip such an asset instead of reading it again.
    pub fn has_failed(&self, uuid: &AssetUUID) -> bool {
        self.failed.contains(uuid)
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
                // A sub-asset has no bytes of its own: the service reads its
                // container's source instead and never hands a `Sub` to the IO.
                AssetSource::Sub { .. } => {
                    panic!("the service handed a Sub source to its AssetIo: {source:?}")
                }
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
        assert_eq!(svc.index().asset_count(), 1);
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
        let mut svc = AssetService::new(&bytes, Box::new(MockIo { files }), metrics, None).unwrap();

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
        assert_eq!(svc.index().asset_count(), 1);

        // Add another asset, rebuild, reindex.
        fs::write(dir.path().join("textures").join("b.png"), b"PNG2").unwrap();
        let bytes_b = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        svc.reindex(&bytes_b).unwrap();
        assert_eq!(svc.index().asset_count(), 2);

        let new_uuid = AssetUUID::new_v5("textures/b.png");
        assert!(svc.index().get_metadata(&new_uuid).is_some());
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
    fn a_failed_load_is_remembered_until_an_asset_changes_or_the_index_is_replaced() {
        let dir = tempdir().unwrap();
        let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
        let mut svc = AssetService::new(
            &bytes,
            Box::new(MockIo {
                files: HashMap::new(),
            }),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .unwrap();
        let missing = AssetUUID::new_v5("missing");
        assert!(!svc.has_failed(&missing), "never tried");

        assert!(svc
            .load::<khora_core::renderer::api::resource::CpuTexture>(&missing)
            .is_err());
        assert!(svc.has_failed(&missing));

        svc.invalidate(&AssetUUID::new_v5("another"));
        assert!(!svc.has_failed(&missing), "any change forgets the failure");

        assert!(svc
            .load::<khora_core::renderer::api::resource::CpuTexture>(&missing)
            .is_err());
        svc.reindex(&bytes).unwrap();
        assert!(!svc.has_failed(&missing), "a new index forgets the failure");
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

    // ── Sub-assets ──────────────────────────────────────────────────────────

    use crate::asset::AssetContainer;
    use khora_core::asset::AssetMetadata;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Index type of the test container files.
    const CONTAINER_TYPE: &str = "ksubtest";

    /// What a test container extracts as a `"mesh"`.
    #[derive(Debug)]
    struct TestMesh {
        container: AssetUUID,
        label: String,
        source_path: std::path::PathBuf,
        text: String,
    }
    impl Asset for TestMesh {}

    /// What a test container extracts as a `"material"`.
    #[derive(Debug)]
    struct TestMaterial {
        label: String,
    }
    impl Asset for TestMaterial {}

    /// What the test container's `open` produces.
    struct OpenedTestContainer {
        text: String,
        source_path: std::path::PathBuf,
    }

    /// A container that counts how many times it was opened.
    struct CountingContainer {
        opens: Arc<AtomicUsize>,
    }

    impl AssetContainer for CountingContainer {
        fn open(&self, bytes: &[u8], source_path: &Path) -> Result<Arc<dyn Any + Send + Sync>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(OpenedTestContainer {
                text: String::from_utf8(bytes.to_vec())?,
                source_path: source_path.to_path_buf(),
            }))
        }

        fn extract(
            &self,
            opened: &(dyn Any + Send + Sync),
            container: AssetUUID,
            label: &str,
            type_name: &str,
        ) -> Result<Box<dyn Any + Send>> {
            let opened = opened
                .downcast_ref::<OpenedTestContainer>()
                .ok_or_else(|| anyhow!("not an opened test container"))?;
            match type_name {
                "mesh" => Ok(Box::new(TestMesh {
                    container,
                    label: label.to_string(),
                    source_path: opened.source_path.clone(),
                    text: opened.text.clone(),
                })),
                "material" => Ok(Box::new(TestMaterial {
                    label: label.to_string(),
                })),
                other => Err(anyhow!("the test container holds no '{other}'")),
            }
        }
    }

    const HERO_PATH: &str = "models/hero.ksubtest";
    const HERO_BYTES: &[u8] = b"HERO-CONTAINER";
    const VILLAIN_PATH: &str = "models/villain.ksubtest";
    const VILLAIN_BYTES: &[u8] = b"VILLAIN-CONTAINER";

    fn hero() -> AssetUUID {
        AssetUUID::new_v5(HERO_PATH)
    }
    fn villain() -> AssetUUID {
        AssetUUID::new_v5(VILLAIN_PATH)
    }
    // The service reads a sub's container and label from its `Sub` source, so
    // these uuids need not be derived through `AssetUUID::sub_asset`.
    fn hero_body() -> AssetUUID {
        AssetUUID::new_v5("test-sub/hero/mesh/Body")
    }
    fn hero_skin() -> AssetUUID {
        AssetUUID::new_v5("test-sub/hero/material/Skin")
    }
    fn villain_body() -> AssetUUID {
        AssetUUID::new_v5("test-sub/villain/mesh/Body")
    }

    fn file_entry(uuid: AssetUUID, rel: &str) -> AssetMetadata {
        let mut variants = HashMap::new();
        variants.insert(
            "default".to_string(),
            AssetSource::Path(std::path::PathBuf::from(rel)),
        );
        AssetMetadata {
            uuid,
            source_path: std::path::PathBuf::from(rel),
            asset_type_name: CONTAINER_TYPE.to_string(),
            dependencies: Vec::new(),
            variants,
            tags: Vec::new(),
        }
    }

    fn sub_entry(
        uuid: AssetUUID,
        container: AssetUUID,
        rel: &str,
        label: &str,
        type_name: &str,
    ) -> AssetMetadata {
        let mut variants = HashMap::new();
        variants.insert(
            "default".to_string(),
            AssetSource::Sub {
                container,
                label: label.to_string(),
            },
        );
        AssetMetadata {
            uuid,
            source_path: std::path::PathBuf::from(rel),
            asset_type_name: type_name.to_string(),
            dependencies: Vec::new(),
            variants,
            tags: Vec::new(),
        }
    }

    /// An index holding two container files and their subs, as the index
    /// builder writes it.
    fn container_index_bytes() -> Vec<u8> {
        let metadata = vec![
            file_entry(hero(), HERO_PATH),
            sub_entry(hero_body(), hero(), HERO_PATH, "mesh/Body", "mesh"),
            sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
            file_entry(villain(), VILLAIN_PATH),
            sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
        ];
        bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap()
    }

    fn container_files(hero_bytes: &[u8]) -> HashMap<std::path::PathBuf, Vec<u8>> {
        let mut files = HashMap::new();
        files.insert(std::path::PathBuf::from(HERO_PATH), hero_bytes.to_vec());
        files.insert(
            std::path::PathBuf::from(VILLAIN_PATH),
            VILLAIN_BYTES.to_vec(),
        );
        files
    }

    /// A service over the container index with a counting container
    /// registered; returns the open counter.
    fn container_service(
        hero_bytes: &[u8],
        manifest: Option<PackManifest>,
    ) -> (AssetService, Arc<AtomicUsize>) {
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &container_index_bytes(),
            Box::new(MockIo {
                files: container_files(hero_bytes),
            }),
            metrics,
            manifest,
        )
        .unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        svc.register_container(
            CONTAINER_TYPE,
            CountingContainer {
                opens: opens.clone(),
            },
        );
        (svc, opens)
    }

    #[test]
    fn loading_a_sub_returns_the_value_its_container_extracts() {
        let (mut svc, _) = container_service(HERO_BYTES, None);
        let body = svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(body.label, "mesh/Body");
        assert_eq!(body.container, hero());
        assert_eq!(body.source_path, std::path::PathBuf::from(HERO_PATH));
        assert_eq!(body.text.as_bytes(), HERO_BYTES);

        let skin = svc.load::<TestMaterial>(&hero_skin()).unwrap();
        assert_eq!(skin.label, "material/Skin");
    }

    #[test]
    fn loading_two_subs_of_one_container_opens_it_once() {
        let (mut svc, opens) = container_service(HERO_BYTES, None);
        svc.load::<TestMesh>(&hero_body()).unwrap();
        svc.load::<TestMaterial>(&hero_skin()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1, "one open for both subs");

        // A second load of a sub is served from the cache.
        let first = svc.load::<TestMesh>(&hero_body()).unwrap();
        let again = svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(first, again, "a loaded sub is cached under its uuid");
        assert_eq!(opens.load(Ordering::SeqCst), 1);

        // Another container is opened on its own.
        svc.load::<TestMesh>(&villain_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn loading_a_sub_as_the_wrong_type_errors() {
        let (mut svc, _) = container_service(HERO_BYTES, None);
        let err = svc
            .load::<TestMaterial>(&hero_body())
            .expect_err("a mesh sub cannot load as a material");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("TestMaterial") || msg.contains("mesh"),
            "the error names the type: {msg}"
        );
        // The sub still loads as its own type afterwards.
        let body = svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(body.label, "mesh/Body");
    }

    /// A sub has no bytes of its own: integrity is checked on the container's
    /// bytes, under the container's uuid (the pack manifest has no sub entry).
    #[test]
    fn a_sub_is_verified_against_the_manifest_under_its_container_uuid() {
        let mut manifest = PackManifest::new();
        manifest.insert(hero(), HERO_BYTES);
        manifest.insert(villain(), VILLAIN_BYTES);

        let (mut svc, _) = container_service(HERO_BYTES, Some(manifest.clone()));
        let body = svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(body.text.as_bytes(), HERO_BYTES);

        let (mut svc_bad, opens) = container_service(b"TAMPERED-HERO!", Some(manifest));
        let err = svc_bad
            .load::<TestMesh>(&hero_body())
            .expect_err("tampered container bytes must fail the integrity check");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("integrity") || msg.contains("BLAKE3") || msg.contains("size mismatch"),
            "expected integrity error, got: {msg}"
        );
        assert_eq!(
            opens.load(Ordering::SeqCst),
            0,
            "tampered bytes never reach the container"
        );
    }

    #[test]
    fn load_raw_of_a_sub_errors() {
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &container_index_bytes(),
            Box::new(MockIo {
                files: container_files(HERO_BYTES),
            }),
            metrics,
            None,
        )
        .unwrap();
        let err = svc
            .load_raw(&hero_body())
            .expect_err("a sub-asset has no bytes of its own");
        let msg = format!("{err:#}");
        assert!(msg.contains("sub-asset"), "unexpected error: {msg}");
        // The container's own bytes are still readable.
        assert_eq!(svc.load_raw(&hero()).unwrap(), HERO_BYTES);
    }

    #[test]
    fn invalidating_the_container_drops_its_opened_container_and_subs() {
        let (mut svc, opens) = container_service(HERO_BYTES, None);
        let before = svc.load::<TestMesh>(&hero_body()).unwrap();
        svc.load::<TestMesh>(&villain_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 2);

        assert!(
            svc.invalidate(&hero()),
            "invalidating a container with cached subs drops something"
        );

        // The hero is opened again; its sub is a fresh value.
        let after = svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 3, "the container re-opens");
        assert_ne!(before, after, "the cached sub was dropped");

        // The villain's sub and opened container are untouched.
        svc.load::<TestMesh>(&villain_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn invalidating_an_opened_container_without_cached_subs_returns_true() {
        let (mut svc, opens) = container_service(HERO_BYTES, None);
        svc.load::<TestMesh>(&hero_body()).unwrap();
        // Drop the sub alone: the opened container stays cached.
        assert!(svc.invalidate(&hero_body()));
        svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1, "the container stays open");
        assert!(svc.invalidate(&hero_body()));

        // Only the opened container is left: dropping it is still a drop.
        assert!(svc.invalidate(&hero()));
        assert!(
            !svc.invalidate(&hero()),
            "nothing is left to drop the second time"
        );
        svc.load::<TestMesh>(&hero_body()).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 2, "the container re-opens");
    }

    /// Subs of one container cached under different Rust types are all
    /// dropped with it.
    #[test]
    fn invalidating_the_container_drops_its_subs_of_every_type() {
        let (mut svc, _) = container_service(HERO_BYTES, None);
        let body = svc.load::<TestMesh>(&hero_body()).unwrap();
        let skin = svc.load::<TestMaterial>(&hero_skin()).unwrap();

        assert!(svc.invalidate(&hero()));

        assert_ne!(
            body,
            svc.load::<TestMesh>(&hero_body()).unwrap(),
            "the cached mesh sub was dropped"
        );
        assert_ne!(
            skin,
            svc.load::<TestMaterial>(&hero_skin()).unwrap(),
            "the cached material sub was dropped"
        );
    }

    /// A re-export can remove a sub-asset: the rebuilt index no longer lists
    /// it. Invalidating the container afterwards must still drop the sub
    /// extracted from the old container — it is a sub of that container
    /// whatever the new index says — so it is no longer served.
    #[test]
    fn invalidating_a_container_after_a_reindex_drops_a_sub_the_new_index_dropped() {
        let (mut svc, _) = container_service(HERO_BYTES, None);
        svc.load::<TestMesh>(&hero_body()).unwrap();

        // The re-exported hero holds the skin only.
        let reexported = vec![
            file_entry(hero(), HERO_PATH),
            sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
            file_entry(villain(), VILLAIN_PATH),
            sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
        ];
        svc.reindex(
            &bincode::serde::encode_to_vec(&reexported, bincode::config::standard()).unwrap(),
        )
        .unwrap();
        svc.invalidate(&hero());

        assert!(
            svc.load::<TestMesh>(&hero_body()).is_err(),
            "a sub of the invalidated container that the index no longer lists \
             must not be served from the cache"
        );
    }

    #[test]
    fn loading_a_sub_without_a_registered_container_errors() {
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &container_index_bytes(),
            Box::new(MockIo {
                files: container_files(HERO_BYTES),
            }),
            metrics,
            None,
        )
        .unwrap();
        assert!(
            svc.load::<TestMesh>(&hero_body()).is_err(),
            "no container opens this file type"
        );
    }

    #[test]
    fn loading_a_sub_whose_container_is_not_indexed_errors() {
        let orphan = AssetUUID::new_v5("test-sub/orphan/mesh/Body");
        let metadata = vec![sub_entry(
            orphan,
            AssetUUID::new_v5("models/gone.ksubtest"),
            "models/gone.ksubtest",
            "mesh/Body",
            "mesh",
        )];
        let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(
            &index,
            Box::new(MockIo {
                files: HashMap::new(),
            }),
            metrics,
            None,
        )
        .unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        svc.register_container(
            CONTAINER_TYPE,
            CountingContainer {
                opens: opens.clone(),
            },
        );
        assert!(svc.load::<TestMesh>(&orphan).is_err());
        assert_eq!(opens.load(Ordering::SeqCst), 0);
    }

    // ── Change announcements ────────────────────────────────────────────────

    use crate::asset::{AssetChanged, AssetDecoder};
    use std::sync::Mutex;

    /// Mock IO over a file map the test keeps a handle to, so it can rewrite
    /// a file's bytes after the service took the IO.
    struct SharedIo {
        files: Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>>,
    }
    impl AssetIo for SharedIo {
        fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
            match source {
                AssetSource::Path(rel) => self
                    .files
                    .lock()
                    .unwrap()
                    .get(rel)
                    .cloned()
                    .ok_or_else(|| anyhow!("not in mock: {:?}", rel)),
                other => Err(anyhow!("the shared mock only serves paths: {other:?}")),
            }
        }
    }

    /// Index type of the test's plain (non-container) files.
    const NOTE_TYPE: &str = "ktestnote";
    const NOTE_PATH: &str = "notes/readme.ktestnote";

    fn note() -> AssetUUID {
        AssetUUID::new_v5(NOTE_PATH)
    }

    /// What the note decoder produces: the file's text.
    #[derive(Debug)]
    struct TestNote {
        #[allow(dead_code)]
        text: String,
    }
    impl Asset for TestNote {}

    struct NoteDecoder;
    impl AssetDecoder<TestNote> for NoteDecoder {
        fn load(
            &self,
            bytes: &[u8],
        ) -> std::result::Result<TestNote, Box<dyn std::error::Error + Send + Sync>> {
            Ok(TestNote {
                text: String::from_utf8(bytes.to_vec())?,
            })
        }
    }

    /// A service over the container index plus one plain note, reading its
    /// bytes from `files`, with the counting container and the note decoder
    /// registered.
    fn shared_service(files: Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>>) -> AssetService {
        let mut note_entry = file_entry(note(), NOTE_PATH);
        note_entry.asset_type_name = NOTE_TYPE.to_string();
        let metadata = vec![
            file_entry(hero(), HERO_PATH),
            sub_entry(hero_body(), hero(), HERO_PATH, "mesh/Body", "mesh"),
            sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
            file_entry(villain(), VILLAIN_PATH),
            sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
            note_entry,
        ];
        let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
        let mut svc = AssetService::new(
            &index,
            Box::new(SharedIo { files }),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .unwrap();
        svc.register_container(
            CONTAINER_TYPE,
            CountingContainer {
                opens: Arc::new(AtomicUsize::new(0)),
            },
        );
        svc.register_decoder::<TestNote>(NOTE_TYPE, NoteDecoder);
        svc
    }

    fn shared_files() -> Arc<Mutex<HashMap<std::path::PathBuf, Vec<u8>>>> {
        let mut files = container_files(HERO_BYTES);
        files.insert(std::path::PathBuf::from(NOTE_PATH), b"hello".to_vec());
        Arc::new(Mutex::new(files))
    }

    fn drained_set(svc: &AssetService) -> HashSet<AssetUUID> {
        svc.changes().drain().into_iter().map(|c| c.0).collect()
    }

    #[test]
    fn invalidating_an_asset_announces_it() {
        let mut svc = shared_service(shared_files());
        svc.load::<TestNote>(&note()).unwrap();

        svc.invalidate(&note());

        assert_eq!(svc.changes().drain(), vec![AssetChanged(note())]);
    }

    #[test]
    fn invalidating_an_asset_nothing_cached_still_announces_it() {
        let mut svc = shared_service(shared_files());

        assert!(
            !svc.invalidate(&note()),
            "a never-loaded asset has nothing to drop"
        );

        assert_eq!(
            svc.changes().drain(),
            vec![AssetChanged(note())],
            "a copy of it may still live outside the service"
        );
    }

    #[test]
    fn two_invalidations_before_a_drain_announce_once() {
        let mut svc = shared_service(shared_files());
        svc.load::<TestNote>(&note()).unwrap();

        svc.invalidate(&note());
        svc.invalidate(&note());

        assert_eq!(svc.changes().drain(), vec![AssetChanged(note())]);
    }

    #[test]
    fn invalidating_a_container_announces_each_part_it_served() {
        let mut svc = shared_service(shared_files());
        svc.load::<TestMesh>(&hero_body()).unwrap();
        svc.load::<TestMaterial>(&hero_skin()).unwrap();
        svc.load::<TestMesh>(&villain_body()).unwrap();

        svc.invalidate(&hero());

        let announced = svc.changes().drain();
        assert_eq!(announced.len(), 3, "no uuid twice: {announced:?}");
        assert_eq!(
            announced.into_iter().map(|c| c.0).collect::<HashSet<_>>(),
            HashSet::from([hero(), hero_body(), hero_skin()]),
            "the container and each of its parts, not another container's"
        );
    }

    /// A reload that fails leaves the parts unserved for a moment; the next
    /// invalidation of the container must still name them, since the engine
    /// still holds the copies it took before.
    #[test]
    fn a_container_still_announces_its_parts_after_a_failed_reload() {
        let files = shared_files();
        let mut svc = shared_service(files.clone());
        svc.load::<TestMesh>(&hero_body()).unwrap();
        svc.invalidate(&hero());
        assert_eq!(
            drained_set(&svc),
            HashSet::from([hero(), hero_body()]),
            "the first invalidation names the container and its part"
        );

        // The re-exported file is broken: the container no longer opens.
        files
            .lock()
            .unwrap()
            .insert(std::path::PathBuf::from(HERO_PATH), vec![0xFF, 0xFE, 0xFD]);
        assert!(svc.load::<TestMesh>(&hero_body()).is_err());

        svc.invalidate(&hero());

        assert_eq!(
            drained_set(&svc),
            HashSet::from([hero(), hero_body()]),
            "the part is announced again after the failed reload"
        );
    }

    #[test]
    fn announce_replacing_names_every_asset_the_outgoing_service_served() {
        let mut outgoing = shared_service(shared_files());
        outgoing.load::<TestNote>(&note()).unwrap();
        outgoing.load::<TestMesh>(&hero_body()).unwrap();

        let incoming = shared_service(shared_files());
        incoming.announce_replacing(&outgoing);

        assert_eq!(
            drained_set(&incoming),
            HashSet::from([note(), hero_body()]),
            "the incoming service announces what the outgoing one served"
        );
        assert!(
            outgoing.changes().drain().is_empty(),
            "the outgoing service's own stream is not the one written to"
        );
    }

    /// A project that served more assets than the stream holds is replaced:
    /// the announcements that do not fit are lost, and what the incoming
    /// service hands the engine must still name every asset the outgoing one
    /// served, since the engine holds a copy of each.
    #[test]
    fn a_replacement_overflowing_the_stream_still_names_everything_the_outgoing_service_served() {
        let count = CHANGE_BACKLOG + 1;
        let paths: Vec<String> = (0..count).map(|i| format!("notes/{i}.ktestnote")).collect();
        let mut files = HashMap::new();
        let metadata: Vec<AssetMetadata> = paths
            .iter()
            .map(|path| {
                files.insert(std::path::PathBuf::from(path), b"note".to_vec());
                let mut entry = file_entry(AssetUUID::new_v5(path), path);
                entry.asset_type_name = NOTE_TYPE.to_string();
                entry
            })
            .collect();
        let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
        let mut outgoing = AssetService::new(
            &index,
            Box::new(SharedIo {
                files: Arc::new(Mutex::new(files)),
            }),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .unwrap();
        outgoing.register_decoder::<TestNote>(NOTE_TYPE, NoteDecoder);
        for path in &paths {
            outgoing.load::<TestNote>(&AssetUUID::new_v5(path)).unwrap();
        }

        let incoming = shared_service(shared_files());
        incoming.announce_replacing(&outgoing);
        let taken = incoming.take_changes();

        let missing: Vec<&String> = paths
            .iter()
            .filter(|path| !taken.contains(&AssetUUID::new_v5(path)))
            .collect();
        assert!(
            missing.is_empty(),
            "every asset the outgoing service served is handed over: {} of {count} missing, \
             e.g. {:?}",
            missing.len(),
            missing.first()
        );
    }
}
