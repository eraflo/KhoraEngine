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
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{Asset, AssetHandle, AssetSource, AssetUUID};
use khora_core::event::{Channel, WhenFull};
use khora_data::assets::Assets;
use khora_telemetry::MetricsRegistry;

use super::change::{AssetChanged, CHANGE_BACKLOG};
use super::decoder_registry::DecoderRegistry;
use super::file::FileLoader;
use super::generations::Generations;
use super::io::AssetIo;
use super::manifest::PackManifest;
use super::reader::AssetReader;
use khora_core::asset::DecodeRequest;

use crate::asset::AssetIndex;

use any_assets::AnyAssets;

mod any_assets;
mod invalidation;
mod sub_assets;

/// How many decode requests wait for the `loading` family before a new one is
/// refused — and asked for again on a later frame.
pub(super) const REQUEST_BACKLOG: usize = 4096;

/// The asset management service.
///
/// Provides on-demand asset loading through a VFS → IO → Decode → Store pipeline.
/// Registered in `ServiceRegistry` and accessed by game code via `AppContext`.
pub struct AssetService {
    /// What decoding reads — the index, the IO, the decoders, the
    /// containers, the manifest — shared with whoever decodes off this
    /// service's lock. Replaced, never changed in place, while a reader is
    /// out: a reader keeps the index it was taken with.
    pub(super) reader: Arc<AssetReader>,
    pub(super) storages: HashMap<TypeId, Box<dyn AnyAssets>>,
    pub(super) load_count: usize,
    /// Each opened container, by the container file's uuid: opened once,
    /// shared by every sub-asset extracted from it.
    opened: HashMap<AssetUUID, Arc<dyn Any + Send + Sync>>,
    /// Every sub-asset ever served from each container, by the container
    /// file's uuid. Never emptied: invalidating the container drops and
    /// announces each of them, even once a new index no longer lists them
    /// and even after a reload of the container failed.
    pub(super) served_subs: HashMap<AssetUUID, HashSet<AssetUUID>>,
    /// Every asset [`load`](Self::load) ever decoded: what the engine may
    /// hold a copy of.
    pub(super) served: HashSet<AssetUUID>,
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
    pub(super) external_files: HashMap<AssetUUID, Vec<String>>,
    /// The assets whose last [`load`](Self::load) failed. Forgotten as soon
    /// as any asset changes or the index is replaced: whatever made a load
    /// fail (a broken file, a missing part) can only be fixed by such a
    /// change, so it is the moment to try again.
    pub(super) failed: HashSet<AssetUUID>,
    /// Where [`request`](Self::request) files a decode for the `loading` family
    /// to take off the frame.
    pub(super) requests: Channel<DecodeRequest>,
    /// The assets, by uuid and type, a decode is filed for and not taken in:
    /// asked for again meanwhile, they file nothing more.
    pub(super) pending: HashSet<(AssetUUID, TypeId)>,
    /// How each type a request named goes into the caches once decoded.
    pub(super) installers: HashMap<TypeId, super::off_frame::Installer>,
    /// Each asset's generation, which a decode in flight holds to.
    pub(super) generations: Arc<Generations>,
    /// The changed assets whose new value is still being decoded, for the
    /// `asset_reload` system to refresh their copies once it lands.
    reloading: HashSet<AssetUUID>,
    /// The last good copy of each asset being decoded again, by asset and
    /// type: what a reference resolved meanwhile is served, as the copies
    /// the engine holds are. Never in the cache, so the new value replaces
    /// it when it lands.
    pub(super) meanwhile: HashMap<(AssetUUID, TypeId), Box<dyn Any + Send + Sync>>,
    /// The project whose loose `assets/` folder it serves, when it serves
    /// one: what a project index filed off the frame must be for to be
    /// taken in. `None` for a pack.
    project_root: Option<PathBuf>,
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
        Ok(Self::over_index(index, io, metrics_registry, manifest))
    }

    /// The service reading `index`'s assets through `io`.
    fn over_index(
        index: AssetIndex,
        io: Box<dyn AssetIo>,
        metrics_registry: Arc<MetricsRegistry>,
        manifest: Option<PackManifest>,
    ) -> Self {
        Self {
            // When `Some`, every byte slice read is re-hashed against the
            // manifest before reaching the decoder: a packed runtime's, once
            // the pack's signed `manifest.bin` verified.
            reader: Arc::new(AssetReader {
                index: Arc::new(index),
                io: Arc::from(io),
                decoders: DecoderRegistry::new(metrics_registry),
                containers: HashMap::new(),
                manifest: manifest.map(Arc::new),
                refusals: None,
                trust: crate::trust::Trust::Project,
            }),
            storages: HashMap::new(),
            load_count: 0,
            opened: HashMap::new(),
            served_subs: HashMap::new(),
            served: HashSet::new(),
            changes: Channel::bounded(CHANGE_BACKLOG, WhenFull::DropOldest),
            changes_lost: AtomicBool::new(false),
            inherited: Mutex::new(HashSet::new()),
            external_files: HashMap::new(),
            failed: HashSet::new(),
            requests: Channel::bounded(REQUEST_BACKLOG, WhenFull::Reject),
            pending: HashSet::new(),
            installers: HashMap::new(),
            generations: Arc::new(Generations::default()),
            reloading: HashSet::new(),
            meanwhile: HashMap::new(),
            project_root: None,
        }
    }

    /// A service with no asset and no project served: what a host holds
    /// until the project it opens lands, or when it has none.
    pub fn empty() -> Self {
        Self::over_index(
            AssetIndex::empty(),
            Box::new(FileLoader::new(Path::new(""))),
            Arc::new(MetricsRegistry::new()),
            None,
        )
    }

    /// The project whose loose `assets/` folder this service serves; `None`
    /// for a pack.
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// Records that this service serves the loose `assets/` folder of the
    /// project at `project_root`.
    pub(crate) fn serve_project(&mut self, project_root: &Path) {
        self.project_root = Some(project_root.to_path_buf());
    }

    /// Returns a reference to the underlying VFS for metadata enumeration
    /// (asset browser) and direct UUID lookup.
    pub fn index(&self) -> &AssetIndex {
        &self.reader.index
    }

    /// The reader decoding this service's assets now: its index, decoders,
    /// containers, manifest and IO, shared. It decodes on any thread without
    /// this service's lock, and fills none of its caches.
    ///
    /// A reader taken before a [`reindex`](Self::reindex) keeps the index
    /// it was taken with; take it again for the new one.
    pub fn reader(&self) -> Arc<AssetReader> {
        Arc::clone(&self.reader)
    }

    /// The reader to change: this service's own, or a copy when a reader
    /// handed out still holds it, which keeps what it was taken with.
    fn reader_mut(&mut self) -> &mut AssetReader {
        Arc::make_mut(&mut self.reader)
    }

    /// Where this service reports the files it refuses to decode: bytes that
    /// are not what their type says.
    ///
    /// The refusals the service reported to before hand what they hold, and
    /// what is reported to them after, over to `refusals`: a reader made
    /// before — a glTF's resolver — reports to the run's.
    pub fn set_refusals(&mut self, refusals: Arc<crate::trust::Refusals>) {
        if let Some(former) = self.reader_mut().refusals.replace(Arc::clone(&refusals)) {
            if !Arc::ptr_eq(&former, &refusals) {
                former.hand_over_to(refusals);
            }
        }
    }

    /// The refusals this service reports to: the ones set, or refusals kept
    /// in memory when none were.
    pub fn refusals(&mut self) -> Arc<crate::trust::Refusals> {
        let refusals = &mut self.reader_mut().refusals;
        Arc::clone(
            refusals.get_or_insert_with(|| Arc::new(crate::trust::Refusals::new(None, None))),
        )
    }

    /// How much this service trusts the files it decodes: the limits each
    /// decode has. `Project` until set.
    ///
    /// Set it before [`Self::register_default_decoders`]: the readers those
    /// decoders hold (a glTF's buffers and images) take it then.
    pub fn set_trust(&mut self, trust: crate::trust::Trust) {
        self.reader_mut().trust = trust;
    }

    /// Registers a decoder for a specific asset type.
    pub fn register_decoder<A: Asset>(
        &mut self,
        type_name: &str,
        decoder: impl super::decoder::AssetDecoder<A> + Send + Sync + 'static,
    ) {
        self.reader_mut().decoders.register::<A>(type_name, decoder);
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

    /// The handle of `uuid` when it is cached; otherwise files one
    /// [`DecodeRequest`](khora_core::asset::DecodeRequest) for its current
    /// generation on [`decode_requests`](Self::decode_requests) — once while
    /// it is pending — and answers [`Requested::Pending`](super::Requested),
    /// or [`Requested::Failed`](super::Requested) once its last decode failed
    /// and nothing changed since.
    pub fn request<A: Asset>(&mut self, uuid: &AssetUUID) -> super::requested::Requested<A> {
        self.request_off_frame(uuid)
    }

    /// The channel this service files its decode requests on, shared: the
    /// `loading` family takes them from the runtime's copy.
    pub fn decode_requests(&self) -> Channel<khora_core::asset::DecodeRequest> {
        self.requests.clone()
    }

    /// The `loading` family's [`AssetDecoding`](khora_core::asset::AssetDecoding)
    /// over the service `shared` holds: it decodes with that service's
    /// current reader without holding its lock across the decode, and leases
    /// each result on the asset's generation.
    pub fn decoding(shared: &Arc<Mutex<Self>>) -> Arc<dyn khora_core::asset::AssetDecoding> {
        Arc::new(super::off_frame::ServiceDecoding::new(shared))
    }

    /// Decodes the asset `uuid` names: from its own bytes, or, for a
    /// sub-asset, extracted from its opened container, which it returns too.
    fn decode_uncached<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<(A, Option<AssetUUID>)> {
        let reader = Arc::clone(&self.reader);
        let (metadata, source) = reader.source_of(uuid)?;

        if let AssetSource::Sub { container, label } = source {
            return self
                .extract_sub::<A>(*container, label, &metadata.asset_type_name)
                .map(|asset| (asset, Some(*container)));
        }

        let bytes = reader.read(uuid, source)?;
        let (asset, files) = reader.decode_file(metadata, &bytes, TypeId::of::<A>())?;
        // A container file decoded whole (a glTF as one mesh) reads the same
        // files beside it as its parts do: a change to one of them is a
        // change to it.
        if let Some(files) = files {
            self.external_files.insert(*uuid, files);
        }
        let asset = asset.downcast::<A>().map_err(|_| {
            anyhow!(
                "Decoder for type '{}' returned a different asset type than requested.",
                metadata.asset_type_name
            )
        })?;
        Ok((*asset, None))
    }

    /// Loads an asset's raw bytes via the VFS → IO path, skipping the
    /// decoder. Used by scene loading where the bytes are then decoded by a
    /// `SerializationService` strategy rather than an `AssetDecoder<A>`.
    pub fn load_raw(&mut self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        self.reader.read_raw(uuid)
    }

    /// The inherited set, poisoning ignored: it holds plain uuids.
    pub(super) fn inherited_mut(&self) -> std::sync::MutexGuard<'_, HashSet<AssetUUID>> {
        self.inherited
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Atomically replaces the inner `AssetIndex` with a freshly
    /// decoded one. Cached handles are kept; callers should `invalidate`
    /// any UUID whose underlying bytes changed (the editor's hot-reload
    /// pump only invokes `reindex` when files were *added or removed* —
    /// pure in-place modifications go through `invalidate`).
    pub fn reindex(&mut self, index_bytes: &[u8]) -> Result<()> {
        let new_index =
            AssetIndex::new(index_bytes).context("Failed to decode replacement VFS index bytes")?;
        self.reader_mut().index = Arc::new(new_index);
        self.failed.clear();
        // Every decode in flight read the old index.
        self.generations.bump_all();
        self.pending.clear();
        Ok(())
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
mod tests;
