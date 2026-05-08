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

/// Returns the canonical asset type name for a file extension.
///
/// The well-known set is the **single source of truth** shared by:
/// - the editor's in-memory index (this module),
/// - the asset browser's tile categorization,
/// - the pack builder (Phase 2).
///
/// All names are lower-case to match the existing decoder registrations
/// (`crates/khora-agents/tests/asset_loading_test.rs:162` registers
/// `"texture"`).
///
/// Unknown extensions return `Some(<extension>)` rather than `None` —
/// the engine tracks **everything** under `assets/` regardless of
/// whether it has a dedicated decoder yet. New file kinds show up in
/// the asset browser automatically; adding a decoder only changes how
/// they're consumed at runtime, not whether they enter the VFS.
pub fn asset_type_for_extension(ext: &str) -> Option<String> {
    let lower = ext.to_ascii_lowercase();
    let canonical: Option<&'static str> = match lower.as_str() {
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
    };
    Some(canonical.map(|s| s.to_string()).unwrap_or(lower))
}

/// Asset type assigned to files with no extension at all. These still
/// enter the VFS so they're visible in the asset browser, but the
/// engine has no way to dispatch a decoder until the user renames or
/// re-classifies them.
pub const EXTENSIONLESS_ASSET_TYPE: &str = "blob";

/// File-name prefixes the scan ignores outright. These are OS / editor
/// scratch files that should never be part of the project.
const SCAN_IGNORE_PREFIXES: &[&str] = &[".", "~"];

/// File-name suffixes the scan ignores outright. Editor swap files,
/// build-tool temporaries, etc.
const SCAN_IGNORE_SUFFIXES: &[&str] = &[".tmp", ".swp", ".bak", "~"];

/// `true` if a file with the given name should be excluded from the
/// VFS scan and from hot-reload events. Catches OS scratch files
/// (`.DS_Store`, `.gitkeep`, …) and editor swap / backup files
/// (`*.tmp`, `*.swp`, `*.bak`, `*~`).
pub fn should_skip_file(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    if SCAN_IGNORE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }
    if SCAN_IGNORE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return true;
    }
    false
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

        let mut entries: Vec<(String, PathBuf, String)> = Vec::new();

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
            let file_name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_file(file_name) {
                continue;
            }
            let type_name = match rel.extension().and_then(|e| e.to_str()) {
                Some(ext) => asset_type_for_extension(ext)
                    .unwrap_or_else(|| EXTENSIONLESS_ASSET_TYPE.to_string()),
                None => EXTENSIONLESS_ASSET_TYPE.to_string(),
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
                asset_type_name: type_name,
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
        assert_eq!(asset_type_for_extension("png").as_deref(), Some("texture"));
        assert_eq!(asset_type_for_extension("PNG").as_deref(), Some("texture"));
        assert_eq!(asset_type_for_extension("gltf").as_deref(), Some("mesh"));
        assert_eq!(asset_type_for_extension("kscene").as_deref(), Some("scene"));
        assert_eq!(
            asset_type_for_extension("kscript").as_deref(),
            Some("script")
        );
        // Unknown extensions fall back to a per-extension bucket so the
        // file is still tracked (vs. silently dropped).
        assert_eq!(asset_type_for_extension("xyz").as_deref(), Some("xyz"));
        assert_eq!(asset_type_for_extension("MD").as_deref(), Some("md"));
    }

    #[test]
    fn build_metadata_tracks_every_file_under_assets() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("scenes")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("textures").join("foo.png"), b"PNG").unwrap();
        fs::write(root.join("scenes").join("default.kscene"), b"SCN").unwrap();
        fs::write(root.join("docs").join("README.md"), b"hello").unwrap();
        fs::write(root.join("notes"), b"raw").unwrap();
        // Editor / OS scratch — these MUST still be skipped.
        fs::write(root.join(".DS_Store"), b"junk").unwrap();
        fs::write(root.join("textures").join("foo.png.tmp"), b"junk").unwrap();
        fs::write(root.join("textures").join("foo.png~"), b"junk").unwrap();

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        let by_path: std::collections::HashMap<String, String> = metadata
            .iter()
            .map(|m| {
                (
                    rel_to_forward_slash(&m.source_path),
                    m.asset_type_name.clone(),
                )
            })
            .collect();

        assert_eq!(by_path.get("textures/foo.png").map(String::as_str), Some("texture"));
        assert_eq!(
            by_path.get("scenes/default.kscene").map(String::as_str),
            Some("scene")
        );
        // Unknown extension still flows into the index.
        assert_eq!(by_path.get("docs/README.md").map(String::as_str), Some("md"));
        // No-extension file too — bucketed as "blob".
        assert_eq!(by_path.get("notes").map(String::as_str), Some("blob"));
        // Scratch files dropped.
        assert!(!by_path.contains_key(".DS_Store"));
        assert!(!by_path.contains_key("textures/foo.png.tmp"));
        assert!(!by_path.contains_key("textures/foo.png~"));
        assert_eq!(metadata.len(), 4);
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
