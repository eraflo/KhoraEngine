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
//! suitable for [`crate::asset::AssetIndex::new`] — used by the editor to
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
//! Each asset's [`AssetUUID`] is resolved through the project's
//! [`AssetIdRegistry`] when one is supplied via [`IndexBuilder::with_registry`],
//! and otherwise **defaults** to [`AssetUUID::new_v5`] of the **forward-slash
//! relative path** (e.g. `"textures/wood.png"`). The registry is authoritative:
//! once an asset has been renamed/moved in the editor its identity is *frozen*
//! there, so the same file keeps the same UUID across renames. For any asset
//! without a registry entry the path-derived default applies, which is what
//! makes pre-registry projects and the pack builder agree by construction (the
//! pack builder loads the same registry, so dev and release resolve identically).

use crate::asset::container::{SubAssetEntry, SubAssetListerRegistration};
use crate::asset::dependencies::{extract_dependencies, type_has_dependency_extractor};
use crate::asset::id_registry::AssetIdRegistry;
use anyhow::{anyhow, Context, Result};
use khora_core::asset::{asset_key, AssetMetadata, AssetSource, AssetUUID};
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
        // Script formats. `.erg` is Ergon, the engine's own language; `kscript`
        // predates it and is kept so a project that used it still indexes.
        khora_script::modules::EXTENSION | "kscript" => Some("script"),
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
    registry: Option<&'a AssetIdRegistry>,
}

impl<'a> IndexBuilder<'a> {
    /// Creates a new builder rooted at `assets_root`.
    ///
    /// `assets_root` must be the **assets directory of a project**, not the
    /// project root — the relative paths recorded in `AssetMetadata` (and
    /// hence the UUIDs) are computed relative to it.
    ///
    /// Without [`Self::with_registry`], UUIDs are the path-derived
    /// `new_v5` default (the pre-registry behaviour).
    pub fn new(assets_root: &'a Path) -> Self {
        Self {
            assets_root,
            registry: None,
        }
    }

    /// Resolves each asset's UUID through `registry` (frozen identities win;
    /// unfrozen paths still fall back to `new_v5`). Supply the same registry in
    /// dev (editor VFS) and release (pack builder) so UUIDs match by
    /// construction.
    pub fn with_registry(mut self, registry: &'a AssetIdRegistry) -> Self {
        self.registry = Some(registry);
        self
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

        // The absolute path is retained alongside the relative one so handled
        // asset types can have their bytes read for dependency extraction after
        // sorting. Leaf types are never read (see below).
        let mut entries: Vec<(String, PathBuf, PathBuf, String)> = Vec::new();

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
            let rel_fwd = asset_key(&rel);
            entries.push((rel_fwd, rel, abs.to_path_buf(), type_name));
        }

        // Sort by forward-slash relative path for byte-deterministic output.
        // Dependencies are extracted *after* sorting so the order they are
        // visited never affects the result.
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut metadata = Vec::with_capacity(entries.len());
        for (rel_fwd, rel_path, abs_path, type_name) in entries {
            // Registry-frozen identity if present, else the path-derived default.
            let uuid = match self.registry {
                Some(reg) => reg.resolve(&rel_fwd),
                None => AssetUUID::new_v5(&rel_fwd),
            };
            // Only read file contents for types whose references we can parse.
            // Textures, audio, meshes, etc. are never read — this preserves the
            // builder's stat-only fast path and avoids a per-file I/O cliff.
            let dependencies = if type_has_dependency_extractor(&type_name) {
                match std::fs::read(&abs_path) {
                    Ok(bytes) => extract_dependencies(&type_name, &rel_fwd, &bytes),
                    Err(e) => {
                        log::warn!(
                            "asset index: failed to read '{}' for dependency extraction: {e}",
                            rel_fwd
                        );
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            let mut variants = HashMap::with_capacity(1);
            variants.insert("default".to_string(), AssetSource::Path(rel_path.clone()));
            metadata.push(AssetMetadata {
                uuid,
                source_path: rel_path.clone(),
                asset_type_name: type_name.clone(),
                dependencies,
                variants,
                tags: Vec::new(),
            });
            push_sub_assets(
                &mut metadata,
                uuid,
                &rel_fwd,
                &rel_path,
                &abs_path,
                &type_name,
            );
        }
        Ok(metadata)
    }

    /// Convenience: builds metadata and bincode-encodes it into the byte
    /// vector that [`crate::asset::AssetIndex::new`] expects.
    pub fn build_index_bytes(&self) -> Result<Vec<u8>> {
        let metadata = self.build_metadata()?;
        let cfg = bincode::config::standard();
        bincode::serde::encode_to_vec(&metadata, cfg)
            .map_err(|e| anyhow!("Failed to encode asset index: {}", e))
            .context("IndexBuilder::build_index_bytes")
    }
}

/// Appends one entry per sub-asset the listers of `type_name` find in the
/// file, right after the file's own entry. Only file types with a lister are
/// read; a lister that fails is logged and leaves the file's own entry alone.
fn push_sub_assets(
    metadata: &mut Vec<AssetMetadata>,
    container: AssetUUID,
    rel_fwd: &str,
    rel_path: &Path,
    abs_path: &Path,
    type_name: &str,
) {
    let mut listers = inventory::iter::<SubAssetListerRegistration>
        .into_iter()
        .filter(|lister| lister.type_name == type_name)
        .peekable();
    if listers.peek().is_none() {
        return;
    }
    let bytes = match std::fs::read(abs_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("asset index: failed to read '{rel_fwd}' to list its sub-assets: {e}");
            return;
        }
    };
    for lister in listers {
        let entries = match (lister.list)(&bytes) {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("asset index: listing the sub-assets of '{rel_fwd}' failed: {e:#}");
                continue;
            }
        };
        for SubAssetEntry { label, type_name } in entries {
            let mut variants = HashMap::with_capacity(1);
            variants.insert(
                "default".to_string(),
                AssetSource::Sub {
                    container,
                    label: label.clone(),
                },
            );
            metadata.push(AssetMetadata {
                uuid: AssetUUID::sub_asset(container, &label),
                source_path: rel_path.to_path_buf(),
                asset_type_name: type_name,
                dependencies: Vec::new(),
                variants,
                tags: Vec::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetIndex;
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
    fn index_builder_honours_frozen_registry_uuid() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("meshes")).unwrap();
        fs::write(root.join("meshes").join("hero.gltf"), b"GLTF").unwrap();

        // Default (no registry): UUID is derived from the path.
        let default_uuid = AssetUUID::new_v5("meshes/hero.gltf");
        let md = IndexBuilder::new(root).build_metadata().unwrap();
        assert_eq!(md[0].uuid, default_uuid);

        // Freeze a *different* identity for that path — exactly what a rename
        // does (the file kept the UUID it had under its previous name).
        let mut reg = AssetIdRegistry::load(root);
        let frozen = AssetUUID::new_v5("meshes/protagonist.gltf");
        reg.freeze("meshes/hero.gltf", frozen);

        let md2 = IndexBuilder::new(root)
            .with_registry(&reg)
            .build_metadata()
            .unwrap();
        assert_eq!(
            md2[0].uuid, frozen,
            "IndexBuilder must resolve the frozen registry UUID, not new_v5(path)"
        );
        assert_ne!(md2[0].uuid, default_uuid);
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
            .map(|m| (asset_key(&m.source_path), m.asset_type_name.clone()))
            .collect();

        assert_eq!(
            by_path.get("textures/foo.png").map(String::as_str),
            Some("texture")
        );
        assert_eq!(
            by_path.get("scenes/default.kscene").map(String::as_str),
            Some("scene")
        );
        // Unknown extension still flows into the index.
        assert_eq!(
            by_path.get("docs/README.md").map(String::as_str),
            Some("md")
        );
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
        let vfs = AssetIndex::new(&bytes).expect("VFS must decode our index");
        assert_eq!(vfs.asset_count(), 1);
        let uuid = AssetUUID::new_v5("textures/foo.png");
        let meta = vfs.get_metadata(&uuid).expect("VFS must surface the asset");
        assert_eq!(meta.asset_type_name, "texture");
    }

    /// Writes a `StandardMaterial` to `path` as `.kmat` RON — the on-disk form
    /// the index builder reads when extracting dependencies.
    fn write_kmat(path: &Path, material: &khora_core::asset::StandardMaterial) {
        let json =
            khora_data::scene::material_to_json(material).expect("material serializes to JSON");
        let ron = ron::ser::to_string(&json).expect("material JSON encodes to RON");
        fs::write(path, ron).unwrap();
    }

    #[test]
    fn material_metadata_lists_its_textures_sorted_and_deduped() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(root.join("textures").join("base.png"), b"PNG").unwrap();
        fs::write(root.join("textures").join("normal.png"), b"PNG").unwrap();

        // The UUIDs a material stores are derived from the texture's
        // forward-slash relative path — identical to the UUIDs the index
        // assigns those texture files.
        let base_uuid = AssetUUID::new_v5("textures/base.png");
        let normal_uuid = AssetUUID::new_v5("textures/normal.png");
        let material = khora_core::asset::StandardMaterial {
            base_color_texture: Some(base_uuid),
            // Reuse the base texture in a second slot to exercise dedup.
            metallic_roughness_texture: Some(base_uuid),
            normal_map: Some(normal_uuid),
            ..Default::default()
        };
        write_kmat(&root.join("materials").join("wood.kmat"), &material);

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        let mat_meta = metadata
            .iter()
            .find(|m| m.asset_type_name == "material")
            .expect("the material must be indexed");

        let mut expected = vec![base_uuid, normal_uuid];
        expected.sort();
        assert_eq!(
            mat_meta.dependencies, expected,
            "material deps must be both textures, sorted and deduped"
        );

        // The texture entries themselves must carry no dependencies — they are
        // leaf assets and are never read/parsed.
        for tex in metadata.iter().filter(|m| m.asset_type_name == "texture") {
            assert!(
                tex.dependencies.is_empty(),
                "texture '{}' must have no dependencies",
                asset_key(&tex.source_path)
            );
        }
    }

    #[test]
    fn build_index_bytes_is_byte_equal_with_material_and_textures() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(root.join("textures").join("a.png"), b"A").unwrap();
        fs::write(root.join("textures").join("b.png"), b"B").unwrap();

        let material = khora_core::asset::StandardMaterial {
            base_color_texture: Some(AssetUUID::new_v5("textures/a.png")),
            normal_map: Some(AssetUUID::new_v5("textures/b.png")),
            ..Default::default()
        };
        write_kmat(&root.join("materials").join("m.kmat"), &material);

        let bytes_a = IndexBuilder::new(root).build_index_bytes().unwrap();
        let bytes_b = IndexBuilder::new(root).build_index_bytes().unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "an index over a material + textures must be byte-deterministic"
        );
    }

    #[test]
    fn corrupt_material_still_builds_with_empty_deps() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(
            root.join("materials").join("broken.kmat"),
            b"this is not valid kmat RON",
        )
        .unwrap();

        let metadata = IndexBuilder::new(root)
            .build_metadata()
            .expect("a corrupt material must not abort the index build");
        let mat_meta = metadata
            .iter()
            .find(|m| m.asset_type_name == "material")
            .expect("the corrupt material is still indexed");
        assert!(
            mat_meta.dependencies.is_empty(),
            "a corrupt material yields empty deps, not a panic"
        );
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

    // ── Sub-assets ──────────────────────────────────────────────────────────

    /// Index type of the test-only container files (`*.ksubtest`). The
    /// extension is unknown to `asset_type_for_extension`, so it falls back to
    /// its own bucket and no production lister claims it.
    const TEST_CONTAINER_TYPE: &str = "ksubtest";

    /// Index type of test-only container files whose lister always fails.
    const FAILING_CONTAINER_TYPE: &str = "ksubfail";

    /// Lists one sub-asset per non-empty line `"<type> <label>"`.
    fn list_test_container(bytes: &[u8]) -> Result<Vec<crate::asset::SubAssetEntry>> {
        let text = std::str::from_utf8(bytes)?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (type_name, label) = line
                    .split_once(' ')
                    .ok_or_else(|| anyhow!("malformed test container line: {line:?}"))?;
                Ok(crate::asset::SubAssetEntry {
                    label: label.to_string(),
                    type_name: type_name.to_string(),
                })
            })
            .collect()
    }

    fn list_failing_container(_bytes: &[u8]) -> Result<Vec<crate::asset::SubAssetEntry>> {
        Err(anyhow!("this container cannot be listed"))
    }

    inventory::submit! {
        crate::asset::SubAssetListerRegistration {
            type_name: TEST_CONTAINER_TYPE,
            list: list_test_container,
        }
    }

    inventory::submit! {
        crate::asset::SubAssetListerRegistration {
            type_name: FAILING_CONTAINER_TYPE,
            list: list_failing_container,
        }
    }

    const HERO_CONTAINER: &[u8] = b"mesh mesh/Body\nmaterial material/Skin\n";

    fn find(metadata: &[AssetMetadata], uuid: AssetUUID) -> &AssetMetadata {
        metadata
            .iter()
            .find(|m| m.uuid == uuid)
            .unwrap_or_else(|| panic!("no entry with uuid {uuid} in the index"))
    }

    fn assert_sub_entry(
        metadata: &[AssetMetadata],
        container: AssetUUID,
        container_path: &Path,
        label: &str,
        type_name: &str,
    ) {
        let sub = find(metadata, AssetUUID::sub_asset(container, label));
        assert_eq!(sub.asset_type_name, type_name, "type of sub '{label}'");
        assert_eq!(
            sub.source_path, container_path,
            "a sub entry's source path is its container's path"
        );
        assert!(sub.dependencies.is_empty(), "sub '{label}' has no deps");
        assert!(sub.tags.is_empty(), "sub '{label}' has no tags");
        assert_eq!(sub.variants.len(), 1, "sub '{label}' has one variant");
        match sub.variants.get("default") {
            Some(AssetSource::Sub {
                container: c,
                label: l,
            }) => {
                assert_eq!(*c, container, "sub '{label}' names its container");
                assert_eq!(l, label, "sub '{label}' carries its label");
            }
            other => panic!("sub '{label}' default variant must be Sub, got {other:?}"),
        }
    }

    #[test]
    fn index_lists_one_sub_entry_per_listed_sub_asset() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();
        // Same text, but a texture: no lister of its type, so no sub entries.
        fs::write(root.join("textures").join("a.png"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        assert_eq!(
            metadata.len(),
            4,
            "the container, its two subs and the texture: {metadata:#?}"
        );

        // The file's own entry is unchanged.
        let rel = Path::new("models").join("hero.ksubtest");
        let file_uuid = AssetUUID::new_v5("models/hero.ksubtest");
        let own = find(&metadata, file_uuid);
        assert_eq!(own.asset_type_name, TEST_CONTAINER_TYPE);
        assert_eq!(own.source_path, rel);
        assert!(matches!(
            own.variants.get("default"),
            Some(AssetSource::Path(p)) if *p == rel
        ));

        assert_sub_entry(&metadata, file_uuid, &rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, file_uuid, &rel, "material/Skin", "material");

        let texture = find(&metadata, AssetUUID::new_v5("textures/a.png"));
        assert_eq!(texture.asset_type_name, "texture");
    }

    #[test]
    fn sub_entries_follow_their_file_entry() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("a.ksubtest"), b"mesh mesh/A\n").unwrap();
        fs::write(root.join("models").join("b.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        let a = AssetUUID::new_v5("models/a.ksubtest");
        let b = AssetUUID::new_v5("models/b.ksubtest");
        let order: Vec<AssetUUID> = metadata.iter().map(|m| m.uuid).collect();
        assert_eq!(order.len(), 5, "two files and three subs: {metadata:#?}");
        assert_eq!(order[0], a);
        assert_eq!(order[1], AssetUUID::sub_asset(a, "mesh/A"));
        assert_eq!(order[2], b);
        let mut b_subs = vec![order[3], order[4]];
        b_subs.sort();
        let mut expected = vec![
            AssetUUID::sub_asset(b, "mesh/Body"),
            AssetUUID::sub_asset(b, "material/Skin"),
        ];
        expected.sort();
        assert_eq!(b_subs, expected);
    }

    /// A rename freezes the file's old uuid in the registry; its sub uuids are
    /// derived from that frozen uuid, so they survive the rename too.
    #[test]
    fn sub_uuids_derive_from_the_registry_frozen_file_uuid() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();

        let mut reg = AssetIdRegistry::load(root);
        let frozen = AssetUUID::new_v5("models/protagonist.ksubtest");
        reg.freeze("models/hero.ksubtest", frozen);

        let metadata = IndexBuilder::new(root)
            .with_registry(&reg)
            .build_metadata()
            .unwrap();
        assert_eq!(
            metadata.len(),
            3,
            "the file and its two subs: {metadata:#?}"
        );
        let rel = Path::new("models").join("hero.ksubtest");
        assert_eq!(find(&metadata, frozen).source_path, rel);
        assert_sub_entry(&metadata, frozen, &rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, frozen, &rel, "material/Skin", "material");

        let path_default = AssetUUID::new_v5("models/hero.ksubtest");
        assert!(
            metadata
                .iter()
                .all(|m| m.uuid != AssetUUID::sub_asset(path_default, "mesh/Body")),
            "sub uuids must not derive from the path default once the file is frozen"
        );
    }

    #[test]
    fn a_lister_error_keeps_the_file_entry_and_spares_the_other_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("broken.ksubfail"), b"anything").unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(root)
            .build_metadata()
            .expect("a lister error must not abort the index build");

        let broken_rel = Path::new("models").join("broken.ksubfail");
        let broken = find(&metadata, AssetUUID::new_v5("models/broken.ksubfail"));
        assert_eq!(broken.asset_type_name, FAILING_CONTAINER_TYPE);
        assert!(matches!(
            broken.variants.get("default"),
            Some(AssetSource::Path(p)) if *p == broken_rel
        ));
        assert_eq!(
            metadata
                .iter()
                .filter(|m| m.source_path == broken_rel)
                .count(),
            1,
            "the failing file adds no sub entry"
        );

        // The next file is still listed.
        let hero_rel = Path::new("models").join("hero.ksubtest");
        let hero = AssetUUID::new_v5("models/hero.ksubtest");
        assert_sub_entry(&metadata, hero, &hero_rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, hero, &hero_rel, "material/Skin", "material");
        assert_eq!(metadata.len(), 4);
    }

    #[test]
    fn an_index_with_sub_entries_is_deterministic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();
        fs::write(root.join("models").join("villain.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(root).build_metadata().unwrap();
        assert_eq!(metadata.len(), 6, "two files, two subs each: {metadata:#?}");

        let bytes_a = IndexBuilder::new(root).build_index_bytes().unwrap();
        let bytes_b = IndexBuilder::new(root).build_index_bytes().unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "two consecutive builds must be byte-equal"
        );

        // The `Sub` source round-trips through the index encoding.
        let vfs = AssetIndex::new(&bytes_a).expect("the index with subs decodes");
        assert_eq!(vfs.asset_count(), 6);
        let hero = AssetUUID::new_v5("models/hero.ksubtest");
        let body = vfs
            .get_metadata(&AssetUUID::sub_asset(hero, "mesh/Body"))
            .expect("the sub entry survives encoding");
        assert!(matches!(
            body.variants.get("default"),
            Some(AssetSource::Sub { container, label }) if *container == hero && label == "mesh/Body"
        ));
    }
}
