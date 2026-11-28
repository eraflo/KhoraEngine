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
use crate::asset::{AssetIndex, AssetIo, PackManifest, PackedFile};
use crate::trust::{
    FileKind, Limits, Refusals, SecurityReason, SecurityRefusal, Trust, TrustLayer,
};

/// Resolves a glTF's buffers and images inside its pack, never from a file beside it.
///
/// A URI, joined to the glTF's folder, is found by its path in the pack's
/// external-files table, read under the glTF's cap; one the index lists as an
/// asset (a texture) is found among the asset entries. Each read is checked
/// against the pack's manifest, if any.
pub struct PackedResolver {
    /// The pack's external files, by their forward-slash path.
    external_files: HashMap<String, PackedFile>,
    /// The packed asset entries, by their forward-slash path.
    assets: HashMap<String, (AssetUUID, AssetSource)>,
    io: Arc<dyn AssetIo>,
    manifest: Option<Arc<PackManifest>>,
    refusals: Arc<Refusals>,
    trust: Trust,
}

impl PackedResolver {
    /// Creates a resolver over the external files and packed entries `index` lists, read through `io`.
    ///
    /// A file declaring more bytes than it may hold at `trust` is refused to `refusals`.
    pub fn new(
        index: &AssetIndex,
        io: Arc<dyn AssetIo>,
        manifest: Option<Arc<PackManifest>>,
        refusals: Arc<Refusals>,
        trust: Trust,
    ) -> Self {
        let assets = index
            .iter_all()
            .filter_map(|metadata| {
                let source = metadata.default_source()?;
                matches!(source, AssetSource::Packed { .. }).then(|| {
                    (
                        asset_key(&metadata.source_path),
                        (metadata.uuid, source.clone()),
                    )
                })
            })
            .collect();
        let external_files = index
            .external_files()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        Self {
            external_files,
            assets,
            io,
            manifest,
            refusals,
            trust,
        }
    }

    /// Refuses, before any read, the file at `uri` if it declares more bytes
    /// than its manifest records or its glTF's cap allows.
    fn check_declared(
        &self,
        uri: &str,
        packed: &Packed<'_>,
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
        let recorded = self.manifest.as_ref().and_then(|manifest| match packed {
            Packed::External(file) => manifest.external_file(&file.path).map(|r| r.size),
            Packed::Asset(uuid, _) => manifest.get(uuid).map(|entry| entry.size),
        });
        let cap = recorded.unwrap_or_else(|| Limits::of(FileKind::Gltf, self.trust).max_bytes);
        if declared <= cap {
            return Ok(());
        }
        Err(self.refuse(
            uri,
            SecurityReason::TooLarge {
                size: declared,
                cap,
            },
        ))
    }

    /// Reports the file at `uri` refused for `reason` at the pack layer; returns the error.
    fn refuse(&self, uri: &str, reason: SecurityReason) -> Box<dyn Error + Send + Sync> {
        let refusal = SecurityRefusal {
            path: std::path::PathBuf::from(uri),
            trust: self.trust,
            layer: TrustLayer::Pack,
            reason,
        };
        let error = refusal.to_string();
        self.refusals.refuse(refusal, None);
        error.into()
    }

    fn read(&self, what: &str, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let key = asset_key(std::path::Path::new(uri));
        let packed = match self.external_files.get(&key) {
            Some(file) => Packed::External(file),
            None => {
                let (uuid, source) = self
                    .assets
                    .get(&key)
                    .ok_or_else(|| format!("the pack holds no external {what} '{uri}'"))?;
                Packed::Asset(*uuid, source)
            }
        };
        let source = match &packed {
            Packed::External(file) => file.source(),
            Packed::Asset(_, source) => (*source).clone(),
        };
        self.check_declared(uri, &packed, &source)?;
        let bytes = match self.io.load_bytes(&source) {
            Ok(bytes) => bytes,
            // Its place and sizes are signed with the index: a sealed file
            // that fails to load was changed after the build.
            Err(error) if self.manifest.is_some() => {
                let _ = self.refuse(uri, SecurityReason::Tampered);
                return Err(format!("reading the packed {what} '{uri}': {error:#}").into());
            }
            Err(error) => {
                return Err(format!("reading the packed {what} '{uri}': {error:#}").into())
            }
        };
        if let Some(manifest) = &self.manifest {
            let verified = match &packed {
                Packed::External(file) => manifest.verify_external_file(&file.path, &bytes),
                Packed::Asset(uuid, _) => manifest.verify(uuid, &bytes),
            };
            if let Err(error) = verified {
                let _ = self.refuse(uri, SecurityReason::Tampered);
                return Err(format!("the packed {what} '{uri}': {error:#}").into());
            }
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

/// What a URI names in the pack: an external file, or an asset entry.
enum Packed<'a> {
    /// A row of the external-files table.
    External(&'a PackedFile),
    /// An asset entry, its uuid and its source.
    Asset(AssetUUID, &'a AssetSource),
}
