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

//! The shared reader: what decoding an asset reads, behind `&self`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{AssetMetadata, AssetSource, AssetUUID, CompressionKind};

use super::container::AssetContainer;
use super::decoder_registry::DecoderRegistry;
use super::io::AssetIo;
use super::manifest::PackManifest;
use super::AssetIndex;
use crate::trust::{
    FileKind, LimitExceeded, Limits, QuarantineKey, Refusals, SecurityReason, SecurityRefusal,
    Trust, TrustLayer,
};

/// How many of a file's first bytes its decoder is shown to recognise it.
const SNIFFED: usize = 512;

/// Everything decoding an asset reads: index snapshot, decoders, containers, manifest and IO.
///
/// Shared as an `Arc`, it decodes on any thread, without the
/// [`AssetService`](super::AssetService)'s lock or caches. A reader taken with
/// [`AssetService::reader`](super::AssetService::reader) keeps the index it was
/// built over when the service reindexes. See the book: concepts/trust.md, "The guard".
#[derive(Clone)]
pub struct AssetReader {
    pub(super) index: Arc<AssetIndex>,
    pub(super) io: Arc<dyn AssetIo>,
    pub(super) decoders: DecoderRegistry,
    pub(super) containers: HashMap<String, Arc<dyn AssetContainer>>,
    /// When `Some`, every byte slice read is hashed against it before it is
    /// decoded.
    pub(super) manifest: Option<Arc<PackManifest>>,
    /// Where a file whose bytes are not what its type says is refused.
    pub(super) refusals: Option<Arc<Refusals>>,
    /// How much the files read are trusted: the limits each decode has.
    pub(super) trust: Trust,
}

/// A file decoded from its own bytes, and — when its type is also a
/// container — the files it reads besides its own.
pub(super) type DecodedFile = (Box<dyn Any + Send>, Option<Vec<String>>);

/// A container file opened: what opens it, the opened value, and the files
/// it reads besides its own, as asset keys.
pub(super) type Opened = (
    Arc<dyn AssetContainer>,
    Arc<dyn Any + Send + Sync>,
    Vec<String>,
);

/// What one [`AssetReader::decode`] produced; defined in `khora-core`, where
/// the `loading` family's lanes names it.
pub use khora_core::asset::Decoded;

impl AssetReader {
    /// Decodes the asset `uuid`, a sub-asset from its container, as the Rust type `type_id`.
    ///
    /// Touches no cache: decoding one asset twice decodes it twice. The decoder is
    /// chosen by the asset's index type name; `type_id` only checks the result.
    /// Never panics on bad data: every failure is an `Err` in [`Decoded::value`].
    pub fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        let mut external_files = Vec::new();
        let value = self
            .decode_any(uuid, type_id, &mut external_files)
            .and_then(|value| {
                if (*value).type_id() == type_id {
                    Ok(value)
                } else {
                    Err(anyhow!(
                        "asset {uuid} decodes to another type than the one asked for"
                    ))
                }
            })
            .map_err(|error| format!("{error:#}"));
        Decoded {
            uuid: *uuid,
            type_id,
            value,
            external_files,
        }
    }

    /// Decodes `uuid` as whatever type its decoder or container produces.
    ///
    /// Notes in `external_files` each container opened, with the files it reads
    /// besides its own.
    fn decode_any(
        &self,
        uuid: &AssetUUID,
        type_id: TypeId,
        external_files: &mut Vec<(AssetUUID, Vec<String>)>,
    ) -> Result<Box<dyn Any + Send>> {
        let (metadata, source) = self.source_of(uuid)?;
        if let AssetSource::Sub { container, label } = source {
            let (opener, opened, files) = self
                .open(container)
                .with_context(|| format!("sub-asset '{label}'"))?;
            external_files.push((*container, files));
            return self.extract(
                &opener,
                &opened,
                *container,
                label,
                &metadata.asset_type_name,
            );
        }
        let bytes = self.read(uuid, source)?;
        let (value, files) = self.decode_file(metadata, &bytes, type_id)?;
        if let Some(files) = files {
            external_files.push((*uuid, files));
        }
        Ok(value)
    }

    /// Returns the raw bytes of the asset `uuid`, undecoded, as a scene load reads them.
    ///
    /// Runs on any thread.
    ///
    /// # Errors
    /// For a sub-asset, which has no bytes of its own, and as any read fails.
    pub fn read_raw(&self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        let (_, source) = self.source_of(uuid)?;
        if let AssetSource::Sub { container, label } = source {
            return Err(anyhow!(
                "sub-asset '{label}' of {container} has no bytes of its own"
            ));
        }
        self.read(uuid, source)
    }

    /// Returns the index this reader reads assets through.
    pub fn index(&self) -> &AssetIndex {
        &self.index
    }

    /// Returns the index entry of `uuid` and its default source.
    pub(super) fn source_of(&self, uuid: &AssetUUID) -> Result<(&AssetMetadata, &AssetSource)> {
        let metadata = self
            .index
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;
        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;
        Ok((metadata, source))
    }

    /// Reads the bytes of `uuid` at `source`, checked against the manifest if any.
    pub(super) fn read(&self, uuid: &AssetUUID, source: &AssetSource) -> Result<Vec<u8>> {
        if let AssetSource::Packed {
            size,
            uncompressed_size,
            compression,
            ..
        } = source
        {
            let declared = match compression {
                CompressionKind::None => *size,
                _ => *uncompressed_size,
            };
            self.check_packed(uuid, declared)?;
        }
        let bytes = self.io.load_bytes(source)?;
        if let Some(manifest) = &self.manifest {
            manifest
                .verify(uuid, &bytes)
                .context("Asset integrity check failed")?;
        }
        Ok(bytes)
    }

    /// Decodes a file's own `bytes` with the decoder of its index type, as `type_id`.
    ///
    /// A file whose type is also a container (a glTF decoded whole) is opened too,
    /// for the files it reads besides its own: `None` for any other.
    pub(super) fn decode_file(
        &self,
        metadata: &AssetMetadata,
        bytes: &[u8],
        type_id: TypeId,
    ) -> Result<DecodedFile> {
        self.check_type(metadata, bytes)?;
        let limits = self.limits_of(metadata);
        self.check_size(metadata, bytes, &limits)?;
        let value = self.guarded(metadata, Content::Bytes(bytes), || {
            self.decoders.decode_any_limited(
                &metadata.asset_type_name,
                bytes,
                &metadata.source_path,
                &limits,
            )
        })?;
        if (*value).type_id() != type_id {
            return Err(anyhow!(
                "Decoder for type '{}' returned a different asset type than requested.",
                metadata.asset_type_name
            ));
        }
        let files = self
            .containers
            .get(&metadata.asset_type_name)
            .and_then(|opener| {
                self.guarded(metadata, Content::Bytes(bytes), || {
                    opener.open_limited(bytes, &metadata.source_path, &limits)
                })
                .ok()
                .map(|opened| opener.external_files(opened.as_ref()))
            });
        Ok((value, files))
    }

    /// Refuses `bytes` whose start the decoder of the file's type does not know.
    ///
    /// A file whose extension lies is never decoded; it is quarantined by content.
    fn check_type(&self, metadata: &AssetMetadata, bytes: &[u8]) -> Result<()> {
        let head = &bytes[..bytes.len().min(SNIFFED)];
        if self.decoders.sniff(&metadata.asset_type_name, head) != Some(false) {
            return Ok(());
        }
        let reason = SecurityReason::WrongType {
            expected: metadata.asset_type_name.clone(),
        };
        Err(self.refuse(metadata, Content::Bytes(bytes), TrustLayer::Decoder, reason))
    }

    /// Returns what opens the container file `container`.
    pub(super) fn opener_of(&self, container: &AssetUUID) -> Result<Arc<dyn AssetContainer>> {
        let metadata = self
            .index
            .get_metadata(container)
            .ok_or_else(|| anyhow!("its container {container} is not in the VFS"))?;
        self.containers
            .get(&metadata.asset_type_name)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "no container registered for '{}' files",
                    metadata.asset_type_name
                )
            })
    }

    /// Opens the container file `container` from its bytes.
    pub(super) fn open(&self, container: &AssetUUID) -> Result<Opened> {
        let opener = self.opener_of(container)?;
        let (metadata, source) = self
            .source_of(container)
            .map_err(|_| anyhow!("container {container} has no 'default' variant"))?;
        let bytes = self.read(container, source)?;
        self.check_type(metadata, &bytes)?;
        let limits = self.limits_of(metadata);
        self.check_size(metadata, &bytes, &limits)?;
        let opened = self
            .guarded(metadata, Content::Bytes(&bytes), || {
                opener.open_limited(&bytes, &metadata.source_path, &limits)
            })
            .with_context(|| format!("opening container {container}"))?;
        let files = opener.external_files(opened.as_ref());
        Ok((opener, opened, files))
    }

    /// Extracts the sub-asset `label` (index type `type_name`) from an
    /// opened container.
    pub(super) fn extract(
        &self,
        opener: &Arc<dyn AssetContainer>,
        opened: &Arc<dyn Any + Send + Sync>,
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<Box<dyn Any + Send>> {
        // The part's own type gives its limits: a glTF's texture is a
        // texture's, read at this reader's trust.
        let limits = self
            .decoders
            .limits(type_name, self.trust)
            .unwrap_or_else(|| {
                Limits::of(FileKind::of_path(std::path::Path::new(label)), self.trust)
            });
        let extract =
            || opener.extract_limited(opened.as_ref(), container, label, type_name, &limits);
        match self.index.get_metadata(&container) {
            Some(metadata) => self.guarded(metadata, Content::Reread, extract),
            None => extract(),
        }
        .with_context(|| format!("extracting '{label}' from {container}"))
    }

    /// Returns the limits of `metadata`'s type at this reader's trust.
    ///
    /// Its decoder's, or else its file kind's.
    fn limits_of(&self, metadata: &AssetMetadata) -> Limits {
        self.decoders
            .limits(&metadata.asset_type_name, self.trust)
            .unwrap_or_else(|| Limits::of(FileKind::of_path(&metadata.source_path), self.trust))
    }

    /// Refuses a pack entry declaring more bytes than it may hold, before anything is read.
    ///
    /// Its cap is the manifest's size for it, or else its type's limit.
    fn check_packed(&self, uuid: &AssetUUID, declared: u64) -> Result<()> {
        let Some(metadata) = self.index.get_metadata(uuid) else {
            return Ok(());
        };
        let cap = match self
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.get(uuid))
        {
            Some(entry) => entry.size,
            None => self.limits_of(metadata).max_bytes,
        };
        if declared <= cap {
            return Ok(());
        }
        let reason = SecurityReason::TooLarge {
            size: declared,
            cap,
        };
        // Nothing is read for it: the declared size is what is refused.
        Err(self.refuse(metadata, Content::Unread, TrustLayer::Pack, reason))
    }

    /// Refuses `bytes` over their type's byte limit, before they are decoded.
    fn check_size(&self, metadata: &AssetMetadata, bytes: &[u8], limits: &Limits) -> Result<()> {
        let size = bytes.len() as u64;
        if size <= limits.max_bytes {
            return Ok(());
        }
        let reason = SecurityReason::TooLarge {
            size,
            cap: limits.max_bytes,
        };
        Err(self.refuse(metadata, Content::Bytes(bytes), TrustLayer::Decoder, reason))
    }

    /// Runs a decoder's `work` on the file of `metadata`, catching its panics.
    ///
    /// A limit that stopped it, or a panic, refuses and quarantines the file by
    /// its `content`; the panic never reaches the caller.
    fn guarded<T>(
        &self,
        metadata: &AssetMetadata,
        content: Content<'_>,
        work: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                let limit = error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<LimitExceeded>())
                    .cloned();
                match limit {
                    Some(limit) => {
                        let reason = SecurityReason::OverLimit {
                            what: limit.what,
                            limit: limit.limit,
                        };
                        let refusal = self.refuse(metadata, content, TrustLayer::Decoder, reason);
                        Err(error.context(refusal))
                    }
                    None => Err(error),
                }
            }
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                let reason = SecurityReason::DecoderPanic { message };
                Err(self.refuse(metadata, content, TrustLayer::Decoder, reason))
            }
        }
    }

    /// Reports the refusal of the file of `metadata` and returns it as an error.
    ///
    /// The file is quarantined by its `content` when that is known.
    fn refuse(
        &self,
        metadata: &AssetMetadata,
        content: Content<'_>,
        layer: TrustLayer,
        reason: SecurityReason,
    ) -> anyhow::Error {
        let path = (metadata.variants.get("default"))
            .and_then(|source| self.io.locate(source))
            .unwrap_or_else(|| PathBuf::from(&metadata.source_path));
        let refusal = SecurityRefusal {
            path,
            trust: self.trust,
            layer,
            reason,
        };
        let error = anyhow!("{refusal}");
        if let Some(refusals) = &self.refusals {
            let reread;
            let bytes = match content {
                Content::Bytes(bytes) => Some(bytes),
                Content::Reread => {
                    reread = (metadata.variants.get("default"))
                        .and_then(|source| self.io.load_bytes(source).ok());
                    reread.as_deref()
                }
                Content::Unread => None,
            };
            let key = bytes.map(|bytes| QuarantineKey::Content(*blake3::hash(bytes).as_bytes()));
            refusals.refuse(refusal, key);
        }
        error
    }
}

/// Returns a panic's message, when it carried one.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the decoder panicked".to_owned())
}

/// What a refused file is quarantined by.
#[derive(Clone, Copy)]
enum Content<'a> {
    /// The bytes read for it.
    Bytes(&'a [u8]),
    /// Its bytes, read again: a container's part refused after the container
    /// was opened from bytes since dropped.
    Reread,
    /// Nothing: a pack entry refused for the size it declares, never read.
    Unread,
}
