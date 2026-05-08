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

//! Asset pack builder.
//!
//! Bundles a project's `assets/` directory into the two-file release layout
//! consumed by [`crate::asset::PackLoader`]:
//!
//! - `<dest>/data.pack` — concatenation of every asset's bytes, in the order
//!   produced by [`crate::asset::IndexBuilder`] (sorted by forward-slash
//!   relative path → deterministic).
//! - `<dest>/index.bin` — `bincode`-encoded `Vec<AssetMetadata>` with each
//!   metadata's `variants["default"]` rewritten from `AssetSource::Path(rel)`
//!   to `AssetSource::Packed { offset, size }`.
//!
//! UUIDs are produced by `IndexBuilder`, so they match what the editor sees
//! during dev mode — same project, same UUIDs in dev and release, by
//! construction.
//!
//! # Determinism
//!
//! Two consecutive `build()` calls on the same source tree produce
//! byte-identical `index.bin` and `data.pack`. CI relies on this for
//! reproducibility checks.
//!
//! # Progress
//!
//! Pass a `crossbeam_channel::Sender<PackProgress>` via [`PackBuilder::with_progress`]
//! to receive incremental events. The build thread sends one event per file
//! processed plus a final `Finished` event. The editor's Build dialog drives
//! its progress bar off this channel.

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Sender;
use khora_core::asset::{AssetSource, CompressionKind};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use super::{IndexBuilder, PackLoader, PackManifest, PACK_FLAG_LZ4, PACK_FLAG_MANIFEST};

/// One step of a pack build, suitable for driving a UI progress bar.
#[derive(Debug, Clone)]
pub enum PackProgress {
    /// A new asset is about to be streamed into `data.pack`.
    Started {
        /// Forward-slash relative path of the asset being processed.
        rel_path: String,
        /// 0-based index of this file in the build.
        current: usize,
        /// Total number of files in the build (constant across events).
        total: usize,
    },
    /// The build finished successfully.
    Finished {
        /// Number of assets packed.
        asset_count: usize,
        /// Total size of `data.pack` in bytes.
        pack_bytes: u64,
    },
    /// The build failed mid-way. The error message is best-effort — the
    /// caller should still surface the `Result` returned from `build()`.
    Failed {
        /// Human-readable description of the failure.
        message: String,
    },
}

/// Output of a successful [`PackBuilder::build`].
#[derive(Debug, Clone)]
pub struct PackOutput {
    /// Absolute path to the produced `index.bin`.
    pub index_bin: PathBuf,
    /// Absolute path to the produced `data.pack`.
    pub data_pack: PathBuf,
    /// Absolute path to the produced `manifest.bin` (BLAKE3 integrity
    /// records). `None` when manifest emission was disabled.
    pub manifest_bin: Option<PathBuf>,
    /// Number of assets indexed (mirrors `index.bin`'s entry count).
    pub asset_count: usize,
    /// Total size of `data.pack` in bytes.
    pack_bytes: u64,
}

impl PackOutput {
    /// Total size of `data.pack` in bytes.
    pub fn pack_bytes(&self) -> u64 {
        self.pack_bytes
    }
}

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
    write_manifest: bool,
}

impl<'a> PackBuilder<'a> {
    /// Creates a new builder. `assets_root` is the project's `assets/`
    /// directory (the same path that `ProjectVfs` watches in dev mode).
    /// `dest_dir` is where `index.bin` + `data.pack` will be written —
    /// created if missing.
    pub fn new(assets_root: &'a Path, dest_dir: &'a Path) -> Self {
        Self {
            assets_root,
            dest_dir,
            progress: None,
            compression: CompressionKind::None,
            write_manifest: false,
        }
    }

    /// Forwards incremental progress to `tx`. Drop the receiver to silently
    /// disable progress (the sends are best-effort and never block).
    pub fn with_progress(mut self, tx: Sender<PackProgress>) -> Self {
        self.progress = Some(tx);
        self
    }

    /// Sets the per-entry compression scheme. Default is
    /// [`CompressionKind::None`]. When set to LZ4, every asset is
    /// compressed individually so the runtime can decompress on demand
    /// (no global state, no streaming gzip).
    pub fn with_compression(mut self, compression: CompressionKind) -> Self {
        self.compression = compression;
        self
    }

    /// Emits a `manifest.bin` sidecar with BLAKE3 hashes of every
    /// uncompressed asset. The runtime opts in via
    /// `RuntimeConfig::verify_integrity` to detect corruption /
    /// tampering at load time.
    pub fn with_manifest(mut self, enable: bool) -> Self {
        self.write_manifest = enable;
        self
    }

    /// Performs the build. Side effects:
    ///
    /// - creates `<dest_dir>/` if missing,
    /// - writes `<dest_dir>/data.pack` (concatenation of asset bytes),
    /// - writes `<dest_dir>/index.bin` (`bincode`-encoded metadata).
    ///
    /// The two files together are everything `khora_runtime` needs to load
    /// the project at runtime via [`crate::asset::PackLoader`].
    pub fn build(self) -> Result<PackOutput> {
        std::fs::create_dir_all(self.dest_dir).with_context(|| {
            format!(
                "Failed to create pack destination: {}",
                self.dest_dir.display()
            )
        })?;

        let mut metadata = IndexBuilder::new(self.assets_root)
            .build_metadata()
            .context("Pack: failed to build asset metadata")?;
        let total = metadata.len();

        let data_pack_path = self.dest_dir.join("data.pack");
        let mut data_file = File::create(&data_pack_path)
            .with_context(|| format!("Failed to create {}", data_pack_path.display()))?;

        // Write the leading 24-byte header. Asset offsets recorded below
        // are relative to the start of the asset region (i.e. don't
        // include the header) — `PackLoader` adds `PACK_HEADER_SIZE` when
        // seeking. This keeps the index oblivious to the on-disk framing.
        let mut flags = 0u32;
        if self.compression == CompressionKind::Lz4 {
            flags |= PACK_FLAG_LZ4;
        }
        if self.write_manifest {
            flags |= PACK_FLAG_MANIFEST;
        }
        PackLoader::write_header(&mut data_file, total as u32, flags)
            .context("Failed to write pack header")?;

        let mut manifest = if self.write_manifest {
            Some(PackManifest::new())
        } else {
            None
        };

        let mut offset = 0u64;
        for (idx, meta) in metadata.iter_mut().enumerate() {
            // The default variant is `AssetSource::Path(rel)` from
            // IndexBuilder — extract the rel path before we overwrite it.
            let rel_path = match meta.variants.get("default") {
                Some(AssetSource::Path(p)) => p.clone(),
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

            let rel_str = rel_to_forward_slash(&rel_path);
            self.send_progress(PackProgress::Started {
                rel_path: rel_str.clone(),
                current: idx,
                total,
            });

            let abs = self.assets_root.join(&rel_path);
            let raw = std::fs::read(&abs)
                .with_context(|| format!("Failed to read asset bytes: {}", abs.display()))?;
            let uncompressed_size = raw.len() as u64;

            // Manifest hashes uncompressed bytes — the runtime computes
            // BLAKE3 after decompression so the check is independent of
            // the compression algorithm.
            if let Some(m) = manifest.as_mut() {
                m.insert(meta.uuid, &raw);
            }

            let (bytes_to_write, compression) = match self.compression {
                CompressionKind::None => (raw, CompressionKind::None),
                CompressionKind::Lz4 => {
                    let compressed = lz4_flex::block::compress(&raw);
                    // Skip compression when it makes the entry larger
                    // (already-compressed media: PNG, OGG, FBX).
                    if compressed.len() < raw.len() {
                        (compressed, CompressionKind::Lz4)
                    } else {
                        (raw, CompressionKind::None)
                    }
                }
            };
            let on_disk_size = bytes_to_write.len() as u64;

            data_file
                .write_all(&bytes_to_write)
                .with_context(|| format!("Failed to append {} to data.pack", rel_str))?;

            meta.variants.insert(
                "default".to_string(),
                AssetSource::Packed {
                    offset,
                    size: on_disk_size,
                    uncompressed_size,
                    compression,
                },
            );

            offset += on_disk_size;
        }

        // Flush before writing index — index references offsets into the
        // pack we just produced.
        data_file.flush().context("Failed to flush data.pack")?;
        drop(data_file);

        let index_bin_path = self.dest_dir.join("index.bin");
        let cfg = bincode::config::standard();
        let encoded = bincode::serde::encode_to_vec(&metadata, cfg)
            .map_err(|e| anyhow!("Failed to encode index: {}", e))?;
        std::fs::write(&index_bin_path, &encoded)
            .with_context(|| format!("Failed to write {}", index_bin_path.display()))?;

        let manifest_bin = if let Some(m) = manifest {
            let manifest_path = self.dest_dir.join("manifest.bin");
            let manifest_bytes = m.encode().context("Failed to encode manifest")?;
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

        Ok(PackOutput {
            index_bin: index_bin_path,
            data_pack: data_pack_path,
            manifest_bin,
            asset_count: total,
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

fn rel_to_forward_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AssetService, PackLoader};
    use crate::vfs::VirtualFileSystem;
    use khora_core::asset::AssetUUID;
    use khora_telemetry::MetricsRegistry;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn seed_project(root: &Path) {
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("scenes")).unwrap();
        fs::write(root.join("textures").join("a.png"), b"PNG-A").unwrap();
        fs::write(root.join("textures").join("b.png"), b"PNG-B").unwrap();
        fs::write(root.join("scenes").join("default.kscene"), b"SCENE").unwrap();
    }

    #[test]
    fn build_produces_index_and_pack() {
        let proj = tempdir().unwrap();
        let dest = tempdir().unwrap();
        seed_project(proj.path());

        let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();

        assert!(out.index_bin.exists());
        assert!(out.data_pack.exists());
        assert_eq!(out.asset_count, 3);
        assert_eq!(out.pack_bytes(), 5 + 5 + 5); // PNG-A + PNG-B + SCENE
    }

    #[test]
    fn build_is_deterministic() {
        let proj = tempdir().unwrap();
        let dest_a = tempdir().unwrap();
        let dest_b = tempdir().unwrap();
        seed_project(proj.path());

        let _ = PackBuilder::new(proj.path(), dest_a.path())
            .build()
            .unwrap();
        let _ = PackBuilder::new(proj.path(), dest_b.path())
            .build()
            .unwrap();

        let idx_a = std::fs::read(dest_a.path().join("index.bin")).unwrap();
        let idx_b = std::fs::read(dest_b.path().join("index.bin")).unwrap();
        assert_eq!(idx_a, idx_b, "index.bin must be byte-deterministic");

        let pack_a = std::fs::read(dest_a.path().join("data.pack")).unwrap();
        let pack_b = std::fs::read(dest_b.path().join("data.pack")).unwrap();
        assert_eq!(pack_a, pack_b, "data.pack must be byte-deterministic");
    }

    #[test]
    fn pack_round_trips_through_packloader() {
        let proj = tempdir().unwrap();
        let dest = tempdir().unwrap();
        seed_project(proj.path());

        let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();

        // Read back the index, hand it to a VFS, and read each asset through
        // a PackLoader to confirm the offsets/sizes line up with the bytes
        // we appended (and that the leading 16-byte header is consumed
        // transparently by PackLoader's seek-relative-to-asset-region logic).
        let index_bytes = std::fs::read(&out.index_bin).unwrap();
        let vfs = VirtualFileSystem::new(&index_bytes).unwrap();
        let pack_file = std::fs::File::open(&out.data_pack).unwrap();
        let pack_loader = PackLoader::new(pack_file).expect("valid pack header");
        assert_eq!(pack_loader.header().asset_count, vfs.asset_count() as u32);
        let metrics = Arc::new(MetricsRegistry::new());
        let mut svc = AssetService::new(&index_bytes, Box::new(pack_loader), metrics).unwrap();

        // Verify each asset by raw load_raw, since we don't register any
        // decoders here (binary blobs aren't real PNGs).
        let known = [
            ("scenes/default.kscene", &b"SCENE"[..]),
            ("textures/a.png", &b"PNG-A"[..]),
            ("textures/b.png", &b"PNG-B"[..]),
        ];
        for (rel, expected) in known {
            let uuid = AssetUUID::new_v5(rel);
            assert!(
                vfs.get_metadata(&uuid).is_some(),
                "uuid for {} missing",
                rel
            );
            let bytes = svc.load_raw(&uuid).unwrap();
            assert_eq!(bytes, expected, "round-trip mismatch for {}", rel);
        }
    }

    #[test]
    fn progress_channel_emits_started_and_finished() {
        let proj = tempdir().unwrap();
        let dest = tempdir().unwrap();
        seed_project(proj.path());

        let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
        let _ = PackBuilder::new(proj.path(), dest.path())
            .with_progress(tx)
            .build()
            .unwrap();

        let mut started = 0usize;
        let mut finished = 0usize;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                PackProgress::Started { current, total, .. } => {
                    assert_eq!(total, 3);
                    assert!(current < total);
                    started += 1;
                }
                PackProgress::Finished { asset_count, .. } => {
                    assert_eq!(asset_count, 3);
                    finished += 1;
                }
                PackProgress::Failed { .. } => panic!("unexpected Failed event"),
            }
        }
        assert_eq!(started, 3);
        assert_eq!(finished, 1);
    }

    #[test]
    fn empty_assets_root_produces_empty_pack() {
        let proj = tempdir().unwrap();
        let dest = tempdir().unwrap();
        let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();
        assert_eq!(out.asset_count, 0);
        assert_eq!(out.pack_bytes(), 0);
        // Both files exist but are zero-byte.
        assert!(out.data_pack.exists());
        assert!(out.index_bin.exists());
    }
}
