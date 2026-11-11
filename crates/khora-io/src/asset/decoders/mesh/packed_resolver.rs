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

//! A glTF's external files read inside the pack the glTF was packed in.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use khora_core::asset::{asset_key, AssetSource, AssetUUID, CompressionKind};

use super::GltfResourceResolver;
use crate::asset::{AssetIndex, AssetIo, PackManifest};
use crate::trust::{FileKind, Layer, Limits, Refusals, SecurityReason, SecurityRefusal, Trust};

/// Resolves a glTF's buffers and images inside a pack: a URI, relative to
/// the assets root once the glTF's folder is joined to it, is found by its
/// asset key among the pack's entries and read from the pack — never from a
/// file beside it. Each read is checked against the pack's manifest when
/// there is one.
pub struct PackedResolver {
    entries: HashMap<String, (AssetUUID, AssetSource)>,
    io: Arc<dyn AssetIo>,
    manifest: Option<Arc<PackManifest>>,
    refusals: Arc<Refusals>,
    trust: Trust,
}

impl PackedResolver {
    /// A resolver over the entries `index` lists, read through `io` at
    /// `trust`, refusing to `refusals` an entry declaring more bytes than
    /// it may hold.
    pub fn new(
        index: &AssetIndex,
        io: Arc<dyn AssetIo>,
        manifest: Option<Arc<PackManifest>>,
        refusals: Arc<Refusals>,
        trust: Trust,
    ) -> Self {
        let entries = index
            .iter_all()
            .filter_map(|metadata| {
                let source = metadata.variants.get("default")?;
                matches!(source, AssetSource::Packed { .. }).then(|| {
                    (
                        asset_key(&metadata.source_path),
                        (metadata.uuid, source.clone()),
                    )
                })
            })
            .collect();
        Self {
            entries,
            io,
            manifest,
            refusals,
            trust,
        }
    }

    /// Refuses the entry at `uri` declaring more bytes than its manifest
    /// recorded, or than its kind may hold at this trust — before anything
    /// is read or allocated.
    fn check_declared(
        &self,
        uri: &str,
        uuid: &AssetUUID,
        source: &AssetSource,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let AssetSource::Packed {
            size,
            uncompressed_size,
            compression,
            ..
        } = source
        else {
            return Ok(());
        };
        let declared = match compression {
            CompressionKind::None => *size,
            _ => *uncompressed_size,
        };
        let cap = match self
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.get(uuid))
        {
            Some(entry) => entry.size,
            None => Limits::of(FileKind::of_path(std::path::Path::new(uri)), self.trust).max_bytes,
        };
        if declared <= cap {
            return Ok(());
        }
        let refusal = SecurityRefusal {
            path: std::path::PathBuf::from(uri),
            trust: self.trust,
            layer: Layer::Pack,
            reason: SecurityReason::TooLarge {
                size: declared,
                cap,
            },
        };
        let error = refusal.to_string();
        self.refusals.refuse(refusal, None);
        Err(error.into())
    }

    fn read(&self, what: &str, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let key = asset_key(std::path::Path::new(uri));
        let (uuid, source) = self
            .entries
            .get(&key)
            .ok_or_else(|| format!("the pack holds no external {what} '{uri}'"))?;
        self.check_declared(uri, uuid, source)?;
        let bytes = self
            .io
            .load_bytes(source)
            .map_err(|error| format!("reading the packed {what} '{uri}': {error:#}"))?;
        if let Some(manifest) = &self.manifest {
            manifest
                .verify(uuid, &bytes)
                .map_err(|error| format!("the packed {what} '{uri}': {error:#}"))?;
        }
        Ok(bytes)
    }
}

impl GltfResourceResolver for PackedResolver {
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.read("buffer", uri)
    }

    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.read("image", uri)
    }
}
