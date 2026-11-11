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
use khora_core::asset::{AssetMetadata, AssetSource, AssetUUID};

use super::container::AssetContainer;
use super::decoder_registry::DecoderRegistry;
use super::io::AssetIo;
use super::manifest::PackManifest;
use super::AssetIndex;
use crate::trust::{Layer, QuarantineKey, Refusals, SecurityReason, SecurityRefusal, Trust};

/// How many of a file's first bytes its decoder is shown to recognise it.
const SNIFFED: usize = 512;

/// What decoding an asset reads: the index snapshot, the decoders, the
/// containers, the pack manifest and the IO. Shared as an
/// `Arc<AssetReader>`, it decodes on any thread, without the
/// [`AssetService`](super::AssetService)'s lock and without touching its
/// caches.
///
/// A service hands its current reader out with
/// [`AssetService::reader`](super::AssetService::reader). A new index
/// (`reindex`) gives the service a new reader; a reader taken before keeps
/// reading the index it was built over.
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
    /// Decodes the asset `uuid` as the Rust type `type_id`: from its own
    /// bytes, or, for a sub-asset, extracted from its container, opened for
    /// the occasion. Touches no cache: decoding one asset twice decodes it
    /// twice.
    ///
    /// The type is a `TypeId` because the decoder is chosen by the asset's
    /// index type name (its metadata), as `DecoderRegistry::decode_at` does;
    /// the `TypeId` only checks the result is the type the caller will
    /// downcast to.
    ///
    /// Never panics on bad data: every failure is an `Err` in
    /// [`Decoded::value`].
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

    /// Decodes `uuid` as whatever type its decoder or container produces,
    /// noting in `external_files` each container it opened with the files
    /// that container reads besides its own.
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
            return Self::extract(
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

    /// The raw bytes of the asset `uuid`, undecoded: what a scene load reads
    /// before its own encodings decode it. Refused for a sub-asset, which has
    /// no bytes of its own. Runs on any thread.
    pub fn read_raw(&self, uuid: &AssetUUID) -> Result<Vec<u8>> {
        let (_, source) = self.source_of(uuid)?;
        if let AssetSource::Sub { container, label } = source {
            return Err(anyhow!(
                "sub-asset '{label}' of {container} has no bytes of its own"
            ));
        }
        self.read(uuid, source)
    }

    /// The index this reader reads assets through.
    pub fn index(&self) -> &AssetIndex {
        &self.index
    }

    /// The index entry of `uuid` and its default source.
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

    /// The bytes of `uuid` at `source`, checked against the manifest when
    /// there is one.
    pub(super) fn read(&self, uuid: &AssetUUID, source: &AssetSource) -> Result<Vec<u8>> {
        let bytes = self.io.load_bytes(source)?;
        if let Some(manifest) = &self.manifest {
            manifest
                .verify(uuid, &bytes)
                .context("Asset integrity check failed")?;
        }
        Ok(bytes)
    }

    /// Decodes a file's own `bytes` with the decoder of its index type, as
    /// the type `type_id`. A file whose type is also a container (a glTF
    /// decoded whole) is opened too, once it decoded as that type, for the
    /// files it reads besides its own: `None` for any other.
    pub(super) fn decode_file(
        &self,
        metadata: &AssetMetadata,
        bytes: &[u8],
        type_id: TypeId,
    ) -> Result<DecodedFile> {
        self.check_type(metadata, bytes)?;
        let value =
            self.decoders
                .decode_any_at(&metadata.asset_type_name, bytes, &metadata.source_path)?;
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
                opener
                    .open(bytes, &metadata.source_path)
                    .ok()
                    .map(|opened| opener.external_files(opened.as_ref()))
            });
        Ok((value, files))
    }

    /// Refuses `bytes` when the decoder of the file's type does not know
    /// what they start with: a file whose extension lies about what it
    /// holds is never handed to a decoder. The refusal quarantines the file
    /// by its content.
    fn check_type(&self, metadata: &AssetMetadata, bytes: &[u8]) -> Result<()> {
        let head = &bytes[..bytes.len().min(SNIFFED)];
        if self.decoders.sniff(&metadata.asset_type_name, head) != Some(false) {
            return Ok(());
        }
        let path = (metadata.variants.get("default"))
            .and_then(|source| self.io.locate(source))
            .unwrap_or_else(|| PathBuf::from(&metadata.source_path));
        let refusal = SecurityRefusal {
            path,
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: metadata.asset_type_name.clone(),
            },
        };
        let error = anyhow!("{refusal}");
        if let Some(refusals) = &self.refusals {
            let key = QuarantineKey::Content(*blake3::hash(bytes).as_bytes());
            refusals.refuse(refusal, Some(key));
        }
        Err(error)
    }

    /// What opens the container file `container`.
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
        let opened = opener
            .open(&bytes, &metadata.source_path)
            .with_context(|| format!("opening container {container}"))?;
        let files = opener.external_files(opened.as_ref());
        Ok((opener, opened, files))
    }

    /// Extracts the sub-asset `label` (index type `type_name`) from an
    /// opened container.
    pub(super) fn extract(
        opener: &Arc<dyn AssetContainer>,
        opened: &Arc<dyn Any + Send + Sync>,
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<Box<dyn Any + Send>> {
        opener
            .extract(opened.as_ref(), container, label, type_name)
            .with_context(|| format!("extracting '{label}' from {container}"))
    }
}
