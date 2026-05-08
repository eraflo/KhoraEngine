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

//! Canonical asset index builder.
//!
//! Walks a project's `assets/` directory and produces an [`AssetMetadata`] list
//! suitable for [`crate::vfs::VirtualFileSystem::new`] — used by the editor to
//! build an in-memory index at boot, and by the pack builder (Phase 2) to
//! produce reproducible release archives.
//!
//! # Determinism
//!
//! Files are sorted by their **forward-slash relative path** (lexicographic on
//! the UTF-8 string) so the resulting metadata vector — and therefore the
//! bincode-encoded index — is byte-identical across invocations. This is what
//! lets CI compare two pack-builder runs and assert reproducibility.
//!
//! # UUID stability
//!
//! Each asset's [`AssetUUID`] is derived via
//! [`AssetUUID::new_v5`] from the **forward-slash relative path** (e.g.
//! `"textures/wood.png"`). The same file produces the same UUID whether
//! the editor scans the project in dev mode (FileLoader) or the pack builder
//! produces a release archive (PackLoader). This is the foundation that makes
//! the dev/release transparency promise of the VFS work.

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{AssetMetadata, AssetSource, AssetUUID};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Returns the canonical asset type name for a file extension, or `None` if
/// the extension isn't recognized.
///
/// The mapping is the **single source of truth** shared by:
/// - the editor's in-memory index (this module),
/// - the asset browser's tile categorization,
/// - the pack builder (Phase 2).
///
/// All names are lower-case to match the existing decoder registrations
/// (`crates/khora-agents/tests/asset_loading_test.rs:162` registers
/// `"texture"`).
///
/// Files whose extension isn't in this list are skipped during the scan —
/// they don't enter the VFS. The asset browser still surfaces them as
/// "unknown" via a separate filesystem walk if needed (Phase 1 doesn't).
pub fn asset_type_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        // Mesh formats
        "gltf" | "glb" | "obj" | "fbx" => Some("mesh"),
        // Texture formats
        "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" => Some("texture"),
        // Audio formats
        "wav" | "ogg" | "mp3" | "flac" => Some("audio"),
        // Shader formats
        "wgsl" | "hlsl" | "glsl" => Some("shader"),
        // Font formats
        "ttf" | "otf" => Some("font"),
        // Scene formats (Khora scene files)
        "kscene" | "scene" => Some("scene"),
        // Material formats
        "kmat" | "mat" => Some("material"),
        // Script formats — data, hot-reloadable, future custom language
        "kscript" => Some("script"),
        // Prefab formats (Phase 5 — instanced via SerializationService)
        "kprefab" => Some("prefab"),
        _ => None,
    }
}

/// Recursive scanner that turns a project's `assets/` directory into an
/// `AssetMetadata` list ready for the VFS.
///
/// See module documentation for determinism and UUID stability guarantees.
pub struct IndexBuilder<'a> {
    assets_root: &'a Path,
}

impl<'a> IndexBuilder<'a> {
    /// Creates a new builder rooted at `assets_root`.
    ///
    /// `assets_root` must be the **assets directory of a project**, not the
    /// project root — the relative paths recorded in `AssetMetadata` (and
    /// hence the UUIDs) are computed relative to it.
    pub fn new(assets_root: &'a Path) -> Self {
        Self { assets_root }
    }

    /// Walks the assets root and produces a sorted, deterministic
    /// `Vec<AssetMetadata>`.
    ///
    /// Returns an empty vector if `assets_root` doesn't exist — the editor
    /// tolerates fresh projects whose `assets/` directory hasn't been
    /// populated yet.
    pub fn build_metadata(&self) -> Result<Vec<AssetMetadata>> {
        if !self.assets_root.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<(String, PathBuf, &'static str)> = Vec::new();

        for entry in walkdir::WalkDir::new(self.assets_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let abs = entry.path();
            let rel = match abs.strip_prefix(self.assets_root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };
            let ext = match rel.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };
            let Some(type_name) = asset_type_for_extension(ext) else {
                continue;
            };
            let rel_fwd = rel_to_forward_slash(&rel);
            entries.push((rel_fwd, rel, type_name));
        }

        // Sort by forward-slash relative path for byte-deterministic output.
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut metadata = Vec::with_capacity(entries.len());
        for (rel_fwd, rel_path, type_name) in entries {
            let uuid = AssetUUID::new_v5(&rel_fwd);
            let mut variants = HashMap::with_capacity(1);
            variants.insert("default".to_string(), AssetSource::Path(rel_path.clone()));
            metadata.push(AssetMetadata {
                uuid,
                source_path: rel_path,
                asset_type_name: type_name.to_string(),
                dependencies: Vec::new(),
                variants,
                tags: Vec::new(),
            });
        }
        Ok(metadata)
    }

    /// Convenience: builds metadata and bincode-encodes it into the byte
    /// vector that [`crate::vfs::VirtualFileSystem::new`] expects.
    pub fn build_index_bytes(&self) -> Result<Vec<u8>> {
        let metadata = self.build_metadata()?;
        let cfg = bincode::config::standard();
        bincode::serde::encode_to_vec(&metadata, cfg)
            .map_err(|e| anyhow!("Failed to encode asset index: {}", e))
            .context("IndexBuilder::build_index_bytes")
    }
}

/// Normalizes a relative path to forward-slash separators so UUIDs are
/// platform-agnostic (`textures\foo.png` on Windows would otherwise hash
/// differently from `textures/foo.png` on Linux).
fn rel_to_forward_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::VirtualFileSystem;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn ext_canonical_mapping() {
        assert_eq!(asset_type_for_extension("png"), Some("texture"));
        assert_eq!(asset_type_for_extension("PNG"), Some("texture"));
        assert_eq!(asset_type_for_extension("gltf"), Some("mesh"));
        assert_eq!(asset_type_for_extension("kscene"), Some("scene"));
        assert_eq!(asset_type_for_extension("kscript"), Some("script"));
        assert_eq!(asset_type_for_extension("xyz"), None);
    }

    #[test]
    fn build_metadata_picks_up_known_files_and_skips_unknown() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("scenes")).unwrap();
        fs::write(root.join("textures").join("foo.png"), b"PNG").unwrap();
        fs::write(root.join("scenes").join("default.kscene"), b"SCN").unwrap();
        fs::write(root.join("README.md"), b"unknown").unwrap();

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        let names: Vec<_> = metadata
            .iter()
            .map(|m| m.asset_type_name.as_str())
            .collect();
        assert_eq!(names.len(), 2);
        // Sorted by rel-path so "scenes/..." < "textures/...".
        assert_eq!(metadata[0].asset_type_name, "scene");
        assert_eq!(metadata[1].asset_type_name, "texture");
        // UUIDs derived from forward-slash relative paths.
        assert_eq!(metadata[0].uuid, AssetUUID::new_v5("scenes/default.kscene"));
        assert_eq!(metadata[1].uuid, AssetUUID::new_v5("textures/foo.png"));
    }

    #[test]
    fn build_metadata_is_deterministic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("audio")).unwrap();
        fs::write(root.join("textures").join("a.png"), b"a").unwrap();
        fs::write(root.join("textures").join("b.png"), b"b").unwrap();
        fs::write(root.join("audio").join("c.wav"), b"c").unwrap();

        let bytes_a = IndexBuilder::new(root).build_index_bytes().unwrap();
        let bytes_b = IndexBuilder::new(root).build_index_bytes().unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "two consecutive builds must be byte-equal"
        );
    }

    #[test]
    fn build_index_bytes_round_trip_through_vfs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::write(root.join("textures").join("foo.png"), b"PNG").unwrap();

        let bytes = IndexBuilder::new(root).build_index_bytes().unwrap();
        let vfs = VirtualFileSystem::new(&bytes).expect("VFS must decode our index");
        assert_eq!(vfs.asset_count(), 1);
        let uuid = AssetUUID::new_v5("textures/foo.png");
        let meta = vfs.get_metadata(&uuid).expect("VFS must surface the asset");
        assert_eq!(meta.asset_type_name, "texture");
    }

    #[test]
    fn build_metadata_empty_root_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let metadata = IndexBuilder::new(dir.path()).build_metadata().unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn build_metadata_missing_root_returns_empty_vec() {
        let metadata = IndexBuilder::new(Path::new("/__nonexistent__"))
            .build_metadata()
            .unwrap();
        assert!(metadata.is_empty());
    }
}
