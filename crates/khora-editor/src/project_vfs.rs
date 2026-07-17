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

//! Project-scoped Virtual File System for the editor.
//!
//! Wraps the `khora-io` `AssetService` + `AssetWatcher` together with a
//! `FileLoader` rooted at `<project>/assets/`. This is the single I/O entry
//! point used by `scene_io`, the asset browser, and any other editor code
//! that needs to read or write project content.

use anyhow::{bail, Context, Result};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::renderer::api::scene::Mesh;
use khora_sdk::{
    AssetChangeEvent, AssetIdRegistry, AssetService, AssetWatcher, AssetWriter, FileLoader,
    FileSystemResolver, IndexBuilder, MeshDispatcher, MetricsRegistry, SoundData, SymphoniaDecoder,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// All project I/O, in one place.
///
/// `open` performs a recursive scan of `<root>/assets/`, builds an in-memory
/// VFS index (UUIDs derived from forward-slash relative paths via
/// `AssetUUID::new_v5`), constructs the `AssetService` with every default
/// decoder registered, and arms a recursive filesystem watcher for hot
/// reload. UUIDs match what a future pack-builder would produce, so the
/// dev/release transparency promise of the VFS is preserved.
pub struct ProjectVfs {
    pub root: PathBuf,
    pub assets_root: PathBuf,
    pub asset_service: AssetService,
    pub watcher: AssetWatcher,
    file_loader: FileLoader,
    /// Stable-identity registry (`<root>/.khora/asset-registry.ron`). The editor
    /// is the sole writer: file operations freeze/rename/remove entries so an
    /// asset keeps its UUID across renames and every reference keeps resolving.
    registry: AssetIdRegistry,
}

impl ProjectVfs {
    /// Opens (or creates) `<root>/assets/`, builds the VFS, and arms the
    /// watcher. Tolerates fresh projects whose `assets/` directory hasn't
    /// been populated yet — the resulting service has zero indexed assets,
    /// which is exactly what the asset browser will show.
    pub fn open(root: PathBuf, metrics: Arc<MetricsRegistry>) -> Result<Self> {
        let assets_root = root.join("assets");
        std::fs::create_dir_all(&assets_root).with_context(|| {
            format!(
                "Failed to ensure project assets directory exists: {}",
                assets_root.display()
            )
        })?;

        // The identity registry lives at the project root (sibling of
        // `assets/`), so it is never scanned, watched, or packed. Missing file →
        // empty registry → every path keeps its `new_v5` default.
        let registry = AssetIdRegistry::load(&root);
        // Materialize the file on first open so it's visible in the project and
        // confirms persistence is wired, even before the first rename.
        if let Err(e) = registry.ensure_file() {
            log::warn!("Could not create asset identity registry file: {e}");
        }

        let index_bytes = IndexBuilder::new(&assets_root)
            .with_registry(&registry)
            .build_index_bytes()
            .context("Failed to build initial project asset index")?;

        let file_loader = FileLoader::new(&assets_root);
        // Note: we hand a *clone* (a fresh FileLoader) to the AssetService so
        // that we keep our own `file_loader` available to implement
        // `AssetWriter` for scene saves. They both read/write the same root,
        // so this is consistent.
        let io = Box::new(FileLoader::new(&assets_root));
        let mut asset_service = AssetService::new(&index_bytes, io, metrics, None)
            .context("Failed to construct AssetService")?;

        // texture + font auto-register via inventory.
        asset_service.register_inventory_decoders();
        // audio + mesh are explicitly chosen by the consumer (see
        // doctrine in decoders/{audio,mesh}/mod.rs).
        asset_service.register_decoder::<SoundData>("audio", SymphoniaDecoder);
        // Mesh dispatch: gltf URIs (external `.bin` / texture buffers) are
        // resolved relative to the project's `assets/` root. Authors place
        // referenced resources at project-relative paths (e.g.
        // `meshes/character/diffuse.png`) and reference them with that same
        // path inside the gltf — this diverges from the strict gltf-spec
        // "URIs are relative to the gltf file" but matches the rest of the
        // VFS's path convention. `.glb` and `.obj` files are self-contained
        // and don't go through the resolver at all.
        let gltf_resolver = Arc::new(FileSystemResolver::new(&assets_root));
        asset_service.register_decoder::<Mesh>("mesh", MeshDispatcher::new(gltf_resolver));

        let watcher = AssetWatcher::new(&assets_root)
            .context("Failed to start asset watcher (filesystem hot reload)")?;

        log::info!(
            "ProjectVfs opened: {} assets indexed under {}, watcher armed.",
            asset_service.vfs().asset_count(),
            assets_root.display()
        );

        Ok(Self {
            root,
            assets_root,
            asset_service,
            watcher,
            file_loader,
            registry,
        })
    }

    /// Re-walks `<root>/assets/` and atomically swaps the VFS index. Called
    /// after a scene save (so the new file shows up) and from the
    /// hot-reload pump on Created/Removed events.
    pub fn rebuild_index(&mut self) -> Result<()> {
        let bytes = IndexBuilder::new(&self.assets_root)
            .with_registry(&self.registry)
            .build_index_bytes()
            .context("Failed to rebuild project asset index")?;
        self.asset_service.reindex(&bytes)?;
        Ok(())
    }

    /// Reads `<root>/project.json` as untyped JSON. Project metadata isn't an
    /// asset (lives outside `assets/`) so it deliberately bypasses the VFS.
    pub fn read_project_json(&self) -> Result<serde_json::Value> {
        let path = self.root.join("project.json");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Writes `bytes` to `<root>/assets/<rel_path>`. Creates intermediate
    /// directories as needed. Caller is responsible for calling
    /// [`Self::rebuild_index`] afterwards if the new path needs to be
    /// resolvable through the VFS in the same frame.
    pub fn write_asset(&self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        self.file_loader.write_bytes(rel_path, bytes)
    }

    /// Returns the UUID for a relative-path-with-forward-slashes string,
    /// regardless of whether the path currently exists on disk. Used by
    /// `scene_io` to look up scenes by canonical path even before a save
    /// has triggered a reindex.
    ///
    /// This is the raw **path-derived default** and ignores the identity
    /// registry. Prefer [`Self::resolve_uuid`] whenever the asset may have been
    /// renamed (frozen identity); use this only for paths that cannot have a
    /// frozen entry (e.g. a file being written for the very first time).
    pub fn uuid_for_rel_path(rel_path_fwd_slash: &str) -> AssetUUID {
        AssetUUID::new_v5(rel_path_fwd_slash)
    }

    /// Resolves a forward-slash relative path to its UUID through the identity
    /// registry: the frozen identity if the asset has been renamed, otherwise
    /// the `new_v5` default. This is the registry-aware counterpart to
    /// [`Self::uuid_for_rel_path`] and must be used whenever an asset the user
    /// could have renamed is looked up by path (scene / prefab / material load).
    pub fn resolve_uuid(&self, rel_path_fwd_slash: &str) -> AssetUUID {
        self.registry.resolve(rel_path_fwd_slash)
    }

    /// Absolute path of an asset given its forward-slash relative path.
    fn abs_of(&self, rel_fwd: &str) -> PathBuf {
        let mut p = self.assets_root.clone();
        for seg in rel_fwd.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }

    /// Creates an (empty) folder under `assets/` at `rel_dir` (forward-slash).
    /// Empty folders are surfaced by [`Self::list_dirs`] since the VFS itself
    /// only indexes files.
    pub fn create_folder(&mut self, rel_dir: &str) -> Result<()> {
        let abs = self.abs_of(rel_dir);
        std::fs::create_dir_all(&abs)
            .with_context(|| format!("Failed to create folder {}", abs.display()))?;
        Ok(())
    }

    /// Renames/moves an asset from `old_rel` to `new_rel` (both forward-slash,
    /// under `assets/`), **freezing its UUID** so every existing reference keeps
    /// resolving. Fails if the destination already exists. Rebuilds the index.
    pub fn rename_asset(&mut self, old_rel: &str, new_rel: &str) -> Result<()> {
        if old_rel == new_rel {
            return Ok(());
        }
        let old_abs = self.abs_of(old_rel);
        let new_abs = self.abs_of(new_rel);
        if !old_abs.exists() {
            bail!("Source asset does not exist: {}", old_abs.display());
        }
        if new_abs.exists() {
            bail!("Destination already exists: {}", new_abs.display());
        }
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::rename(&old_abs, &new_abs)
            .with_context(|| format!("Failed to move {} → {}", old_abs.display(), new_abs.display()))?;

        // Freeze identity across the move, then persist. In-memory update must
        // precede `rebuild_index` so the new path resolves to the frozen UUID.
        self.registry.rename(old_rel, new_rel);
        if let Err(e) = self.registry.save() {
            log::error!("Failed to persist asset registry after rename: {e}");
        }
        self.rebuild_index()
    }

    /// Moves an asset into `dest_dir` (forward-slash folder, `""` = assets root),
    /// keeping its file name. Thin wrapper over [`Self::rename_asset`].
    pub fn move_asset(&mut self, src_rel: &str, dest_dir: &str) -> Result<()> {
        let file_name = src_rel.rsplit('/').next().unwrap_or(src_rel);
        let new_rel = if dest_dir.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", dest_dir.trim_end_matches('/'), file_name)
        };
        self.rename_asset(src_rel, &new_rel)
    }

    /// Sends an asset to the OS recycle bin (reversible), drops its registry
    /// entry, and rebuilds the index.
    pub fn delete_to_trash(&mut self, rel: &str) -> Result<()> {
        let abs = self.abs_of(rel);
        if !abs.exists() {
            bail!("Asset does not exist: {}", abs.display());
        }
        trash::delete(&abs)
            .with_context(|| format!("Failed to move {} to the recycle bin", abs.display()))?;
        self.registry.remove(rel);
        if let Err(e) = self.registry.save() {
            log::error!("Failed to persist asset registry after delete: {e}");
        }
        self.rebuild_index()
    }

    /// Duplicates an asset next to itself with a unique ` copy` suffix. The copy
    /// is a **new asset** (no registry entry → fresh `new_v5` identity). Returns
    /// the new forward-slash relative path.
    pub fn duplicate_asset(&mut self, rel: &str) -> Result<String> {
        let src_abs = self.abs_of(rel);
        if !src_abs.exists() {
            bail!("Asset does not exist: {}", src_abs.display());
        }
        let (dir, file) = match rel.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), rel.to_string()),
        };
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (file.clone(), String::new()),
        };
        // Find the first free `stem copy`, `stem copy 2`, … name.
        let mut new_rel = String::new();
        for n in 1..10_000 {
            let suffix = if n == 1 {
                " copy".to_string()
            } else {
                format!(" copy {n}")
            };
            let candidate_file = format!("{stem}{suffix}{ext}");
            let candidate = if dir.is_empty() {
                candidate_file
            } else {
                format!("{dir}/{candidate_file}")
            };
            if !self.abs_of(&candidate).exists() {
                new_rel = candidate;
                break;
            }
        }
        if new_rel.is_empty() {
            bail!("Could not find a free duplicate name for {rel}");
        }
        std::fs::copy(&src_abs, self.abs_of(&new_rel))
            .with_context(|| format!("Failed to duplicate {}", src_abs.display()))?;
        self.rebuild_index()?;
        Ok(new_rel)
    }

    /// Lists every directory under `assets/` as forward-slash relative paths
    /// (sorted, excluding the root). The VFS only indexes files, so empty
    /// folders — including freshly created ones — are only visible through this.
    pub fn list_dirs(&self) -> Vec<String> {
        let mut dirs = Vec::new();
        for entry in walkdir::WalkDir::new(&self.assets_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let rel = match entry.path().strip_prefix(&self.assets_root) {
                Ok(r) if !r.as_os_str().is_empty() => r,
                _ => continue,
            };
            // Skip the engine's own `.khora` project dir if it ever sits inside.
            let rel_fwd = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if rel_fwd.starts_with('.') {
                continue;
            }
            dirs.push(rel_fwd);
        }
        dirs.sort();
        dirs
    }

    /// Drains pending hot-reload events. Convenience wrapper so callers
    /// don't need to reach through `pvfs.watcher`.
    pub fn poll_changes(&self) -> Vec<AssetChangeEvent> {
        self.watcher.poll()
    }
}
