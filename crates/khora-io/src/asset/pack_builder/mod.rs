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

//! The pack builder: a project's `assets/` bundled into `data.pack` and `index.bin`,
//! and a signed `manifest.bin` when it is given a signer.
//!
//! See the book: reference/formats.md, "`.pack` — asset archive".

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Sender;
use khora_core::asset::{asset_key, AssetMetadata, AssetSource, CompressionKind};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};

use super::{
    AssetIdRegistry, AssetIndex, IndexBuilder, LeftOutFile, LeftOutReason, PackLoader,
    PackManifest, PACK_FLAG_LZ4, PACK_FLAG_MANIFEST,
};
use crate::trust::{FileGuard, ProjectFile, ReadError, Refusals, Trust};

mod entry_writer;
mod external_files;
mod pack_output;
mod pack_progress;

use entry_writer::EntryWriter;
use external_files::{external_paths, ExternalFiles};
pub use pack_output::PackOutput;
pub use pack_progress::PackProgress;

/// Builds a release pack from a project's `assets/` directory.
///
/// ```ignore
/// use khora_io::asset::{PackBuilder, PackProgress};
///
/// let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
/// std::thread::spawn(move || {
///     while let Ok(ev) = rx.recv() { /* update UI */ }
/// });
///
/// let out = PackBuilder::new(&project_assets_dir, &dest_dir)
///     .with_progress(tx)
///     .build()?;
/// ```
pub struct PackBuilder<'a> {
    assets_root: &'a Path,
    dest_dir: &'a Path,
    progress: Option<Sender<PackProgress>>,
    compression: CompressionKind,
    /// What signs `manifest.bin`; none, no manifest is written.
    signer: Option<&'a super::signing::PackSigner>,
    /// Where the build's refusals are reported.
    refusals: Arc<Refusals>,
}

impl<'a> PackBuilder<'a> {
    /// Returns a builder packing `assets_root`, a project's `assets/`, into `dest_dir`.
    ///
    /// `dest_dir` is created if missing.
    pub fn new(assets_root: &'a Path, dest_dir: &'a Path) -> Self {
        Self {
            assets_root,
            dest_dir,
            progress: None,
            compression: CompressionKind::None,
            signer: None,
            refusals: Refusals::detached(),
        }
    }

    /// Forwards the build's progress to `tx`; the sends never block nor fail the build.
    pub fn with_progress(mut self, tx: Sender<PackProgress>) -> Self {
        self.progress = Some(tx);
        self
    }

    /// Sets the per-entry compression; [`CompressionKind::None`] by default.
    ///
    /// With LZ4, each asset is compressed alone, so the runtime decompresses it on demand.
    pub fn with_compression(mut self, compression: CompressionKind) -> Self {
        self.compression = compression;
        self
    }

    /// Signs the pack with `signer`, writing `manifest.bin` over the `index.bin` built.
    ///
    /// A build without a signer writes no manifest, and a runtime refuses its pack.
    pub fn with_signer(mut self, signer: &'a super::signing::PackSigner) -> Self {
        self.signer = Some(signer);
        self
    }

    /// Reports the build's refusals to `refusals`, the run's; kept in memory by default.
    ///
    /// Each refusal is logged as well.
    pub fn with_refusals(mut self, refusals: Arc<Refusals>) -> Self {
        self.refusals = refusals;
        self
    }

    /// Builds the pack, writing `data.pack`, `index.bin` and, when signed, `manifest.bin`.
    ///
    /// Two builds of one tree write byte-identical files. Every file is read
    /// through a [`FileGuard`] at the trust of a trusted project, reporting to
    /// the build's refusals ([`Self::with_refusals`]): the assets and their
    /// external files under the assets root, the identity registry under the
    /// project root, the assets root's parent. An external file the guard
    /// refuses — its path leaving the assets root, by `..` or through a link,
    /// or its size over its container's cap — or that is missing is left out of
    /// the pack, listed in [`PackOutput::left_out`] and logged, and the build
    /// goes on. Its progress ends in [`PackProgress::Finished`], or in
    /// [`PackProgress::Failed`] naming the cause. See the book:
    /// concepts/trust.md, "The guard".
    ///
    /// # Errors
    ///
    /// When the identity registry is refused or cannot be read; when an asset
    /// is refused or cannot be read; when an external file exists but cannot be
    /// read; when an index entry has no path of its own; when `data.pack`,
    /// `index.bin` or `manifest.bin` cannot be written, or the manifest signed.
    pub fn build(self) -> Result<PackOutput> {
        let built = self.build_files();
        if let Err(error) = &built {
            self.send_progress(PackProgress::Failed {
                message: format!("{error:#}"),
            });
        }
        built
    }

    /// Builds the pack as [`Self::build`] says, but sends no `Failed`.
    fn build_files(&self) -> Result<PackOutput> {
        std::fs::create_dir_all(self.dest_dir).with_context(|| {
            format!(
                "Failed to create pack destination: {}",
                self.dest_dir.display()
            )
        })?;

        // Through the editor's identity registry, so a renamed asset keeps its
        // frozen UUID in the pack, as in development.
        // Build Game runs for a trusted project only.
        let guard_of =
            |root: &Path| FileGuard::new(root, Trust::Project, Arc::clone(&self.refusals));
        let project_root = self.assets_root.parent().unwrap_or(self.assets_root);
        // Packed without it, a renamed asset would ship under another identity.
        let registry_path = project_root.join(ProjectFile::IdentityRegistry.rel_path());
        let registry = AssetIdRegistry::load(&guard_of(project_root)).map_err(|error| {
            anyhow!(
                "Pack: the asset identity registry {} is not read: {error}",
                registry_path.display()
            )
        })?;
        if !registry.is_read() {
            return Err(anyhow!(
                "Pack: the asset identity registry {} is not read: it does not parse",
                registry_path.display()
            ));
        }
        let guard = guard_of(self.assets_root);
        let mut metadata = IndexBuilder::new(guard_of(self.assets_root))
            .with_registry(&registry)
            .build_metadata()
            .context("Pack: failed to build asset metadata")?;
        let ExternalFiles {
            to_pack: external_paths,
            mut left_out,
        } = external_paths(&guard, &metadata)?;
        let total = metadata.len();
        // Progress counts the files read: the assets and the external files,
        // a refused one included. A sub-asset has no bytes, so is not one of
        // them, though it stays in the index and in the header's count.
        let streamed_total = metadata
            .iter()
            .filter(|meta| !matches!(meta.default_source(), Some(AssetSource::Sub { .. })))
            .count()
            + external_paths.len();
        let mut streamed = 0usize;

        let data_pack_path = self.dest_dir.join("data.pack");
        let mut data_file = File::create(&data_pack_path)
            .with_context(|| format!("Failed to create {}", data_pack_path.display()))?;

        // Offsets recorded below are relative to the asset region after this
        // header.
        // The LZ4 flag is set once an entry is written compressed, when the
        // header is written again after the entries.
        let mut flags = 0u32;
        if self.signer.is_some() {
            flags |= PACK_FLAG_MANIFEST;
        }
        PackLoader::write_header(&mut data_file, total as u32, flags)
            .context("Failed to write pack header")?;

        let mut manifest = self.signer.map(|_| PackManifest::new());
        let mut writer = EntryWriter {
            file: &mut data_file,
            compression: self.compression,
            offset: 0,
            any_lz4: false,
        };

        for meta in metadata.iter_mut() {
            let rel_path = match meta.default_source() {
                Some(AssetSource::Path(p)) => p.clone(),
                // A sub-asset has no bytes of its own: it is extracted from
                // its container, which is packed under its own uuid. Its
                // entry goes to the packed index as it is.
                Some(AssetSource::Sub { .. }) => continue,
                Some(AssetSource::Packed { .. }) => {
                    return Err(anyhow!(
                        "Pack: asset {:?} already has a Packed variant; \
                         IndexBuilder must produce Path variants",
                        meta.uuid
                    ));
                }
                None => {
                    return Err(anyhow!(
                        "Pack: asset {:?} has no 'default' variant",
                        meta.uuid
                    ));
                }
            };

            let rel_str = asset_key(&rel_path);
            self.send_progress(PackProgress::Started {
                rel_path: rel_str.clone(),
                current: streamed,
                total: streamed_total,
            });
            streamed += 1;

            let raw = guard
                .read(&rel_path)
                .with_context(|| format!("Pack: failed to read the asset '{rel_str}'"))?;
            // Uncompressed: the runtime checks the bytes after decompressing them.
            if let Some(m) = manifest.as_mut() {
                m.insert(meta.uuid, &raw);
            }
            let written = writer.write(&rel_str, raw)?;
            meta.variants
                .insert(AssetMetadata::DEFAULT_VARIANT.to_owned(), written.source());
        }

        let mut external_files = Vec::with_capacity(external_paths.len());
        for (path, kind) in external_paths {
            self.send_progress(PackProgress::Started {
                rel_path: path.clone(),
                current: streamed,
                total: streamed_total,
            });
            streamed += 1;
            let raw = match guard.read_as(&path, kind) {
                Ok(raw) => raw,
                Err(ReadError::Refused(refusal)) => {
                    log::warn!("Pack: the external file '{path}' is not packed: {refusal}");
                    left_out.push(LeftOutFile {
                        path,
                        reason: LeftOutReason::Refused(refusal.reason),
                    });
                    continue;
                }
                Err(error) if error.is_missing() => {
                    log::warn!("Pack: the external file '{path}' is not packed: it is missing");
                    left_out.push(LeftOutFile {
                        path,
                        reason: LeftOutReason::Missing,
                    });
                    continue;
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Pack: failed to read the external file '{path}': {e}"
                    ))
                }
            };
            if let Some(m) = manifest.as_mut() {
                m.insert_external_file(&path, &raw);
            }
            external_files.push(writer.write(&path, raw)?);
        }
        let offset = writer.offset;
        if writer.any_lz4 {
            flags |= PACK_FLAG_LZ4;
        }

        data_file
            .seek(SeekFrom::Start(0))
            .context("Failed to rewind data.pack")?;
        PackLoader::write_header(&mut data_file, total as u32, flags)?;
        data_file.flush().context("Failed to flush data.pack")?;
        drop(data_file);

        let index_bin_path = self.dest_dir.join("index.bin");
        let encoded = AssetIndex::encode(&metadata, &external_files)
            .map_err(|e| anyhow!("Failed to encode index: {}", e))?;
        std::fs::write(&index_bin_path, &encoded)
            .with_context(|| format!("Failed to write {}", index_bin_path.display()))?;

        let manifest_bin = if let (Some(m), Some(signer)) = (manifest, self.signer) {
            let manifest_path = self.dest_dir.join("manifest.bin");
            // Signed over the index just written: a pack whose index or
            // manifest changed does not verify.
            let manifest_bytes = signer
                .sign(&m, &encoded)
                .and_then(|signed| signed.encode())
                .context("Failed to sign the manifest")?;
            std::fs::write(&manifest_path, &manifest_bytes)
                .with_context(|| format!("Failed to write {}", manifest_path.display()))?;
            Some(manifest_path)
        } else {
            None
        };

        let pack_bytes = offset;
        self.send_progress(PackProgress::Finished {
            asset_count: total,
            pack_bytes,
        });

        left_out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(PackOutput {
            index_bin: index_bin_path,
            data_pack: data_pack_path,
            manifest_bin,
            asset_count: total,
            left_out,
            pack_bytes,
        })
    }

    fn send_progress(&self, ev: PackProgress) {
        if let Some(tx) = &self.progress {
            // Best-effort — disconnected receiver is fine.
            let _ = tx.send(ev);
        }
    }
}

#[cfg(test)]
mod tests;
