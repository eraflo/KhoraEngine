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

//! The asset service: assets loaded on demand through the VFS, the IO and the decoders.
//!
//! See the book: concepts/assets.md, "The pipeline".

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

/// How many decode requests wait for the `loading` family; past it, a request
/// is refused and asked for again on a later frame.
pub(super) const REQUEST_BACKLOG: usize = 4096;

/// The asset service: a cache of decoded assets by UUID, loaded on demand.
///
/// A service, not an agent: loading has no strategy to negotiate.
pub struct AssetService {
    /// What decoding reads, shared with whoever decodes off this service's lock.
    ///
    /// Replaced, never changed in place, while a reader is out.
    pub(super) reader: Arc<AssetReader>,
    pub(super) storages: HashMap<TypeId, Box<dyn AnyAssets>>,
    pub(super) load_count: usize,
    /// Each opened container by its file's uuid, shared by its extracted sub-assets.
    opened: HashMap<AssetUUID, Arc<dyn Any + Send + Sync>>,
    /// Every sub-asset ever served from each container, by the container file's uuid.
    ///
    /// Never emptied: invalidating the container announces each, even one a new
    /// index dropped or whose container failed to reload.
    pub(super) served_subs: HashMap<AssetUUID, HashSet<AssetUUID>>,
    /// Every asset [`load`](Self::load) ever decoded: the engine may hold a copy.
    pub(super) served: HashSet<AssetUUID>,
    /// The assets whose bytes changed, for the engine to replace its copies.
    changes: Channel<AssetChanged>,
    /// Set when `changes` dropped an announcement it had no room for.
    changes_lost: AtomicBool,
    /// What the services this one replaced served and it has not served since.
    ///
    /// The engine may still hold those copies, no last good copy of this service's assets.
    inherited: Mutex<HashSet<AssetUUID>>,
    /// The files, by asset key, each opened container reads besides its own.
    ///
    /// A change to one is a change to the container. Kept across invalidations.
    pub(super) external_files: HashMap<AssetUUID, Vec<String>>,
    /// The assets whose last [`load`](Self::load) failed.
    ///
    /// Forgotten when any asset changes or the index is replaced: only such a
    /// change can fix what made a load fail.
    pub(super) failed: HashSet<AssetUUID>,
    /// Where [`request`](Self::request) files a decode for the `loading` family.
    pub(super) requests: Channel<DecodeRequest>,
    /// The assets, by uuid and type, with a decode filed and not taken in.
    pub(super) pending: HashSet<(AssetUUID, TypeId)>,
    /// How each type a request named goes into the caches once decoded.
    pub(super) installers: HashMap<TypeId, super::off_frame::Installer>,
    /// Each asset's generation, which a decode in flight holds to.
    pub(super) generations: Arc<Generations>,
    /// The changed assets whose new value is being decoded, for `asset_reload`.
    reloading: HashSet<AssetUUID>,
    /// The last good copy of each asset being decoded again, by asset and type.
    ///
    /// Served to a reference resolved meanwhile; never cached, so the new value
    /// replaces it when it lands.
    pub(super) meanwhile: HashMap<(AssetUUID, TypeId), Box<dyn Any + Send + Sync>>,
    /// The project whose loose `assets/` it serves; `None` for a pack.
    ///
    /// A project index filed off the frame is taken in only for this project.
    project_root: Option<PathBuf>,
}

impl AssetService {
    /// Creates a service over the index `index_bytes`, reading through `io`.
    ///
    /// With a `manifest`, every read is checked against its BLAKE3 digest and
    /// refused on a mismatch.
    ///
    /// # Errors
    /// When `index_bytes` is no index.
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

    /// Returns the service reading `index`'s assets through `io`.
    fn over_index(
        index: AssetIndex,
        io: Box<dyn AssetIo>,
        metrics_registry: Arc<MetricsRegistry>,
        manifest: Option<PackManifest>,
    ) -> Self {
        Self {
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

    /// Returns a service of no asset, for a host until its project lands or without one.
    pub fn empty() -> Self {
        Self::over_index(
            AssetIndex::empty(),
            Box::new(FileLoader::new(Path::new(""))),
            Arc::new(MetricsRegistry::new()),
            None,
        )
    }

    /// Returns the project whose loose `assets/` this service serves; `None` for a pack.
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// Records that this service serves the loose `assets/` folder of the
    /// project at `project_root`.
    pub(crate) fn serve_project(&mut self, project_root: &Path) {
        self.project_root = Some(project_root.to_path_buf());
    }

    /// Returns the asset index, to enumerate or look up metadata by UUID.
    pub fn index(&self) -> &AssetIndex {
        &self.reader.index
    }

    /// Returns the reader decoding this service's assets, on any thread, without its lock.
    ///
    /// It fills none of the caches. A reader taken before a
    /// [`reindex`](Self::reindex) keeps the index it was taken with.
    pub fn reader(&self) -> Arc<AssetReader> {
        Arc::clone(&self.reader)
    }

    /// Returns the reader to change: its own, or a copy when one handed out still holds it.
    fn reader_mut(&mut self) -> &mut AssetReader {
        Arc::make_mut(&mut self.reader)
    }

    /// Sets the refusals this service reports the files it refuses to decode to.
    ///
    /// The refusals it reported to before hand their content, and what is reported
    /// to them later, over to `refusals`: a glTF resolver made before reports here.
    pub fn set_refusals(&mut self, refusals: Arc<crate::trust::Refusals>) {
        if let Some(former) = self.reader_mut().refusals.replace(Arc::clone(&refusals)) {
            if !Arc::ptr_eq(&former, &refusals) {
                former.hand_over_to(refusals);
            }
        }
    }

    /// Returns the refusals this service reports to; in memory only when none were set.
    pub fn refusals(&mut self) -> Arc<crate::trust::Refusals> {
        let refusals = &mut self.reader_mut().refusals;
        Arc::clone(
            refusals.get_or_insert_with(|| Arc::new(crate::trust::Refusals::new(None, None))),
        )
    }

    /// Sets the trust the files are decoded at, which gives each decode its limits.
    ///
    /// `Project` until set. Set it before [`Self::register_default_decoders`]:
    /// the readers those decoders hold take it then.
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

    /// Loads and decodes the asset `uuid` on the calling thread; returns its cached handle.
    ///
    /// # Errors
    /// When the asset is not indexed, cannot be read, or does not decode as `A`.
    pub fn load<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<AssetHandle<A>> {
        let type_id = TypeId::of::<A>();

        let storage = self
            .storages
            .entry(type_id)
            .or_insert_with(|| Box::new(Assets::<A>::new()));

        let assets = storage
            .as_any_mut()
            .downcast_mut::<Assets<A>>()
            .ok_or_else(|| anyhow!("Mismatched asset storage type"))?;

        if let Some(handle) = assets.get(uuid) {
            return Ok(handle.clone());
        }

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

    /// Returns the asset `uuid` if cached, else files its decode off the frame.
    ///
    /// One [`DecodeRequest`](khora_core::asset::DecodeRequest) is filed while it is
    /// pending. [`Requested`](super::Requested) says `Failed` once its last decode
    /// failed and nothing changed since.
    pub fn request<A: Asset>(&mut self, uuid: &AssetUUID) -> super::requested::Requested<A> {
        self.request_off_frame(uuid)
    }

    /// Returns the channel this service files its decode requests on.
    pub fn decode_requests(&self) -> Channel<khora_core::asset::DecodeRequest> {
        self.requests.clone()
    }

    /// Returns the `loading` family's decoding over the service `shared` holds.
    ///
    /// An [`AssetDecoding`](khora_core::asset::AssetDecoding) that decodes without
    /// holding the service's lock and leases each result on the files it read.
    pub fn decoding(shared: &Arc<Mutex<Self>>) -> Arc<dyn khora_core::asset::AssetDecoding> {
        Arc::new(super::off_frame::ServiceDecoding::new(shared))
    }

    /// Decodes the asset `uuid`, a sub-asset from its container, which it returns too.
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
        // A container decoded whole (a glTF as one mesh) reads the files beside
        // it as its parts do: a change to one is a change to it.
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

    /// Returns an asset's raw bytes, undecoded, for a scene load to decode itself.
    ///
    /// # Errors
    /// As [`AssetReader::read_raw`](super::AssetReader::read_raw).
    pub fn load_raw(&mut self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        self.reader.read_raw(uuid)
    }

    /// Returns the inherited set, poisoning ignored: it holds plain uuids.
    pub(super) fn inherited_mut(&self) -> std::sync::MutexGuard<'_, HashSet<AssetUUID>> {
        self.inherited
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Replaces the index with the one `index_bytes` encode, keeping cached handles.
    ///
    /// An asset whose bytes changed is to be invalidated too.
    ///
    /// # Errors
    /// When `index_bytes` is no index; the service is left as it was.
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

    /// Whether the last [`load`](Self::load) of `uuid` failed and nothing changed since.
    ///
    /// Loading it again would fail the same way, so a caller resolving every tick skips it.
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
